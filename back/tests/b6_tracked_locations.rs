//! Tests e2e du CRUD `/api/tracked-locations` (B6) — GABARIT des suites CRUD.
//!
//! Prérequis : identiques à `tests/e2e.rs` (3 bases réelles + schéma + seed).
//! Chaque test crée son org JETABLE (cf. `tests/common/mod.rs`) : la seed n'est
//! JAMAIS mutée, les tests restent parallélisables.
//!
//! Stations du référentiel seed utilisées : 1001..=1004 (FR), 2001, 3001, 4001.

use serde_json::{json, Value};

mod common;
use common::{access_token, create_test_org, pg_pool, spawn_app};

/// POST /api/tracked-locations → (status, corps JSON éventuel).
async fn create_location(base: &str, token: &str, body: Value) -> (u16, Option<Value>) {
    let res = reqwest::Client::new()
        .post(format!("{base}/api/tracked-locations"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .expect("requête create");
    (res.status().as_u16(), res.json::<Value>().await.ok())
}

/// GET (path relatif) avec Bearer → (status, corps JSON éventuel).
async fn get_json(base: &str, token: &str, path: &str) -> (u16, Option<Value>) {
    let res = reqwest::Client::new()
        .get(format!("{base}{path}"))
        .bearer_auth(token)
        .send()
        .await
        .expect("requête GET");
    (res.status().as_u16(), res.json::<Value>().await.ok())
}

/// Corps de création minimal valide (1 station seed).
fn minimal_body(name: &str) -> Value {
    json!({ "name": name, "openaq_location_ids": [1001] })
}

// ─────────────────────────────────────────────────────────────────────────────
// Création
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn create_returns_201_with_stations_and_rules() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;

    let (status, body) = create_location(
        &base,
        &token,
        json!({
            "name": "Écoles du centre",
            "description": "Groupe scolaire + maternelle",
            "openaq_location_ids": [1002, 1001],
            "rules": [
                { "parameter": "pm25", "comparator": ">", "threshold_value": 15.0 },
                { "parameter": "no2", "comparator": ">=", "threshold_value": 90.0,
                  "severity": "critical", "name": "NO2 réglementaire" }
            ]
        }),
    )
    .await;
    let body = body.expect("corps 201");

    assert_eq!(status, 201, "création : {body:?}");
    assert_eq!(body["org_id"].as_i64(), Some(org.org_id));
    assert_eq!(body["name"], "Écoles du centre");
    assert_eq!(body["station_count"].as_i64(), Some(2));
    assert_eq!(body["active_rule_count"].as_i64(), Some(2));
    assert_eq!(body["is_active"], true);

    // La PREMIÈRE station du tableau est primaire (sémantique P1).
    let stations = body["stations"].as_array().expect("stations");
    assert_eq!(stations.len(), 2);
    assert_eq!(stations[0]["openaq_location_id"].as_i64(), Some(1002));
    assert_eq!(stations[0]["is_primary"], true);
    assert_eq!(stations[1]["is_primary"], false);
}

#[tokio::test]
async fn create_duplicate_name_in_same_org_is_409() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;

    let (s1, _) = create_location(&base, &token, minimal_body("Site unique")).await;
    assert_eq!(s1, 201);
    let (s2, body) = create_location(&base, &token, minimal_body("Site unique")).await;
    assert_eq!(s2, 409, "doublon de nom dans l'org : {body:?}");
    assert_eq!(body.expect("corps 409")["error"], "conflict");
}

#[tokio::test]
async fn same_name_in_another_org_is_allowed() {
    // L'unicité du nom est PAR ORG (uq_tracked_location_org_name) — pas globale.
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;

    let token_a = access_token(&base, &org_a.admin_email).await;
    let token_b = access_token(&base, &org_b.admin_email).await;
    let (s1, _) = create_location(&base, &token_a, minimal_body("Mairie")).await;
    let (s2, _) = create_location(&base, &token_b, minimal_body("Mairie")).await;
    assert_eq!((s1, s2), (201, 201));
}

#[tokio::test]
async fn create_with_unknown_station_is_422() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;

    let (status, body) = create_location(
        &base,
        &token,
        json!({ "name": "Station fantôme", "openaq_location_ids": [99999999] }),
    )
    .await;
    assert_eq!(status, 422, "station hors référentiel → QRT_P1 : {body:?}");
    assert_eq!(body.expect("corps 422")["error"], "unprocessable_entity");
}

#[tokio::test]
async fn create_with_duplicate_stations_is_422() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;

    let (status, _) = create_location(
        &base,
        &token,
        json!({ "name": "Doublon", "openaq_location_ids": [1001, 1001] }),
    )
    .await;
    assert_eq!(status, 422, "stations en double → QRT_P1");
}

#[tokio::test]
async fn create_without_station_is_400() {
    // Tableau vide : arrêté AU BOUNDARY (validator 1..=50) — 400, pas 422 :
    // la requête n'atteint jamais P1.
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;

    let (status, _) = create_location(
        &base,
        &token,
        json!({ "name": "Sans station", "openaq_location_ids": [] }),
    )
    .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn create_with_invalid_rule_is_400() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;

    // Comparateur hors allowlist (`<` n'existe pas — seuls les dépassements).
    let (status, _) = create_location(
        &base,
        &token,
        json!({
            "name": "Règle invalide",
            "openaq_location_ids": [1001],
            "rules": [{ "parameter": "pm25", "comparator": "<", "threshold_value": 10.0 }]
        }),
    )
    .await;
    assert_eq!(status, 400);
}

// ─────────────────────────────────────────────────────────────────────────────
// RBAC : le rôle lecteur ne mute pas
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn reader_role_cannot_mutate_but_can_read() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let admin = access_token(&base, &org.admin_email).await;
    let reader = access_token(&base, &org.reader_email).await;

    let (_, created) = create_location(&base, &admin, minimal_body("Lecture seule")).await;
    let id = created.expect("corps")["id"].as_i64().expect("id");

    // Lecture : OK (le lecteur voit les lieux de SON org).
    let (status, _) = get_json(&base, &reader, &format!("/api/tracked-locations/{id}")).await;
    assert_eq!(status, 200);

    // Mutations : refusées par l'extracteur CanWrite, code stable `read_only_role`.
    let (status, body) = create_location(&base, &reader, minimal_body("Tentative")).await;
    assert_eq!(status, 403);
    assert_eq!(body.expect("corps 403")["error"], "read_only_role");

    let client = reqwest::Client::new();
    let patch = client
        .patch(format!("{base}/api/tracked-locations/{id}"))
        .bearer_auth(&reader)
        .json(&json!({ "name": "Renommé" }))
        .send()
        .await
        .expect("requête PATCH");
    assert_eq!(patch.status().as_u16(), 403);

    let delete = client
        .delete(format!("{base}/api/tracked-locations/{id}"))
        .bearer_auth(&reader)
        .send()
        .await
        .expect("requête DELETE");
    assert_eq!(delete.status().as_u16(), 403);
}

// ─────────────────────────────────────────────────────────────────────────────
// LE rempart : isolation multi-tenant (404 anti-énumération)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cross_tenant_location_is_invisible() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;

    let token_a = access_token(&base, &org_a.admin_email).await;
    let token_b = access_token(&base, &org_b.admin_email).await;

    let (_, created) = create_location(&base, &token_a, minimal_body("Secret A")).await;
    let id = created.expect("corps")["id"].as_i64().expect("id");

    // GET : 404 — pas 403 — un id étranger est indistinguable d'un id inexistant.
    let (status, body) = get_json(&base, &token_b, &format!("/api/tracked-locations/{id}")).await;
    assert_eq!(status, 404, "lecture cross-tenant : {body:?}");

    // PATCH et DELETE : même invisibilité (et la ressource n'est PAS modifiée).
    let client = reqwest::Client::new();
    let patch = client
        .patch(format!("{base}/api/tracked-locations/{id}"))
        .bearer_auth(&token_b)
        .json(&json!({ "name": "Piraté" }))
        .send()
        .await
        .expect("requête PATCH");
    assert_eq!(patch.status().as_u16(), 404);

    let delete = client
        .delete(format!("{base}/api/tracked-locations/{id}"))
        .bearer_auth(&token_b)
        .send()
        .await
        .expect("requête DELETE");
    assert_eq!(delete.status().as_u16(), 404);

    // Le listing de B ne contient jamais le lieu de A.
    let (_, page) = get_json(&base, &token_b, "/api/tracked-locations?page_size=100").await;
    let data = page.expect("page")["data"]
        .as_array()
        .expect("data")
        .clone();
    assert!(
        data.iter().all(|l| l["id"].as_i64() != Some(id)),
        "le lieu de l'org A ne doit pas apparaître chez B"
    );

    // Et côté A, rien n'a bougé.
    let (status, body) = get_json(&base, &token_a, &format!("/api/tracked-locations/{id}")).await;
    assert_eq!(status, 200);
    assert_eq!(body.expect("corps")["name"], "Secret A");
}

// ─────────────────────────────────────────────────────────────────────────────
// Listing : pagination, tri, recherche, filtre
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_paginates_sorts_and_filters() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;

    for name in ["Alpha", "Bravo", "Charlie"] {
        let (s, _) = create_location(&base, &token, minimal_body(name)).await;
        assert_eq!(s, 201);
    }

    // Page 1 triée par nom : déterministe, total = 3.
    let (status, page) = get_json(
        &base,
        &token,
        "/api/tracked-locations?page_size=2&sort=name",
    )
    .await;
    assert_eq!(status, 200);
    let page = page.expect("page 1");
    assert_eq!(page["total"].as_i64(), Some(3));
    assert_eq!(page["count"].as_i64(), Some(2));
    let names: Vec<&str> = page["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|l| l["name"].as_str().expect("name"))
        .collect();
    assert_eq!(names, ["Alpha", "Bravo"]);

    // Page 2.
    let (_, page2) = get_json(
        &base,
        &token,
        "/api/tracked-locations?page_size=2&sort=name&page=2",
    )
    .await;
    let page2 = page2.expect("page 2");
    assert_eq!(page2["count"].as_i64(), Some(1));
    assert_eq!(page2["data"][0]["name"], "Charlie");

    // Tri descendant (préfixe `-`).
    let (_, desc) = get_json(&base, &token, "/api/tracked-locations?sort=-name").await;
    assert_eq!(desc.expect("page desc")["data"][0]["name"], "Charlie");

    // Recherche sous-chaîne insensible à la casse.
    let (_, found) = get_json(&base, &token, "/api/tracked-locations?q=RAV").await;
    let found = found.expect("page q");
    assert_eq!(found["total"].as_i64(), Some(1));
    assert_eq!(found["data"][0]["name"], "Bravo");

    // Filtre is_active après mise en pause d'un lieu.
    let alpha_id = {
        let (_, p) = get_json(&base, &token, "/api/tracked-locations?q=Alpha").await;
        p.expect("alpha")["data"][0]["id"].as_i64().expect("id")
    };
    let paused = reqwest::Client::new()
        .patch(format!("{base}/api/tracked-locations/{alpha_id}"))
        .bearer_auth(&token)
        .json(&json!({ "is_active": false }))
        .send()
        .await
        .expect("requête PATCH");
    assert_eq!(paused.status().as_u16(), 200);

    let (_, actives) = get_json(&base, &token, "/api/tracked-locations?is_active=true").await;
    assert_eq!(actives.expect("page actifs")["total"].as_i64(), Some(2));
    let (_, inactives) = get_json(&base, &token, "/api/tracked-locations?is_active=false").await;
    assert_eq!(inactives.expect("page inactifs")["total"].as_i64(), Some(1));
}

#[tokio::test]
async fn list_rejects_unknown_sort_and_out_of_bounds_page() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;

    // Tri hors allowlist → 400 (la chaîne client n'atteint jamais le SQL).
    let (status, body) = get_json(&base, &token, "/api/tracked-locations?sort=password").await;
    assert_eq!(status, 400, "{body:?}");

    // Bornes de pagination (mêmes conventions que /api/measurements).
    let (status, _) = get_json(&base, &token, "/api/tracked-locations?page=0").await;
    assert_eq!(status, 400);
    let (status, _) = get_json(&base, &token, "/api/tracked-locations?page_size=101").await;
    assert_eq!(status, 400);
}

// ─────────────────────────────────────────────────────────────────────────────
// PATCH / DELETE
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn patch_updates_renames_and_clears_description() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;
    let client = reqwest::Client::new();

    let (_, created) = create_location(
        &base,
        &token,
        json!({ "name": "Avant", "description": "à effacer", "openaq_location_ids": [1001] }),
    )
    .await;
    let id = created.expect("corps")["id"].as_i64().expect("id");

    // Renommage + effacement de description (`""` → NULL).
    let res = client
        .patch(format!("{base}/api/tracked-locations/{id}"))
        .bearer_auth(&token)
        .json(&json!({ "name": "Après", "description": "" }))
        .send()
        .await
        .expect("requête PATCH");
    assert_eq!(res.status().as_u16(), 200);
    let body: Value = res.json().await.expect("corps PATCH");
    assert_eq!(body["name"], "Après");
    assert!(
        body["description"].is_null(),
        "description effacée : {body:?}"
    );

    // PATCH vide : 400 explicite (aucun champ).
    let res = client
        .patch(format!("{base}/api/tracked-locations/{id}"))
        .bearer_auth(&token)
        .json(&json!({}))
        .send()
        .await
        .expect("requête PATCH vide");
    assert_eq!(res.status().as_u16(), 400);

    // Renommer vers le nom d'un AUTRE lieu de l'org : 409.
    let (s, _) = create_location(&base, &token, minimal_body("Occupé")).await;
    assert_eq!(s, 201);
    let res = client
        .patch(format!("{base}/api/tracked-locations/{id}"))
        .bearer_auth(&token)
        .json(&json!({ "name": "Occupé" }))
        .send()
        .await
        .expect("requête PATCH doublon");
    assert_eq!(res.status().as_u16(), 409);
}

#[tokio::test]
async fn delete_returns_204_then_404() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;
    let client = reqwest::Client::new();

    // Avec une règle adossée : la cascade doit passer (T5 audite la suppression).
    let (_, created) = create_location(
        &base,
        &token,
        json!({
            "name": "Éphémère",
            "openaq_location_ids": [1001],
            "rules": [{ "parameter": "o3", "comparator": ">", "threshold_value": 120.0 }]
        }),
    )
    .await;
    let id = created.expect("corps")["id"].as_i64().expect("id");

    let res = client
        .delete(format!("{base}/api/tracked-locations/{id}"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("requête DELETE");
    assert_eq!(res.status().as_u16(), 204);

    let (status, _) = get_json(&base, &token, &format!("/api/tracked-locations/{id}")).await;
    assert_eq!(status, 404);

    // Suppression rejouée : 404 (idempotence côté observabilité, pas de 500).
    let res = client
        .delete(format!("{base}/api/tracked-locations/{id}"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("requête DELETE rejouée");
    assert_eq!(res.status().as_u16(), 404);
}

// ─────────────────────────────────────────────────────────────────────────────
// Authentification requise
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn endpoints_require_bearer() {
    let base = spawn_app().await;
    let client = reqwest::Client::new();

    for (method, path) in [
        ("GET", "/api/tracked-locations"),
        ("POST", "/api/tracked-locations"),
        ("GET", "/api/tracked-locations/1"),
        ("PATCH", "/api/tracked-locations/1"),
        ("DELETE", "/api/tracked-locations/1"),
    ] {
        let req = match method {
            "GET" => client.get(format!("{base}{path}")),
            "POST" => client.post(format!("{base}{path}")).json(&json!({})),
            "PATCH" => client.patch(format!("{base}{path}")).json(&json!({})),
            _ => client.delete(format!("{base}{path}")),
        };
        let status = req.send().await.expect("requête sans Bearer").status();
        assert_eq!(status.as_u16(), 401, "{method} {path} sans Bearer");
    }
}
