//! Tests e2e B9b-2 — API publique authentifiée par clé API (`X-API-Key`) + quota.
//!
//! Prérequis : 3 bases réelles + schéma + seed. Org JETABLE par test (sans abonnement
//! ⇒ plan par défaut « free » : 1000/mois, 30/min côté extracteur).

use serde_json::{json, Value};

mod common;
use common::{access_token, create_test_org, pg_pool, spawn_app};

/// POST authentifié par JWT (pour émettre une clé).
async fn post_jwt(base: &str, token: &str, path: &str, body: Value) -> (u16, Value) {
    let res = reqwest::Client::new()
        .post(format!("{base}{path}"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .expect("POST");
    (
        res.status().as_u16(),
        res.json::<Value>().await.unwrap_or(Value::Null),
    )
}

/// DELETE authentifié par JWT (révocation de clé).
async fn delete_jwt(base: &str, token: &str, path: &str) -> u16 {
    reqwest::Client::new()
        .delete(format!("{base}{path}"))
        .bearer_auth(token)
        .send()
        .await
        .expect("DELETE")
        .status()
        .as_u16()
}

/// GET de l'API publique avec (ou sans) en-tête `X-API-Key`.
async fn get_public(base: &str, key: Option<&str>, path: &str) -> (u16, Value) {
    let mut req = reqwest::Client::new().get(format!("{base}{path}"));
    if let Some(k) = key {
        req = req.header("X-API-Key", k);
    }
    let res = req.send().await.expect("GET public");
    (
        res.status().as_u16(),
        res.json::<Value>().await.unwrap_or(Value::Null),
    )
}

/// Émet une clé API via l'admin JWT ; renvoie `(secret, key_id)`.
async fn issue_key(base: &str, admin_token: &str) -> (String, i64) {
    let (st, body) = post_jwt(
        base,
        admin_token,
        "/api/api-keys",
        json!({ "name": "Clé publique e2e" }),
    )
    .await;
    assert_eq!(st, 201, "émission clé : {body:?}");
    (
        body["secret"].as_str().expect("secret").to_string(),
        body["id"].as_i64().expect("id"),
    )
}

#[tokio::test]
async fn public_aqi_with_valid_key() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let admin = access_token(&base, &org.admin_email).await;
    let (secret, _) = issue_key(&base, &admin).await;

    // L'org jetable n'a aucun lieu : 200 avec data=[] (la clé résout bien l'org).
    let (st, body) = get_public(&base, Some(&secret), "/api/public/aqi").await;
    assert_eq!(st, 200, "clé valide : {body:?}");
    assert!(body.get("data").is_some(), "réponse AQI : {body:?}");
}

#[tokio::test]
async fn public_api_rejects_missing_invalid_or_revoked_key() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let admin = access_token(&base, &org.admin_email).await;

    // Pas de clé → 401.
    assert_eq!(get_public(&base, None, "/api/public/aqi").await.0, 401);
    // Clé bidon → 401.
    assert_eq!(
        get_public(&base, Some("qrt_pas_une_vraie_cle"), "/api/public/aqi")
            .await
            .0,
        401
    );

    // Clé valide puis RÉVOQUÉE → 401 (révocation immédiate).
    let (secret, id) = issue_key(&base, &admin).await;
    assert_eq!(
        get_public(&base, Some(&secret), "/api/public/aqi").await.0,
        200
    );
    assert_eq!(
        delete_jwt(&base, &admin, &format!("/api/api-keys/{id}")).await,
        204
    );
    assert_eq!(
        get_public(&base, Some(&secret), "/api/public/aqi").await.0,
        401,
        "une clé révoquée ne s'authentifie plus"
    );
}

#[tokio::test]
async fn public_measurements_is_org_scoped() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let admin = access_token(&base, &org.admin_email).await;
    let (secret, _) = issue_key(&base, &admin).await;

    // Station NON suivie par l'org de la clé → 403 (isolation).
    let (st, _) = get_public(
        &base,
        Some(&secret),
        "/api/public/measurements?location_id=999999999&parameter=pm25&from=2026-05-01&to=2026-05-02",
    )
    .await;
    assert_eq!(st, 403, "station étrangère refusée");
}

#[tokio::test]
async fn public_api_enforces_rate_limit() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let admin = access_token(&base, &org.admin_email).await;
    let (secret, _) = issue_key(&base, &admin).await;

    // Plan par défaut (pas d'abo) = 30 req/min. Les 30 premières passent, la 31ᵉ → 429.
    for i in 1..=30 {
        let st = get_public(&base, Some(&secret), "/api/public/aqi").await.0;
        assert_eq!(st, 200, "requête {i} doit passer (≤ 30/min)");
    }
    let st = get_public(&base, Some(&secret), "/api/public/aqi").await.0;
    assert_eq!(st, 429, "la 31ᵉ requête dépasse le rate-limit du plan");
}
