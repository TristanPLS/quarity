//! Tests e2e B9a-3 — calcul de dose d'exposition (`POST …/compute-dose`, `GET …/results`).
//!
//! Prérequis : 3 bases réelles + schéma + seed. Scénario DÉTERMINISTE : station
//! synthétique (plage 999e9+), tz=UTC (local == UTC) et fenêtre/jours larges pour que
//! le comptage des heures > seuil soit prévisible.

use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Duration, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;

mod common;
use common::{access_token, create_test_org, pg_pool, spawn_app};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers HTTP
// ─────────────────────────────────────────────────────────────────────────────

async fn post(base: &str, token: &str, path: &str, body: Value) -> (u16, Value) {
    let res = reqwest::Client::new()
        .post(format!("{base}{path}"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .expect("POST");
    let status = res.status().as_u16();
    (status, res.json::<Value>().await.unwrap_or(Value::Null))
}

/// POST sans corps (compute-dose : tout est en query string).
async fn post_empty(base: &str, token: &str, path: &str) -> (u16, Value) {
    let res = reqwest::Client::new()
        .post(format!("{base}{path}"))
        .bearer_auth(token)
        .send()
        .await
        .expect("POST");
    let status = res.status().as_u16();
    (status, res.json::<Value>().await.unwrap_or(Value::Null))
}

async fn get(base: &str, token: &str, path: &str) -> (u16, Value) {
    let res = reqwest::Client::new()
        .get(format!("{base}{path}"))
        .bearer_auth(token)
        .send()
        .await
        .expect("GET");
    let status = res.status().as_u16();
    (status, res.json::<Value>().await.unwrap_or(Value::Null))
}

// ─────────────────────────────────────────────────────────────────────────────
// Setup
// ─────────────────────────────────────────────────────────────────────────────

static NEXT_STATION: AtomicU64 = AtomicU64::new(0);

fn new_station_id() -> i64 {
    let ts = Utc::now().timestamp() as u64;
    (999_000_000_000 + ts * 1_000 + NEXT_STATION.fetch_add(1, Ordering::Relaxed)) as i64
}

async fn register_station(pool: &PgPool, openaq_location_id: i64) {
    sqlx::query(
        "INSERT INTO ref_locations (openaq_location_id, name, country, city, latitude, longitude, timezone)
         VALUES ($1, $2, 'FR', 'Ville-Test', 43.7, 7.27, 'Europe/Paris')",
    )
    .bind(openaq_location_id)
    .bind(format!("Station dose {openaq_location_id}"))
    .execute(pool)
    .await
    .expect("insertion station");
}

/// Insère UNE mesure brute dans ClickHouse (`ingested_at` = maintenant).
async fn insert_ch(location_id: i64, parameter: &str, value: f64, measured_at: &str) {
    let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let sql = format!(
        "INSERT INTO quarity.measurements \
         (location_id, sensor_id, parameter, country, unit, measured_at, value, latitude, longitude, ingested_at) \
         VALUES ({location_id}, 7, '{parameter}', 'FR', 'µg/m³', '{measured_at}', {value}, 43.7, 7.27, '{now}')"
    );
    let resp = reqwest::Client::new()
        .post(std::env::var("CLICKHOUSE_URL").expect("CLICKHOUSE_URL"))
        .basic_auth(
            std::env::var("CLICKHOUSE_USER").expect("CLICKHOUSE_USER"),
            Some(std::env::var("CLICKHOUSE_PASSWORD").expect("CLICKHOUSE_PASSWORD")),
        )
        .query(&[(
            "database",
            std::env::var("CLICKHOUSE_DB").expect("CLICKHOUSE_DB"),
        )])
        .body(sql)
        .send()
        .await
        .expect("INSERT ClickHouse");
    assert!(
        resp.status().is_success(),
        "insert CH : {}",
        resp.text().await.unwrap_or_default()
    );
}

async fn create_location(base: &str, token: &str, station: i64) -> i64 {
    let (st, body) = post(
        base,
        token,
        "/api/tracked-locations",
        json!({
            "name": format!("Lieu dose {station}"),
            "openaq_location_ids": [station],
            "rules": [{ "parameter": "pm25", "comparator": ">", "threshold_value": 15.0, "severity": "warning", "name": "Regle dose e2e" }]
        }),
    )
    .await;
    assert_eq!(st, 201, "création lieu : {body:?}");
    body["id"].as_i64().expect("location id")
}

/// Profil custom + 1 seuil pm25 > 15 (1h) ; renvoie l'id du profil.
async fn create_profile_with_threshold(base: &str, token: &str) -> i64 {
    let (st, body) = post(
        base,
        token,
        "/api/exposure-profiles",
        json!({ "code": "general", "name": "Dose G" }),
    )
    .await;
    assert_eq!(st, 201, "{body:?}");
    let pid = body["id"].as_i64().expect("profile id");
    let (st, body) = post(
        base,
        token,
        &format!("/api/exposure-profiles/{pid}/thresholds"),
        json!({ "parameter": "pm25", "threshold_value": 15.0, "averaging_period": "1h" }),
    )
    .await;
    assert_eq!(st, 201, "seuil : {body:?}");
    pid
}

/// Associe profil×lieu, fenêtre 08:00–17:00, tous les jours, tz UTC (local == UTC).
async fn create_tlp(base: &str, token: &str, loc: i64, profile: i64) -> i64 {
    let (st, body) = post(
        base,
        token,
        "/api/tracked-location-profiles",
        json!({ "tracked_location_id": loc, "exposure_profile_id": profile,
                "start_time": "08:00", "end_time": "17:00", "days_mask": 127, "timezone": "UTC" }),
    )
    .await;
    assert_eq!(st, 201, "tlp : {body:?}");
    body["id"].as_i64().expect("tlp id")
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn compute_dose_counts_hours_over_threshold() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;

    let station = new_station_id();
    register_station(&pool, station).await;
    let loc = create_location(&base, &token, station).await;
    let profile = create_profile_with_threshold(&base, &token).await;
    let tlp = create_tlp(&base, &token, loc, profile).await;

    // Jour récent (dans le TTL ; tz=UTC ⇒ heure locale = UTC). 5 heures avec donnée
    // dans la fenêtre 08–17 : 08=20, 09=10, 10=30, 12=5, 16=25 → 3 heures > 15.
    let day = (Utc::now() - Duration::days(7))
        .format("%Y-%m-%d")
        .to_string();
    for (h, v) in [(8, 20.0), (9, 10.0), (10, 30.0), (12, 5.0), (16, 25.0)] {
        insert_ch(station, "pm25", v, &format!("{day} {h:02}:15:00")).await;
    }

    let (st, body) = post_empty(
        &base,
        &token,
        &format!(
            "/api/tracked-location-profiles/{tlp}/compute-dose?period_start={day}&period_end={day}"
        ),
    )
    .await;
    assert_eq!(st, 200, "compute-dose : {body:?}");
    let results = body.as_array().expect("liste de résultats");
    assert_eq!(
        results.len(),
        1,
        "un résultat (le seul seuil 1h) : {body:?}"
    );
    let r = &results[0];
    assert_eq!(r["parameter"], "pm25");
    assert_eq!(r["threshold_value"].as_f64(), Some(15.0));
    assert_eq!(
        r["hours_over_threshold"].as_f64(),
        Some(3.0),
        "3 heures > 15 : {r:?}"
    );
    assert_eq!(
        r["sample_count"].as_i64(),
        Some(5),
        "5 heures évaluées : {r:?}"
    );

    // GET …/results relit le cache.
    let (st, list) = get(
        &base,
        &token,
        &format!("/api/tracked-location-profiles/{tlp}/results"),
    )
    .await;
    assert_eq!(st, 200);
    assert_eq!(list.as_array().map(Vec::len), Some(1));
    assert_eq!(list[0]["hours_over_threshold"].as_f64(), Some(3.0));
}

#[tokio::test]
async fn compute_dose_is_org_scoped() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;
    let token_a = access_token(&base, &org_a.admin_email).await;
    let token_b = access_token(&base, &org_b.admin_email).await;

    let station = new_station_id();
    register_station(&pool, station).await;
    let loc = create_location(&base, &token_a, station).await;
    let profile = create_profile_with_threshold(&base, &token_a).await;
    let tlp = create_tlp(&base, &token_a, loc, profile).await;

    let day = (Utc::now() - Duration::days(7))
        .format("%Y-%m-%d")
        .to_string();
    // org B : compute-dose et results sur l'association d'org A → 404.
    assert_eq!(
        post_empty(&base, &token_b,
            &format!("/api/tracked-location-profiles/{tlp}/compute-dose?period_start={day}&period_end={day}")).await.0,
        404
    );
    assert_eq!(
        get(
            &base,
            &token_b,
            &format!("/api/tracked-location-profiles/{tlp}/results")
        )
        .await
        .0,
        404
    );
}

#[tokio::test]
async fn compute_dose_rejects_inverted_period() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;

    let station = new_station_id();
    register_station(&pool, station).await;
    let loc = create_location(&base, &token, station).await;
    let profile = create_profile_with_threshold(&base, &token).await;
    let tlp = create_tlp(&base, &token, loc, profile).await;

    // period_end < period_start → 400.
    let (st, _) = post_empty(
        &base,
        &token,
        &format!("/api/tracked-location-profiles/{tlp}/compute-dose?period_start=2026-05-31&period_end=2026-05-01"),
    )
    .await;
    assert_eq!(st, 400, "période inversée rejetée");
}

#[tokio::test]
async fn compute_dose_with_only_annual_threshold_returns_empty() {
    // Un profil dont le SEUL seuil est `annual` (hors périmètre B9a-3) : compute-dose
    // renvoie une liste vide, sans erreur (les seuils annuels sont ignorés).
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;

    let station = new_station_id();
    register_station(&pool, station).await;
    let loc = create_location(&base, &token, station).await;

    // Profil custom avec UNIQUEMENT un seuil annual.
    let (_, body) = post(
        &base,
        &token,
        "/api/exposure-profiles",
        json!({ "code": "general", "name": "Annual seul" }),
    )
    .await;
    let pid = body["id"].as_i64().expect("profile id");
    let (st, _) = post(
        &base,
        &token,
        &format!("/api/exposure-profiles/{pid}/thresholds"),
        json!({ "parameter": "no2", "threshold_value": 40.0, "averaging_period": "annual" }),
    )
    .await;
    assert_eq!(st, 201);
    let tlp = create_tlp(&base, &token, loc, pid).await;

    let day = (Utc::now() - Duration::days(7))
        .format("%Y-%m-%d")
        .to_string();
    let (st, body) = post_empty(
        &base,
        &token,
        &format!(
            "/api/tracked-location-profiles/{tlp}/compute-dose?period_start={day}&period_end={day}"
        ),
    )
    .await;
    assert_eq!(st, 200, "{body:?}");
    assert_eq!(
        body.as_array().map(Vec::len),
        Some(0),
        "seuils annual ignorés → aucun résultat"
    );
}
