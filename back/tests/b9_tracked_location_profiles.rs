//! Tests e2e B9a-2 — CRUD des associations lieu suivi × profil d'exposition.
//!
//! Prérequis : 3 bases réelles + schéma + seed. Chaque test a son org JETABLE, sa
//! station synthétique (plage 999e9+) et son lieu suivi créé via l'API.

use std::sync::atomic::{AtomicU64, Ordering};

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

async fn patch(base: &str, token: &str, path: &str, body: Value) -> u16 {
    reqwest::Client::new()
        .patch(format!("{base}{path}"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .expect("PATCH")
        .status()
        .as_u16()
}

async fn delete(base: &str, token: &str, path: &str) -> u16 {
    reqwest::Client::new()
        .delete(format!("{base}{path}"))
        .bearer_auth(token)
        .send()
        .await
        .expect("DELETE")
        .status()
        .as_u16()
}

// ─────────────────────────────────────────────────────────────────────────────
// Setup (station + lieu suivi + profil custom)
// ─────────────────────────────────────────────────────────────────────────────

static NEXT_STATION: AtomicU64 = AtomicU64::new(0);

fn new_station_id() -> i64 {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    (999_000_000_000 + ts * 1_000 + NEXT_STATION.fetch_add(1, Ordering::Relaxed)) as i64
}

async fn register_station(pool: &PgPool, openaq_location_id: i64) {
    sqlx::query(
        "INSERT INTO ref_locations (openaq_location_id, name, country, city, latitude, longitude, timezone)
         VALUES ($1, $2, 'FR', 'Ville-Test', 43.7, 7.27, 'Europe/Paris')",
    )
    .bind(openaq_location_id)
    .bind(format!("Station tlp {openaq_location_id}"))
    .execute(pool)
    .await
    .expect("insertion station synthétique");
}

/// Crée un lieu suivi (1 station + 1 règle) via l'API ; renvoie son id.
async fn create_location(base: &str, token: &str, station: i64) -> i64 {
    let (st, body) = post(
        base,
        token,
        "/api/tracked-locations",
        json!({
            "name": format!("Lieu tlp {station}"),
            "openaq_location_ids": [station],
            "rules": [{ "parameter": "pm25", "comparator": ">", "threshold_value": 15.0, "severity": "warning", "name": "Regle tlp e2e" }]
        }),
    )
    .await;
    assert_eq!(st, 201, "création lieu : {body:?}");
    body["id"].as_i64().expect("location id")
}

/// Crée un profil d'exposition custom ; renvoie son id.
async fn create_profile(base: &str, token: &str, code: &str) -> i64 {
    let (st, body) = post(
        base,
        token,
        "/api/exposure-profiles",
        json!({ "code": code, "name": format!("Profil {code}") }),
    )
    .await;
    assert_eq!(st, 201, "création profil : {body:?}");
    body["id"].as_i64().expect("profile id")
}

/// (lieu, profil) prêts à associer, pour l'org du token.
async fn setup(base: &str, token: &str, pool: &PgPool) -> (i64, i64) {
    let station = new_station_id();
    register_station(pool, station).await;
    let loc = create_location(base, token, station).await;
    let profile = create_profile(base, token, "general").await;
    (loc, profile)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn tlp_crud_lifecycle() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;
    let (loc, profile) = setup(&base, &token, &pool).await;

    // Création : enfants 08:00–17:00 Lun–Ven (days_mask 31).
    let (st, body) = post(
        &base,
        &token,
        "/api/tracked-location-profiles",
        json!({
            "tracked_location_id": loc, "exposure_profile_id": profile,
            "start_time": "08:00", "end_time": "17:00", "days_mask": 31
        }),
    )
    .await;
    assert_eq!(st, 201, "création tlp : {body:?}");
    let id = body["id"].as_i64().expect("id");
    assert_eq!(body["start_time"], "08:00:00");
    assert_eq!(body["end_time"], "17:00:00");
    assert_eq!(body["days_mask"].as_i64(), Some(31));
    assert_eq!(body["timezone"], "Europe/Paris");
    assert_eq!(body["is_active"], true);
    assert_eq!(body["exposure_profile_code"], "general");

    // Détail.
    assert_eq!(
        get(
            &base,
            &token,
            &format!("/api/tracked-location-profiles/{id}")
        )
        .await
        .0,
        200
    );

    // PATCH : fin à 18:00 + désactivation.
    assert_eq!(
        patch(
            &base,
            &token,
            &format!("/api/tracked-location-profiles/{id}"),
            json!({ "end_time": "18:00", "is_active": false })
        )
        .await,
        200
    );
    let (_, body) = get(
        &base,
        &token,
        &format!("/api/tracked-location-profiles/{id}"),
    )
    .await;
    assert_eq!(body["end_time"], "18:00:00");
    assert_eq!(body["is_active"], false);

    // Listing filtré par lieu.
    let (st, list) = get(
        &base,
        &token,
        &format!("/api/tracked-location-profiles?tracked_location_id={loc}"),
    )
    .await;
    assert_eq!(st, 200);
    assert_eq!(list["total"].as_i64(), Some(1));

    // Suppression.
    assert_eq!(
        delete(
            &base,
            &token,
            &format!("/api/tracked-location-profiles/{id}")
        )
        .await,
        204
    );
    assert_eq!(
        get(
            &base,
            &token,
            &format!("/api/tracked-location-profiles/{id}")
        )
        .await
        .0,
        404
    );
}

#[tokio::test]
async fn tlp_is_invisible_cross_tenant() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;
    let token_a = access_token(&base, &org_a.admin_email).await;
    let token_b = access_token(&base, &org_b.admin_email).await;
    let (loc, profile) = setup(&base, &token_a, &pool).await;

    let (st, body) = post(
        &base,
        &token_a,
        "/api/tracked-location-profiles",
        json!({ "tracked_location_id": loc, "exposure_profile_id": profile,
                "start_time": "08:00", "end_time": "17:00", "days_mask": 31 }),
    )
    .await;
    assert_eq!(st, 201, "{body:?}");
    let id = body["id"].as_i64().expect("id");

    // org B : invisible (404 sur tous les chemins).
    assert_eq!(
        get(
            &base,
            &token_b,
            &format!("/api/tracked-location-profiles/{id}")
        )
        .await
        .0,
        404
    );
    assert_eq!(
        patch(
            &base,
            &token_b,
            &format!("/api/tracked-location-profiles/{id}"),
            json!({ "is_active": false })
        )
        .await,
        404
    );
    assert_eq!(
        delete(
            &base,
            &token_b,
            &format!("/api/tracked-location-profiles/{id}")
        )
        .await,
        404
    );
    let (_, list) = get(
        &base,
        &token_b,
        "/api/tracked-location-profiles?page_size=100",
    )
    .await;
    let found = list["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["id"].as_i64() == Some(id));
    assert!(
        !found,
        "l'association d'org A ne doit pas apparaître chez org B"
    );
}

#[tokio::test]
async fn tlp_rejects_foreign_location() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;
    let token_a = access_token(&base, &org_a.admin_email).await;
    let token_b = access_token(&base, &org_b.admin_email).await;
    // Lieu + profil appartenant à org A.
    let (loc_a, profile_a) = setup(&base, &token_a, &pool).await;

    // org B tente d'attacher un profil au lieu d'org A → 404 (lieu invisible).
    let (st, _) = post(
        &base,
        &token_b,
        "/api/tracked-location-profiles",
        json!({ "tracked_location_id": loc_a, "exposure_profile_id": profile_a,
                "start_time": "08:00", "end_time": "17:00", "days_mask": 31 }),
    )
    .await;
    assert_eq!(st, 404, "lieu d'une autre org invisible");
}

#[tokio::test]
async fn tlp_duplicate_location_profile_conflicts() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;
    let (loc, profile) = setup(&base, &token, &pool).await;

    let body = json!({ "tracked_location_id": loc, "exposure_profile_id": profile,
                       "start_time": "08:00", "end_time": "17:00", "days_mask": 31 });
    assert_eq!(
        post(
            &base,
            &token,
            "/api/tracked-location-profiles",
            body.clone()
        )
        .await
        .0,
        201
    );
    // Même (lieu, profil) → 409 (uq_tlp).
    assert_eq!(
        post(&base, &token, "/api/tracked-location-profiles", body)
            .await
            .0,
        409
    );
}

#[tokio::test]
async fn tlp_invalid_window_is_rejected() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;
    let (loc, profile) = setup(&base, &token, &pool).await;

    // end <= start → CHECK chk_tlp_time_window → 422.
    let (st, _) = post(
        &base,
        &token,
        "/api/tracked-location-profiles",
        json!({ "tracked_location_id": loc, "exposure_profile_id": profile,
                "start_time": "17:00", "end_time": "09:00", "days_mask": 31 }),
    )
    .await;
    assert_eq!(st, 422, "plage end <= start refusée par la base");

    // days_mask hors 1..=127 → 400 (validation au boundary).
    let (st, _) = post(
        &base,
        &token,
        "/api/tracked-location-profiles",
        json!({ "tracked_location_id": loc, "exposure_profile_id": profile,
                "start_time": "08:00", "end_time": "17:00", "days_mask": 200 }),
    )
    .await;
    assert_eq!(st, 400, "days_mask hors borne rejeté au boundary");
}
