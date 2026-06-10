//! Tests e2e de la vue d'ensemble AQI B10 (`GET /api/aqi`).
//!
//! Prérequis : identiques à `tests/b7_matching.rs` / `tests/ch.rs` (Postgres +
//! ClickHouse réels, schéma + seed, variables d'env exportées). L'endpoint réutilise
//! la requête B5 Q2 (`aqi_snapshot`) — ces tests prouvent le CHEMIN HTTP (isolation,
//! agrégation par lieu), la correction du calcul AQI lui-même étant déjà couverte par
//! `tests/ch.rs`.
//!
//! Isolation : org JETABLE + station synthétique unique par test (plage 999e9+) — la
//! vue d'ensemble étant scopée à l'org du JWT, deux tests parallèles ne se voient pas.

use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;

mod common;
use common::{access_token, create_test_org, pg_pool, spawn_app};

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
    .bind(format!("Station e2e {openaq_location_id}"))
    .execute(pool)
    .await
    .expect("insertion station synthétique");
}

fn fmt_ch(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

/// Heure pleine courante (UTC) — l'ancre de la fenêtre AQI (`toStartOfHour(now)`).
fn current_hour() -> DateTime<Utc> {
    let secs = Utc::now().timestamp();
    DateTime::<Utc>::from_timestamp(secs - secs.rem_euclid(3_600), 0).expect("heure pile")
}

async fn insert_ch_measurement(
    location_id: i64,
    sensor_id: i64,
    parameter: &str,
    measured_at: DateTime<Utc>,
    value: f64,
) {
    let sql = format!(
        "INSERT INTO quarity.measurements \
         (location_id, sensor_id, parameter, country, unit, measured_at, \
          value, latitude, longitude, ingested_at) \
         VALUES ({location_id}, {sensor_id}, '{parameter}', 'FR', 'µg/m³', '{}', {value}, 43.7, 7.27, '{}')",
        fmt_ch(measured_at),
        fmt_ch(Utc::now()),
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
        .expect("requête HTTP ClickHouse");
    assert!(
        resp.status().is_success(),
        "insertion ClickHouse refusée : {}",
        resp.text().await.unwrap_or_default()
    );
}

/// 20 buckets horaires d'un même polluant à valeur constante dans la fenêtre 24 h
/// close à l'heure courante (couverture 20/24 ≈ 0.83 ≥ 75 % ⇒ AQI réglementairement valide).
async fn insert_valid_pm25_series(station: i64, value: f64) {
    let anchor = current_hour();
    for i in 1..=20 {
        insert_ch_measurement(station, 700 + i, "pm25", anchor - Duration::hours(i), value).await;
    }
}

/// Crée un lieu suivi (1 station) via l'API ; renvoie son id (= tracked_location_id).
async fn create_tracked_location(base: &str, token: &str, station: i64) -> i64 {
    let res = reqwest::Client::new()
        .post(format!("{base}/api/tracked-locations"))
        .bearer_auth(token)
        .json(&json!({
            "name": format!("Lieu AQI {station}"),
            "openaq_location_ids": [station],
            "rules": [{
                "parameter": "pm25", "comparator": ">",
                "threshold_value": 15.0, "severity": "warning"
            }]
        }))
        .send()
        .await
        .expect("create location");
    assert_eq!(res.status().as_u16(), 201, "création lieu");
    let body: Value = res.json().await.expect("corps 201");
    body["id"].as_i64().expect("location id")
}

async fn fetch_aqi(base: &str, token: &str) -> Value {
    let res = reqwest::Client::new()
        .get(format!("{base}/api/aqi"))
        .bearer_auth(token)
        .send()
        .await
        .expect("GET /api/aqi");
    assert_eq!(res.status().as_u16(), 200, "AQI 200");
    res.json().await.expect("corps AQI")
}

/// Élément `data[]` de la vue d'ensemble pour un lieu donné.
fn location_in(body: &Value, location_id: i64) -> Option<&Value> {
    body["data"]
        .as_array()?
        .iter()
        .find(|l| l["tracked_location_id"].as_i64() == Some(location_id))
}

#[tokio::test]
async fn aqi_overview_reports_epa_aqi_for_tracked_location() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;

    let station = new_station_id();
    register_station(&pool, station).await;
    let location_id = create_tracked_location(&base, &token, station).await;

    // pm25 constant à 20 µg/m³ sur la fenêtre 24 h ⇒ AQI EPA = 71 (cf. tests/ch.rs).
    insert_valid_pm25_series(station, 20.0).await;

    let body = fetch_aqi(&base, &token).await;
    let loc = location_in(&body, location_id).expect("le lieu doit figurer dans la vue d'ensemble");

    assert_eq!(loc["has_data"].as_bool(), Some(true));
    assert_eq!(loc["overall_aqi"].as_i64(), Some(71), "{loc:?}");
    assert_eq!(loc["dominant_parameter"], "pm25");
    assert_eq!(
        loc["valid"].as_bool(),
        Some(true),
        "couverture 20/24 ≥ 75 %"
    );
    assert_eq!(loc["name"], format!("Lieu AQI {station}"));

    let pm25 = loc["pollutants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["parameter"] == "pm25")
        .expect("polluant pm25 présent");
    assert_eq!(pm25["aqi"].as_i64(), Some(71));
    assert_eq!(pm25["is_dominant"].as_bool(), Some(true));
    assert_eq!(pm25["is_valid"].as_bool(), Some(true));
}

#[tokio::test]
async fn aqi_overview_is_org_scoped() {
    // La vue d'ensemble d'une org ne contient JAMAIS le lieu d'une autre org.
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;
    let token_a = access_token(&base, &org_a.admin_email).await;
    let token_b = access_token(&base, &org_b.admin_email).await;

    let station = new_station_id();
    register_station(&pool, station).await;
    let location_a = create_tracked_location(&base, &token_a, station).await;
    insert_valid_pm25_series(station, 20.0).await;

    // org A voit son lieu.
    let body_a = fetch_aqi(&base, &token_a).await;
    assert!(
        location_in(&body_a, location_a).is_some(),
        "org A doit voir son lieu"
    );

    // org B ne le voit PAS (isolation).
    let body_b = fetch_aqi(&base, &token_b).await;
    assert!(
        location_in(&body_b, location_a).is_none(),
        "org B ne doit pas voir le lieu de l'org A"
    );
}

#[tokio::test]
async fn aqi_overview_lists_location_without_recent_data() {
    // Un lieu sans mesure récente sort quand même, marqué has_data = false.
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;

    let station = new_station_id();
    register_station(&pool, station).await;
    let location_id = create_tracked_location(&base, &token, station).await;
    // AUCUNE mesure insérée pour cette station.

    let body = fetch_aqi(&base, &token).await;
    let loc = location_in(&body, location_id).expect("le lieu figure même sans donnée");
    assert_eq!(loc["has_data"].as_bool(), Some(false));
    assert!(
        loc["overall_aqi"].is_null(),
        "pas d'AQI sans donnée : {loc:?}"
    );
    assert!(loc["dominant_parameter"].is_null());
    assert_eq!(loc["valid"].as_bool(), Some(false));
    assert_eq!(loc["pollutants"].as_array().map(Vec::len), Some(0));
}
