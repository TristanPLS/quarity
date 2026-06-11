//! Tests e2e B9a — CRUD des profils d'exposition + seuils adaptés.
//!
//! Prérequis : 3 bases réelles + schéma + seed (profils SYSTÈME `enfants`/… seedés
//! avec `org_id NULL`). Chaque test a son org JETABLE (cf. `common::create_test_org`) —
//! les profils CUSTOM créés restent confinés au tenant du test.

use serde_json::{json, Value};

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
    let body = res.json::<Value>().await.unwrap_or(Value::Null);
    (status, body)
}

async fn get(base: &str, token: &str, path: &str) -> (u16, Value) {
    let res = reqwest::Client::new()
        .get(format!("{base}{path}"))
        .bearer_auth(token)
        .send()
        .await
        .expect("GET");
    let status = res.status().as_u16();
    let body = res.json::<Value>().await.unwrap_or(Value::Null);
    (status, body)
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
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn profile_crud_lifecycle_with_thresholds() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;

    // Création d'un profil custom.
    let (st, body) = post(
        &base,
        &token,
        "/api/exposure-profiles",
        json!({ "code": "sportifs", "name": "Sportifs amateurs", "description": "Effort en extérieur" }),
    )
    .await;
    assert_eq!(st, 201, "création profil : {body:?}");
    let pid = body["id"].as_i64().expect("id profil");
    assert_eq!(body["org_id"].as_i64(), Some(org.org_id));
    assert_eq!(body["is_system"], false);
    assert_eq!(body["code"], "sportifs");

    // Détail.
    let (st, body) = get(&base, &token, &format!("/api/exposure-profiles/{pid}")).await;
    assert_eq!(st, 200);
    assert_eq!(body["name"], "Sportifs amateurs");

    // PATCH (name + effacement description).
    let code = patch(
        &base,
        &token,
        &format!("/api/exposure-profiles/{pid}"),
        json!({ "name": "Sportifs", "description": "" }),
    )
    .await;
    assert_eq!(code, 200);
    let (_, body) = get(&base, &token, &format!("/api/exposure-profiles/{pid}")).await;
    assert_eq!(body["name"], "Sportifs");
    assert!(body["description"].is_null(), "description effacée");

    // Ajout d'un seuil.
    let (st, body) = post(
        &base,
        &token,
        &format!("/api/exposure-profiles/{pid}/thresholds"),
        json!({ "parameter": "pm25", "threshold_value": 12.5, "averaging_period": "1h" }),
    )
    .await;
    assert_eq!(st, 201, "création seuil : {body:?}");
    let tid = body["id"].as_i64().expect("id seuil");
    assert_eq!(body["parameter"], "pm25");
    assert_eq!(body["threshold_value"].as_f64(), Some(12.5));
    assert!(
        !body["unit"].as_str().unwrap_or("").is_empty(),
        "unité dérivée présente"
    );

    // Doublon (même polluant + période) → 409.
    let (st, _) = post(
        &base,
        &token,
        &format!("/api/exposure-profiles/{pid}/thresholds"),
        json!({ "parameter": "pm25", "threshold_value": 20.0, "averaging_period": "1h" }),
    )
    .await;
    assert_eq!(st, 409, "seuil en doublon");

    // Liste des seuils.
    let (st, body) = get(
        &base,
        &token,
        &format!("/api/exposure-profiles/{pid}/thresholds"),
    )
    .await;
    assert_eq!(st, 200);
    assert_eq!(body.as_array().map(Vec::len), Some(1));

    // Suppression du seuil, puis du profil.
    assert_eq!(
        delete(
            &base,
            &token,
            &format!("/api/exposure-profiles/{pid}/thresholds/{tid}")
        )
        .await,
        204
    );
    assert_eq!(
        delete(&base, &token, &format!("/api/exposure-profiles/{pid}")).await,
        204
    );
    let (st, _) = get(&base, &token, &format!("/api/exposure-profiles/{pid}")).await;
    assert_eq!(st, 404, "profil supprimé invisible");
}

#[tokio::test]
async fn custom_profile_is_invisible_cross_tenant() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;
    let token_a = access_token(&base, &org_a.admin_email).await;
    let token_b = access_token(&base, &org_b.admin_email).await;

    let (st, body) = post(
        &base,
        &token_a,
        "/api/exposure-profiles",
        json!({ "code": "general", "name": "Général A" }),
    )
    .await;
    assert_eq!(st, 201, "{body:?}");
    let pid = body["id"].as_i64().expect("id");

    // org B : GET/PATCH/DELETE + ajout de seuil → 404 (anti-énumération).
    assert_eq!(
        get(&base, &token_b, &format!("/api/exposure-profiles/{pid}"))
            .await
            .0,
        404
    );
    assert_eq!(
        patch(
            &base,
            &token_b,
            &format!("/api/exposure-profiles/{pid}"),
            json!({ "name": "X" })
        )
        .await,
        404
    );
    assert_eq!(
        delete(&base, &token_b, &format!("/api/exposure-profiles/{pid}")).await,
        404
    );
    assert_eq!(
        post(
            &base,
            &token_b,
            &format!("/api/exposure-profiles/{pid}/thresholds"),
            json!({ "parameter": "pm25", "threshold_value": 5.0 })
        )
        .await
        .0,
        404
    );

    // Le listing d'org B ne contient pas le profil custom d'org A.
    let (_, list) = get(&base, &token_b, "/api/exposure-profiles?page_size=100").await;
    let found = list["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"].as_i64() == Some(pid));
    assert!(
        !found,
        "le profil custom d'org A ne doit pas apparaître chez org B"
    );
}

#[tokio::test]
async fn system_profiles_are_visible_but_read_only() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;

    // Les profils système (seedés, org_id NULL) apparaissent en scope=system.
    let (st, list) = get(
        &base,
        &token,
        "/api/exposure-profiles?scope=system&page_size=100",
    )
    .await;
    assert_eq!(st, 200);
    let sys = list["data"].as_array().expect("data");
    assert!(!sys.is_empty(), "des profils système doivent être seedés");
    assert!(sys
        .iter()
        .all(|p| p["is_system"] == true && p["org_id"].is_null()));
    let sys_id = sys[0]["id"].as_i64().expect("id système");

    // Lecture seule : PATCH / DELETE d'un profil système → 404 (hors org de l'appelant).
    assert_eq!(
        patch(
            &base,
            &token,
            &format!("/api/exposure-profiles/{sys_id}"),
            json!({ "name": "X" })
        )
        .await,
        404
    );
    assert_eq!(
        delete(&base, &token, &format!("/api/exposure-profiles/{sys_id}")).await,
        404
    );
    // Et on ne peut pas lui ajouter de seuil.
    assert_eq!(
        post(
            &base,
            &token,
            &format!("/api/exposure-profiles/{sys_id}/thresholds"),
            json!({ "parameter": "pm25", "threshold_value": 5.0 })
        )
        .await
        .0,
        404
    );
}

#[tokio::test]
async fn reader_role_cannot_write() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.reader_email).await;

    let (st, _) = post(
        &base,
        &token,
        "/api/exposure-profiles",
        json!({ "code": "general", "name": "Tentative lecteur" }),
    )
    .await;
    assert_eq!(st, 403, "le rôle lecteur ne peut pas créer un profil");
}

#[tokio::test]
async fn duplicate_code_for_same_org_conflicts() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;

    let first = post(
        &base,
        &token,
        "/api/exposure-profiles",
        json!({ "code": "enfants", "name": "Enfants A" }),
    )
    .await;
    assert_eq!(first.0, 201, "{:?}", first.1);
    let second = post(
        &base,
        &token,
        "/api/exposure-profiles",
        json!({ "code": "enfants", "name": "Enfants bis" }),
    )
    .await;
    assert_eq!(second.0, 409, "même (org, code) → conflit");
}

#[tokio::test]
async fn threshold_with_unknown_parameter_is_rejected() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;

    let (_, body) = post(
        &base,
        &token,
        "/api/exposure-profiles",
        json!({ "code": "general", "name": "G" }),
    )
    .await;
    let pid = body["id"].as_i64().expect("id");

    // Polluant hors allowlist → 400 (rejet validation au boundary).
    let (st, _) = post(
        &base,
        &token,
        &format!("/api/exposure-profiles/{pid}/thresholds"),
        json!({ "parameter": "xenon", "threshold_value": 1.0 }),
    )
    .await;
    assert_eq!(st, 400, "polluant inconnu rejeté");
}
