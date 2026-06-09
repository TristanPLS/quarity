//! Tests e2e du CRUD `/api/alert-rules` (B6).
//!
//! Prérequis : identiques à `tests/e2e.rs` (3 bases réelles + schéma + seed).
//! Chaque test crée son org JETABLE (cf. `tests/common/mod.rs`) : la seed n'est
//! JAMAIS mutée, les tests restent parallélisables.
//!
//! Les lieux supports sont posés via `POST /api/tracked-locations` (déjà câblé) ;
//! station seed utilisée : 1001. Le test `audit_t5_…` prouve le câblage
//! GUC `quarity.actor_user_id` → trigger T5 de bout en bout (API → `audit_log`).

use serde_json::{json, Value};

mod common;
use common::{access_token, create_test_org, pg_pool, spawn_app};

/// POST /api/tracked-locations minimal (1 station seed) → id du lieu créé.
async fn create_location(base: &str, token: &str, name: &str) -> i64 {
    let res = reqwest::Client::new()
        .post(format!("{base}/api/tracked-locations"))
        .bearer_auth(token)
        .json(&json!({ "name": name, "openaq_location_ids": [1001] }))
        .send()
        .await
        .expect("requête create location");
    assert_eq!(res.status().as_u16(), 201, "lieu support « {name} »");
    res.json::<Value>().await.expect("corps lieu")["id"]
        .as_i64()
        .expect("id lieu")
}

/// POST /api/alert-rules → (status, corps JSON éventuel).
async fn create_rule(base: &str, token: &str, body: Value) -> (u16, Option<Value>) {
    let res = reqwest::Client::new()
        .post(format!("{base}/api/alert-rules"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .expect("requête create rule");
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

/// PATCH /api/alert-rules/{id} → (status, corps JSON éventuel).
async fn patch_rule(base: &str, token: &str, id: i64, body: Value) -> (u16, Option<Value>) {
    let res = reqwest::Client::new()
        .patch(format!("{base}/api/alert-rules/{id}"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .expect("requête PATCH");
    (res.status().as_u16(), res.json::<Value>().await.ok())
}

/// DELETE /api/alert-rules/{id} → status.
async fn delete_rule(base: &str, token: &str, id: i64) -> u16 {
    reqwest::Client::new()
        .delete(format!("{base}/api/alert-rules/{id}"))
        .bearer_auth(token)
        .send()
        .await
        .expect("requête DELETE")
        .status()
        .as_u16()
}

/// Corps de création minimal valide (comparateur `>`).
fn rule_body(location_id: i64, parameter: &str, threshold: f64) -> Value {
    json!({
        "tracked_location_id": location_id,
        "parameter": parameter,
        "comparator": ">",
        "threshold_value": threshold
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Création
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn create_returns_201_with_defaults() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;
    let loc = create_location(&base, &token, "Écoles du centre").await;

    // Corps complet : tous les champs restitués.
    let (status, body) = create_rule(
        &base,
        &token,
        json!({
            "tracked_location_id": loc,
            "parameter": "no2",
            "comparator": ">=",
            "threshold_value": 90.0,
            "severity": "critical",
            "name": "NO2 réglementaire"
        }),
    )
    .await;
    let body = body.expect("corps 201");
    assert_eq!(status, 201, "création : {body:?}");
    assert_eq!(body["org_id"].as_i64(), Some(org.org_id));
    assert_eq!(body["tracked_location_id"].as_i64(), Some(loc));
    assert_eq!(body["tracked_location_name"], "Écoles du centre");
    assert_eq!(body["parameter"], "no2");
    assert_eq!(body["comparator"], ">=");
    assert_eq!(body["threshold_value"].as_f64(), Some(90.0));
    assert_eq!(body["severity"], "critical");
    assert_eq!(body["status"], "active");
    assert_eq!(body["name"], "NO2 réglementaire");

    // Corps minimal : défauts du schéma — severity `warning`, status `active`,
    // name absent ⇒ null.
    let (status, body) = create_rule(&base, &token, rule_body(loc, "pm25", 15.0)).await;
    let body = body.expect("corps 201 minimal");
    assert_eq!(status, 201, "création minimale : {body:?}");
    assert_eq!(body["severity"], "warning");
    assert_eq!(body["status"], "active");
    assert!(body["name"].is_null(), "name omis ⇒ null : {body:?}");
}

#[tokio::test]
async fn create_duplicate_is_409_but_other_location_is_201() {
    // L'unicité est (lieu, polluant, comparateur, seuil) — la MÊME règle sur un
    // AUTRE lieu est légitime.
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;
    let loc_a = create_location(&base, &token, "Lieu A").await;
    let loc_b = create_location(&base, &token, "Lieu B").await;

    let (s1, _) = create_rule(&base, &token, rule_body(loc_a, "pm25", 15.0)).await;
    assert_eq!(s1, 201);

    let (s2, body) = create_rule(&base, &token, rule_body(loc_a, "pm25", 15.0)).await;
    assert_eq!(s2, 409, "doublon exact : {body:?}");
    assert_eq!(body.expect("corps 409")["error"], "conflict");

    let (s3, body) = create_rule(&base, &token, rule_body(loc_b, "pm25", 15.0)).await;
    assert_eq!(s3, 201, "même règle, autre lieu : {body:?}");
}

#[tokio::test]
async fn create_on_foreign_location_is_404() {
    // Un lieu d'une AUTRE org est INVISIBLE : 404 — pas 403 (réservé aux refus de
    // rôle), pas 422 (qui confirmerait l'existence du lieu à un attaquant).
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;

    let token_a = access_token(&base, &org_a.admin_email).await;
    let token_b = access_token(&base, &org_b.admin_email).await;
    let loc_a = create_location(&base, &token_a, "Lieu de A").await;

    let (status, body) = create_rule(&base, &token_b, rule_body(loc_a, "pm25", 15.0)).await;
    assert_eq!(status, 404, "lieu étranger : {body:?}");
    assert_eq!(
        body.expect("corps 404")["error"],
        "tracked_location_not_found"
    );
}

#[tokio::test]
async fn create_with_invalid_comparator_or_parameter_is_400() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;
    let loc = create_location(&base, &token, "Validations").await;

    // `<` n'existe pas (seuls les dépassements — US-02) : arrêté au boundary.
    let (status, _) = create_rule(
        &base,
        &token,
        json!({
            "tracked_location_id": loc,
            "parameter": "pm25",
            "comparator": "<",
            "threshold_value": 10.0
        }),
    )
    .await;
    assert_eq!(status, 400, "comparator « < » doit être rejeté");

    // Polluant hors allowlist : même rempart.
    let (status, _) = create_rule(&base, &token, rule_body(loc, "xyz", 10.0)).await;
    assert_eq!(status, 400, "parameter hors allowlist doit être rejeté");
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
    let loc = create_location(&base, &admin, "Lecture seule").await;

    let (_, created) = create_rule(&base, &admin, rule_body(loc, "pm25", 15.0)).await;
    let id = created.expect("corps")["id"].as_i64().expect("id");

    // Lecture : OK (le lecteur voit les règles de SON org).
    let (status, _) = get_json(&base, &reader, &format!("/api/alert-rules/{id}")).await;
    assert_eq!(status, 200);
    let (status, _) = get_json(&base, &reader, "/api/alert-rules").await;
    assert_eq!(status, 200);

    // Mutations : refusées par l'extracteur CanWrite, code stable `read_only_role`.
    let (status, body) = create_rule(&base, &reader, rule_body(loc, "no2", 40.0)).await;
    assert_eq!(status, 403);
    assert_eq!(body.expect("corps 403")["error"], "read_only_role");

    let (status, _) = patch_rule(&base, &reader, id, json!({ "threshold_value": 20.0 })).await;
    assert_eq!(status, 403);

    assert_eq!(delete_rule(&base, &reader, id).await, 403);
}

// ─────────────────────────────────────────────────────────────────────────────
// LE rempart : isolation multi-tenant (404 anti-énumération)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cross_tenant_rule_is_invisible() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;

    let token_a = access_token(&base, &org_a.admin_email).await;
    let token_b = access_token(&base, &org_b.admin_email).await;
    let loc_a = create_location(&base, &token_a, "Site secret").await;

    let (_, created) = create_rule(&base, &token_a, rule_body(loc_a, "pm25", 15.0)).await;
    let id = created.expect("corps")["id"].as_i64().expect("id");

    // GET : 404 — pas 403 — un id étranger est indistinguable d'un id inexistant.
    let (status, body) = get_json(&base, &token_b, &format!("/api/alert-rules/{id}")).await;
    assert_eq!(status, 404, "lecture cross-tenant : {body:?}");

    // PATCH et DELETE : même invisibilité (et la règle n'est PAS modifiée).
    let (status, _) = patch_rule(&base, &token_b, id, json!({ "threshold_value": 1.0 })).await;
    assert_eq!(status, 404);
    assert_eq!(delete_rule(&base, &token_b, id).await, 404);

    // Le listing de B ne contient jamais la règle de A.
    let (_, page) = get_json(&base, &token_b, "/api/alert-rules?page_size=100").await;
    let data = page.expect("page")["data"]
        .as_array()
        .expect("data")
        .clone();
    assert!(
        data.iter().all(|r| r["id"].as_i64() != Some(id)),
        "la règle de l'org A ne doit pas apparaître chez B"
    );

    // Et côté A, rien n'a bougé.
    let (status, body) = get_json(&base, &token_a, &format!("/api/alert-rules/{id}")).await;
    assert_eq!(status, 200);
    assert_eq!(body.expect("corps")["threshold_value"].as_f64(), Some(15.0));
}

// ─────────────────────────────────────────────────────────────────────────────
// Listing : pagination, tri, recherche, filtres
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_paginates_and_sorts() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;
    let loc = create_location(&base, &token, "Tri & pages").await;

    // 3 seuils distincts sur le même (lieu, polluant, comparateur) : licite,
    // l'unicité porte sur le quadruplet complet.
    for threshold in [10.0, 20.0, 30.0] {
        let (s, _) = create_rule(&base, &token, rule_body(loc, "pm25", threshold)).await;
        assert_eq!(s, 201);
    }

    // Page 1 triée par seuil croissant : déterministe, total = 3.
    let (status, page) = get_json(
        &base,
        &token,
        "/api/alert-rules?page_size=2&sort=threshold_value",
    )
    .await;
    assert_eq!(status, 200);
    let page = page.expect("page 1");
    assert_eq!(page["total"].as_i64(), Some(3));
    assert_eq!(page["count"].as_i64(), Some(2));
    let thresholds: Vec<f64> = page["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|r| r["threshold_value"].as_f64().expect("threshold"))
        .collect();
    assert_eq!(thresholds, [10.0, 20.0]);

    // Page 2.
    let (_, page2) = get_json(
        &base,
        &token,
        "/api/alert-rules?page_size=2&sort=threshold_value&page=2",
    )
    .await;
    let page2 = page2.expect("page 2");
    assert_eq!(page2["count"].as_i64(), Some(1));
    assert_eq!(page2["data"][0]["threshold_value"].as_f64(), Some(30.0));

    // Tri descendant (préfixe `-`).
    let (_, desc) = get_json(&base, &token, "/api/alert-rules?sort=-threshold_value").await;
    assert_eq!(
        desc.expect("page desc")["data"][0]["threshold_value"].as_f64(),
        Some(30.0)
    );
}

#[tokio::test]
async fn list_searches_rule_name_and_location_name() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;

    // Règle SANS nom sur un lieu au nom parlant : trouvable via le lieu seulement
    // (name NULL ⇒ ILIKE NULL ≅ false, comportement voulu).
    let loc_a = create_location(&base, &token, "Groupe scolaire Pasteur").await;
    let (s, created_a) = create_rule(&base, &token, rule_body(loc_a, "pm25", 10.0)).await;
    assert_eq!(s, 201);
    let id_a = created_a.expect("corps")["id"].as_i64().expect("id");

    // Règle nommée sur un lieu neutre : trouvable via son propre nom.
    let loc_b = create_location(&base, &token, "Mairie centrale").await;
    let (s, created_b) = create_rule(
        &base,
        &token,
        json!({
            "tracked_location_id": loc_b,
            "parameter": "no2",
            "comparator": ">",
            "threshold_value": 25.0,
            "name": "Seuil OMS NO2"
        }),
    )
    .await;
    assert_eq!(s, 201);
    let id_b = created_b.expect("corps")["id"].as_i64().expect("id");

    // Recherche par nom de LIEU (sous-chaîne insensible à la casse).
    let (_, found) = get_json(&base, &token, "/api/alert-rules?q=pasteur").await;
    let found = found.expect("page q lieu");
    assert_eq!(found["total"].as_i64(), Some(1));
    assert_eq!(found["data"][0]["id"].as_i64(), Some(id_a));

    // Recherche par nom de RÈGLE.
    let (_, found) = get_json(&base, &token, "/api/alert-rules?q=oms").await;
    let found = found.expect("page q règle");
    assert_eq!(found["total"].as_i64(), Some(1));
    assert_eq!(found["data"][0]["id"].as_i64(), Some(id_b));

    // Aucune correspondance.
    let (_, none) = get_json(&base, &token, "/api/alert-rules?q=zzz-introuvable").await;
    assert_eq!(none.expect("page vide")["total"].as_i64(), Some(0));
}

#[tokio::test]
async fn list_filters_by_location_parameter_and_severity() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;
    let loc_a = create_location(&base, &token, "Filtre A").await;
    let loc_b = create_location(&base, &token, "Filtre B").await;

    let (s, _) = create_rule(&base, &token, rule_body(loc_a, "pm25", 10.0)).await;
    assert_eq!(s, 201);
    let (s, _) = create_rule(
        &base,
        &token,
        json!({
            "tracked_location_id": loc_a,
            "parameter": "no2",
            "comparator": ">",
            "threshold_value": 20.0,
            "severity": "critical"
        }),
    )
    .await;
    assert_eq!(s, 201);
    let (s, _) = create_rule(
        &base,
        &token,
        json!({
            "tracked_location_id": loc_b,
            "parameter": "pm10",
            "comparator": ">",
            "threshold_value": 30.0,
            "severity": "info"
        }),
    )
    .await;
    assert_eq!(s, 201);

    // Par lieu.
    let path = format!("/api/alert-rules?tracked_location_id={loc_a}");
    let (_, by_loc) = get_json(&base, &token, &path).await;
    assert_eq!(by_loc.expect("page lieu")["total"].as_i64(), Some(2));

    // Par polluant.
    let (_, by_param) = get_json(&base, &token, "/api/alert-rules?parameter=no2").await;
    let by_param = by_param.expect("page polluant");
    assert_eq!(by_param["total"].as_i64(), Some(1));
    assert_eq!(by_param["data"][0]["parameter"], "no2");

    // Par sévérité.
    let (_, by_sev) = get_json(&base, &token, "/api/alert-rules?severity=info").await;
    let by_sev = by_sev.expect("page sévérité");
    assert_eq!(by_sev["total"].as_i64(), Some(1));
    assert_eq!(by_sev["data"][0]["severity"], "info");

    // Filtres combinés : lieu + sévérité.
    let path = format!("/api/alert-rules?tracked_location_id={loc_a}&severity=critical");
    let (_, combo) = get_json(&base, &token, &path).await;
    assert_eq!(combo.expect("page combinée")["total"].as_i64(), Some(1));
}

#[tokio::test]
async fn list_rejects_unknown_sort_filter_and_out_of_bounds_page() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;

    // Tri hors allowlist → 400 (la chaîne client n'atteint jamais le SQL).
    let (status, body) = get_json(&base, &token, "/api/alert-rules?sort=password").await;
    assert_eq!(status, 400, "{body:?}");

    // Filtres hors allowlist → 400 (validation déclarative au boundary).
    let (status, _) = get_json(&base, &token, "/api/alert-rules?status=bogus").await;
    assert_eq!(status, 400);
    let (status, _) = get_json(&base, &token, "/api/alert-rules?parameter=xyz").await;
    assert_eq!(status, 400);
    let (status, _) = get_json(&base, &token, "/api/alert-rules?severity=fatal").await;
    assert_eq!(status, 400);

    // Bornes de pagination (mêmes conventions que les autres listings B6).
    let (status, _) = get_json(&base, &token, "/api/alert-rules?page=0").await;
    assert_eq!(status, 400);
    let (status, _) = get_json(&base, &token, "/api/alert-rules?page_size=101").await;
    assert_eq!(status, 400);
}

// ─────────────────────────────────────────────────────────────────────────────
// PATCH / DELETE
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn patch_updates_clears_name_and_rejects_empty() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;
    let loc = create_location(&base, &token, "PATCH").await;

    let (_, created) = create_rule(
        &base,
        &token,
        json!({
            "tracked_location_id": loc,
            "parameter": "pm25",
            "comparator": ">",
            "threshold_value": 10.0,
            "name": "Avant"
        }),
    )
    .await;
    let id = created.expect("corps")["id"].as_i64().expect("id");

    // Mutation multi-champs + effacement du nom (`""` → NULL).
    let (status, body) = patch_rule(
        &base,
        &token,
        id,
        json!({
            "comparator": ">=",
            "threshold_value": 42.0,
            "severity": "critical",
            "name": ""
        }),
    )
    .await;
    let body = body.expect("corps PATCH");
    assert_eq!(status, 200, "PATCH : {body:?}");
    assert_eq!(body["comparator"], ">=");
    assert_eq!(body["threshold_value"].as_f64(), Some(42.0));
    assert_eq!(body["severity"], "critical");
    assert!(body["name"].is_null(), "nom effacé : {body:?}");
    // Les champs non fournis sont inchangés.
    assert_eq!(body["status"], "active");
    assert_eq!(body["parameter"], "pm25");

    // PATCH vide : 400 explicite (aucun champ).
    let (status, _) = patch_rule(&base, &token, id, json!({})).await;
    assert_eq!(status, 400);

    // Les champs IMMUABLES ne sont même pas désérialisés (serde les ignore) :
    // un corps qui ne contient qu'eux équivaut à un PATCH vide → 400.
    let (status, _) = patch_rule(
        &base,
        &token,
        id,
        json!({ "tracked_location_id": 999, "parameter": "no2" }),
    )
    .await;
    assert_eq!(status, 400, "immuables seuls ⇒ aucun champ reconnu");
}

#[tokio::test]
async fn patch_duplicate_is_409() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;
    let loc = create_location(&base, &token, "Doublon PATCH").await;

    let (s, _) = create_rule(&base, &token, rule_body(loc, "pm25", 10.0)).await;
    assert_eq!(s, 201);
    let (s, created) = create_rule(&base, &token, rule_body(loc, "pm25", 20.0)).await;
    assert_eq!(s, 201);
    let id2 = created.expect("corps")["id"].as_i64().expect("id");

    // Ramener le seuil de la 2e règle sur celui de la 1re ⇒ quadruplet identique.
    let (status, body) = patch_rule(&base, &token, id2, json!({ "threshold_value": 10.0 })).await;
    assert_eq!(status, 409, "doublon par PATCH : {body:?}");
    assert_eq!(body.expect("corps 409")["error"], "conflict");
}

#[tokio::test]
async fn patch_status_toggle_then_filter() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;
    let loc = create_location(&base, &token, "Bascule").await;

    let (_, created) = create_rule(&base, &token, rule_body(loc, "pm25", 15.0)).await;
    let id = created.expect("corps")["id"].as_i64().expect("id");

    // active → inactive (T5 tracera `deactivate`).
    let (status, body) = patch_rule(&base, &token, id, json!({ "status": "inactive" })).await;
    assert_eq!(status, 200);
    assert_eq!(body.expect("corps PATCH")["status"], "inactive");

    // Le filtre status=inactive la renvoie…
    let (_, inactives) = get_json(
        &base,
        &token,
        "/api/alert-rules?status=inactive&page_size=100",
    )
    .await;
    let inactives = inactives.expect("page inactives")["data"]
        .as_array()
        .expect("data")
        .clone();
    assert!(
        inactives.iter().any(|r| r["id"].as_i64() == Some(id)),
        "la règle désactivée doit apparaître dans status=inactive"
    );

    // … et status=active ne la renvoie plus.
    let (_, actives) = get_json(
        &base,
        &token,
        "/api/alert-rules?status=active&page_size=100",
    )
    .await;
    let actives = actives.expect("page actives")["data"]
        .as_array()
        .expect("data")
        .clone();
    assert!(
        actives.iter().all(|r| r["id"].as_i64() != Some(id)),
        "la règle désactivée ne doit plus apparaître dans status=active"
    );
}

#[tokio::test]
async fn delete_returns_204_then_404() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;
    let loc = create_location(&base, &token, "Éphémère").await;

    let (_, created) = create_rule(&base, &token, rule_body(loc, "o3", 120.0)).await;
    let id = created.expect("corps")["id"].as_i64().expect("id");

    assert_eq!(delete_rule(&base, &token, id).await, 204);

    let (status, _) = get_json(&base, &token, &format!("/api/alert-rules/{id}")).await;
    assert_eq!(status, 404);

    // Suppression rejouée : 404 (idempotence côté observabilité, pas de 500).
    assert_eq!(delete_rule(&base, &token, id).await, 404);
}

// ─────────────────────────────────────────────────────────────────────────────
// Audit T5 de bout en bout : API → GUC quarity.actor_user_id → audit_log
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn audit_t5_traces_create_update_delete_with_actor() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;

    // L'id de l'acteur attendu s'obtient par l'API elle-même (GET /api/auth/me).
    let (status, me) = get_json(&base, &token, "/api/auth/me").await;
    assert_eq!(status, 200);
    let admin_id = me.expect("corps me")["user_id"].as_i64().expect("user_id");

    let loc = create_location(&base, &token, "Lieu audité").await;
    let (status, created) = create_rule(&base, &token, rule_body(loc, "pm25", 10.0)).await;
    assert_eq!(status, 201);
    let id = created.expect("corps")["id"].as_i64().expect("id");

    // UPDATE « pur » (changement de seuil, pas de bascule de status) → `update`.
    let (status, _) = patch_rule(&base, &token, id, json!({ "threshold_value": 25.0 })).await;
    assert_eq!(status, 200);

    assert_eq!(delete_rule(&base, &token, id).await, 204);

    // Vérification DIRECTE en base : c'est LE test qui prouve le câblage
    // set_audit_actor (GUC transaction-local) → trigger T5. Sans lui, l'API
    // fonctionnerait à l'identique mais l'audit serait anonyme (actor NULL).
    let rows: Vec<(String, Option<i64>)> = sqlx::query_as(
        "SELECT action, actor_user_id FROM audit_log
         WHERE entity_type = 'alert_rule' AND entity_id = $1
         ORDER BY id",
    )
    .bind(id)
    .fetch_all(&pool)
    .await
    .expect("lecture audit_log");

    let actions: Vec<&str> = rows.iter().map(|(a, _)| a.as_str()).collect();
    assert_eq!(
        actions,
        ["create", "update", "delete"],
        "actions T5 : {rows:?}"
    );
    for (action, actor) in &rows {
        assert_eq!(
            *actor,
            Some(admin_id),
            "acteur de « {action} » = admin appelant"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Fidélité numérique du seuil (NUMERIC ↔ f64)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn decimal_threshold_value_roundtrips() {
    // NUMERIC(12,4) en base, casté ::float8 au SELECT : 12.5 doit ressortir 12.5
    // en JSON (ni 12, ni 12.5000 en chaîne).
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;
    let loc = create_location(&base, &token, "Décimal").await;

    let (status, created) = create_rule(&base, &token, rule_body(loc, "pm25", 12.5)).await;
    assert_eq!(status, 201);
    let created = created.expect("corps 201");
    assert_eq!(created["threshold_value"].as_f64(), Some(12.5));
    let id = created["id"].as_i64().expect("id");

    let (status, body) = get_json(&base, &token, &format!("/api/alert-rules/{id}")).await;
    assert_eq!(status, 200);
    assert_eq!(
        body.expect("corps GET")["threshold_value"].as_f64(),
        Some(12.5)
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Authentification requise
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn endpoints_require_bearer() {
    let base = spawn_app().await;
    let client = reqwest::Client::new();

    for (method, path) in [
        ("GET", "/api/alert-rules"),
        ("POST", "/api/alert-rules"),
        ("GET", "/api/alert-rules/1"),
        ("PATCH", "/api/alert-rules/1"),
        ("DELETE", "/api/alert-rules/1"),
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
