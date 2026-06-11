//! Tests e2e B9b-1 — CRUD des clés API (`/api/api-keys`).
//!
//! Prérequis : 3 bases réelles + schéma + seed. Org JETABLE par test (admin/manager/lecteur).

use serde_json::{json, Value};

mod common;
use common::{access_token, create_test_org, pg_pool, spawn_app, TestOrg};

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

async fn admin_token(base: &str, org: &TestOrg) -> String {
    access_token(base, &org.admin_email).await
}

#[tokio::test]
async fn api_key_lifecycle() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = admin_token(&base, &org).await;

    // Création : secret renvoyé UNE fois, prefix = 8 premiers caractères.
    let (st, body) = post(
        &base,
        &token,
        "/api/api-keys",
        json!({ "name": "Intégration ville", "scope": "read" }),
    )
    .await;
    assert_eq!(st, 201, "création : {body:?}");
    let id = body["id"].as_i64().expect("id");
    let secret = body["secret"]
        .as_str()
        .expect("secret en clair")
        .to_string();
    assert!(secret.starts_with("qrt_"), "préfixe secret : {secret}");
    assert_eq!(body["token_prefix"].as_str(), Some(&secret[..8]));
    assert_eq!(body["scope"], "read");
    assert!(body["revoked_at"].is_null());

    // Listing : la clé est là, SANS secret ni hash.
    let (st, list) = get(&base, &token, "/api/api-keys").await;
    assert_eq!(st, 200);
    let k = list
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["id"].as_i64() == Some(id))
        .expect("clé listée");
    assert!(
        k.get("secret").is_none(),
        "le listing ne doit JAMAIS exposer le secret"
    );
    assert!(k.get("token_hash").is_none(), "ni le hash");

    // Révocation : 204, puis la clé apparaît révoquée, et re-révoquer → 404.
    assert_eq!(
        delete(&base, &token, &format!("/api/api-keys/{id}")).await,
        204
    );
    let (_, list) = get(&base, &token, "/api/api-keys").await;
    let k = list
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["id"].as_i64() == Some(id))
        .unwrap();
    assert!(!k["revoked_at"].is_null(), "revoked_at renseigné");
    assert_eq!(
        delete(&base, &token, &format!("/api/api-keys/{id}")).await,
        404,
        "déjà révoquée"
    );
}

#[tokio::test]
async fn created_secret_is_hashed_not_stored_plaintext() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = admin_token(&base, &org).await;

    let (st, body) = post(&base, &token, "/api/api-keys", json!({ "name": "K" })).await;
    assert_eq!(st, 201, "{body:?}");
    let id = body["id"].as_i64().expect("id");
    let secret = body["secret"].as_str().expect("secret").to_string();

    // En base : le hash N'EST PAS le secret en clair, et fait 64 hex (SHA-256). Le prefix
    // (8 car. en clair) correspond au début du secret.
    let row: (String, String) =
        sqlx::query_as("SELECT token_hash, token_prefix FROM api_tokens WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("clé en base");
    assert_ne!(row.0, secret, "le secret NE DOIT PAS être stocké en clair");
    assert_eq!(row.0.len(), 64, "hash SHA-256 hex");
    assert_eq!(
        row.1,
        secret[..8],
        "token_prefix = 8 premiers caractères du secret"
    );
}

#[tokio::test]
async fn api_keys_are_org_scoped() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;
    let token_a = admin_token(&base, &org_a).await;
    let token_b = admin_token(&base, &org_b).await;

    let (st, body) = post(&base, &token_a, "/api/api-keys", json!({ "name": "A" })).await;
    assert_eq!(st, 201, "{body:?}");
    let id = body["id"].as_i64().expect("id");

    // org B : ne voit pas la clé d'org A, et ne peut pas la révoquer (404).
    let (_, list) = get(&base, &token_b, "/api/api-keys").await;
    assert!(list
        .as_array()
        .unwrap()
        .iter()
        .all(|k| k["id"].as_i64() != Some(id)));
    assert_eq!(
        delete(&base, &token_b, &format!("/api/api-keys/{id}")).await,
        404
    );
}

#[tokio::test]
async fn api_keys_require_admin() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;

    // Le gestionnaire (écriture mais pas admin) et le lecteur sont refusés (403).
    for email in [&org.manager_email, &org.reader_email] {
        let token = access_token(&base, email).await;
        let (st, _) = post(&base, &token, "/api/api-keys", json!({ "name": "X" })).await;
        assert_eq!(st, 403, "non-admin refusé en création");
        assert_eq!(
            get(&base, &token, "/api/api-keys").await.0,
            403,
            "non-admin refusé en listing"
        );
    }
}
