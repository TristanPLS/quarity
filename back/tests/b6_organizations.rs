//! Tests e2e du CRUD `/api/organizations` (B6).
//!
//! Prérequis : identiques à `tests/e2e.rs` (3 bases réelles + schéma + seed).
//! Chaque test crée ses orgs JETABLES (cf. `tests/common/mod.rs`) : la seed n'est
//! JAMAIS mutée, les tests restent parallélisables.
//!
//! Spécificité de la ressource : le périmètre est « les orgs dont l'appelant est
//! MEMBRE », pas l'org du JWT — et l'unicité du slug est GLOBALE : chaque test
//! fabrique donc ses slugs uniques ([`unique_slug`]) pour ne jamais entrer en
//! collision avec un test parallèle ni avec la seed (`agglo-riviera`, …).

use serde_json::{json, Value};
use uuid::Uuid;

mod common;
use common::{access_token, create_test_org, login, pg_pool, spawn_app, DEMO_PASSWORD};

/// Slug unique (`org-<hex12>`) — l'unicité du slug est GLOBALE.
fn unique_slug() -> String {
    let suffix = Uuid::new_v4().simple().to_string();
    format!("org-{}", &suffix[..12])
}

/// POST /api/organizations → (status, corps JSON éventuel).
async fn create_org(base: &str, token: &str, body: Value) -> (u16, Option<Value>) {
    let res = reqwest::Client::new()
        .post(format!("{base}/api/organizations"))
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

// ─────────────────────────────────────────────────────────────────────────────
// Création
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn create_returns_201_with_creator_as_admin() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;

    let slug = unique_slug();
    let (status, body) = create_org(
        &base,
        &token,
        json!({ "name": "Mairie de Bormes", "slug": slug.clone(), "segment": "B2G" }),
    )
    .await;
    let body = body.expect("corps 201");

    assert_eq!(status, 201, "création : {body:?}");
    assert_eq!(body["name"], "Mairie de Bormes");
    assert_eq!(body["slug"].as_str(), Some(slug.as_str()));
    assert_eq!(body["segment"], "B2G");
    // Le fondateur est admin de SA nouvelle org (membership atomique).
    assert_eq!(body["my_role"], "admin");
    assert_eq!(body["unread_alert_count"].as_i64(), Some(0));
}

#[tokio::test]
async fn reader_of_another_org_can_found_and_manage_his_own_org() {
    // Choix documenté du module : POST = AuthUser simple, pas CanWrite — fonder
    // un NOUVEAU tenant n'est pas une mutation du tenant courant. Un lecteur
    // (can_write=false dans SON org) peut donc créer la sienne.
    let base = spawn_app().await;
    let org_a = create_test_org(&pg_pool().await).await;
    let reader = access_token(&base, &org_a.reader_email).await;
    let client = reqwest::Client::new();

    let (status, created) = create_org(
        &base,
        &reader,
        json!({ "name": "Asso du lecteur", "slug": unique_slug() }),
    )
    .await;
    let created = created.expect("corps 201");
    assert_eq!(
        status, 201,
        "POST par un lecteur d'une autre org : {created:?}"
    );
    assert_eq!(created["my_role"], "admin");
    let new_id = created["id"].as_i64().expect("id");

    // GET avec le MÊME jeton : le JWT pointe org_a, mais le périmètre est la
    // MEMBERSHIP — la nouvelle org est visible et my_role re-résolu en base.
    let (status, body) = get_json(&base, &reader, &format!("/api/organizations/{new_id}")).await;
    assert_eq!(status, 200, "GET de la nouvelle org : {body:?}");
    assert_eq!(body.expect("corps GET")["my_role"], "admin");

    // PATCH : passe aussi (rôle admin re-résolu, alors que les claims du jeton
    // disent can_write=false — preuve que la garde n'est PAS celle des claims).
    let res = client
        .patch(format!("{base}/api/organizations/{new_id}"))
        .bearer_auth(&reader)
        .json(&json!({ "name": "Asso renommée" }))
        .send()
        .await
        .expect("requête PATCH");
    assert_eq!(res.status().as_u16(), 200);
    let body: Value = res.json().await.expect("corps PATCH");
    assert_eq!(body["name"], "Asso renommée");
}

#[tokio::test]
async fn duplicate_slug_is_409() {
    // L'unicité du slug est GLOBALE (identifiant public) : un autre utilisateur,
    // d'une autre org, se heurte au même 409.
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;
    let token_a = access_token(&base, &org_a.admin_email).await;
    let token_b = access_token(&base, &org_b.admin_email).await;

    let slug = unique_slug();
    let (s1, _) = create_org(
        &base,
        &token_a,
        json!({ "name": "Première", "slug": slug.clone() }),
    )
    .await;
    assert_eq!(s1, 201);
    let (s2, body) = create_org(&base, &token_b, json!({ "name": "Seconde", "slug": slug })).await;
    assert_eq!(s2, 409, "slug pris : {body:?}");
    assert_eq!(body.expect("corps 409")["error"], "conflict");
}

#[tokio::test]
async fn create_with_invalid_payload_is_400() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;

    // Slug hors charte `^[a-z0-9-]{2,64}$` : majuscules, underscore, 1 caractère.
    for bad_slug in ["MAJUSCULE", "a_b", "a"] {
        let (status, body) = create_org(
            &base,
            &token,
            json!({ "name": "Slug KO", "slug": bad_slug }),
        )
        .await;
        assert_eq!(status, 400, "slug {bad_slug:?} : {body:?}");
    }

    // Segment hors allowlist (sensible à la casse, miroir du CHECK).
    let (status, _) = create_org(
        &base,
        &token,
        json!({ "name": "Segment KO", "slug": unique_slug(), "segment": "b2g" }),
    )
    .await;
    assert_eq!(status, 400);

    // Nom blanc (espaces uniquement) : arrêté au boundary.
    let (status, _) = create_org(
        &base,
        &token,
        json!({ "name": "   ", "slug": unique_slug() }),
    )
    .await;
    assert_eq!(status, 400);
}

// ─────────────────────────────────────────────────────────────────────────────
// RBAC : seul l'admin de l'org mute (gestionnaire ET lecteur ⇒ admin_required)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn manager_and_reader_cannot_mutate_org() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let client = reqwest::Client::new();

    // La garde est « admin de CETTE org » (re-résolue en base) : le gestionnaire
    // (pourtant can_write=true) comme le lecteur reçoivent le MÊME code stable
    // `admin_required` — pas `read_only_role`, qui est la garde d'écriture des
    // ressources scopées JWT.
    for (email, role) in [
        (&org.manager_email, "gestionnaire"),
        (&org.reader_email, "lecteur"),
    ] {
        let token = access_token(&base, email).await;

        // Lecture : OK — un membre voit son org, avec SON rôle dans my_role.
        let (status, body) =
            get_json(&base, &token, &format!("/api/organizations/{}", org.org_id)).await;
        assert_eq!(status, 200);
        assert_eq!(body.expect("corps GET")["my_role"], role);

        let res = client
            .patch(format!("{base}/api/organizations/{}", org.org_id))
            .bearer_auth(&token)
            .json(&json!({ "name": "Tentative" }))
            .send()
            .await
            .expect("requête PATCH");
        assert_eq!(res.status().as_u16(), 403, "PATCH par {role}");
        let body: Value = res.json().await.expect("corps 403 PATCH");
        assert_eq!(body["error"], "admin_required");

        let res = client
            .delete(format!("{base}/api/organizations/{}", org.org_id))
            .bearer_auth(&token)
            .send()
            .await
            .expect("requête DELETE");
        assert_eq!(res.status().as_u16(), 403, "DELETE par {role}");
        let body: Value = res.json().await.expect("corps 403 DELETE");
        assert_eq!(body["error"], "admin_required");
    }

    // Rien n'a bougé : l'org est intacte côté admin.
    let admin = access_token(&base, &org.admin_email).await;
    let (status, body) =
        get_json(&base, &admin, &format!("/api/organizations/{}", org.org_id)).await;
    assert_eq!(status, 200);
    assert_eq!(body.expect("corps")["my_role"], "admin");
}

// ─────────────────────────────────────────────────────────────────────────────
// LE rempart : une org où l'on n'est pas membre est invisible (404, jamais 403)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cross_tenant_org_is_invisible() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;

    let token_a = access_token(&base, &org_a.admin_email).await;
    let id_b = org_b.org_id;

    // GET : 404 — pas 403 — une org étrangère est indistinguable d'une org
    // inexistante (le 403 ne s'adresse qu'aux MEMBRES non admin).
    let (status, body) = get_json(&base, &token_a, &format!("/api/organizations/{id_b}")).await;
    assert_eq!(status, 404, "lecture cross-tenant : {body:?}");
    assert_eq!(body.expect("corps 404")["error"], "organization_not_found");

    // PATCH et DELETE : même invisibilité (et l'org de B n'est PAS modifiée).
    let client = reqwest::Client::new();
    let patch = client
        .patch(format!("{base}/api/organizations/{id_b}"))
        .bearer_auth(&token_a)
        .json(&json!({ "name": "Piraté" }))
        .send()
        .await
        .expect("requête PATCH");
    assert_eq!(patch.status().as_u16(), 404);

    let delete = client
        .delete(format!("{base}/api/organizations/{id_b}"))
        .bearer_auth(&token_a)
        .send()
        .await
        .expect("requête DELETE");
    assert_eq!(delete.status().as_u16(), 404);

    // Le listing de A ne contient jamais l'org de B.
    let (_, page) = get_json(&base, &token_a, "/api/organizations?page_size=100").await;
    let data = page.expect("page")["data"]
        .as_array()
        .expect("data")
        .clone();
    assert!(
        data.iter().all(|o| o["id"].as_i64() != Some(id_b)),
        "l'org B ne doit pas apparaître dans le listing de A"
    );

    // Et côté B, rien n'a bougé (ni renommage ni soft-delete).
    let token_b = access_token(&base, &org_b.admin_email).await;
    let (status, body) = get_json(&base, &token_b, &format!("/api/organizations/{id_b}")).await;
    assert_eq!(status, 200);
    assert_eq!(
        body.expect("corps")["slug"].as_str(),
        Some(org_b.slug.as_str())
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Listing : pagination, tri, recherche, filtre
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_paginates_sorts_and_filters() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;

    // L'appelant fonde 3 orgs : son périmètre = 4 (la jetable du harnais,
    // « Org e2e … » segment B2B, + les 3 ci-dessous).
    let alpha_slug = unique_slug();
    for (name, slug, segment) in [
        ("Alpha qualité", alpha_slug.clone(), Some("B2G")),
        ("Bravo qualité", unique_slug(), Some("B2B2C")),
        ("Charlie qualité", unique_slug(), None),
    ] {
        let mut body = json!({ "name": name, "slug": slug });
        if let Some(seg) = segment {
            body["segment"] = json!(seg);
        }
        let (s, b) = create_org(&base, &token, body).await;
        assert_eq!(s, 201, "setup {name} : {b:?}");
    }

    // Page 1 triée par nom : déterministe, total = 4.
    let (status, page) = get_json(&base, &token, "/api/organizations?page_size=2&sort=name").await;
    assert_eq!(status, 200);
    let page = page.expect("page 1");
    assert_eq!(page["total"].as_i64(), Some(4));
    assert_eq!(page["count"].as_i64(), Some(2));
    let names: Vec<&str> = page["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|o| o["name"].as_str().expect("name"))
        .collect();
    assert_eq!(names, ["Alpha qualité", "Bravo qualité"]);

    // Page 2 : Charlie + l'org du harnais.
    let (_, page2) = get_json(
        &base,
        &token,
        "/api/organizations?page_size=2&sort=name&page=2",
    )
    .await;
    let page2 = page2.expect("page 2");
    assert_eq!(page2["count"].as_i64(), Some(2));
    assert_eq!(page2["data"][0]["name"], "Charlie qualité");

    // Tri descendant (préfixe `-`) : l'org du harnais (« Org e2e … ») en tête.
    let (_, desc) = get_json(&base, &token, "/api/organizations?sort=-name").await;
    let first = desc.expect("page desc")["data"][0]["name"]
        .as_str()
        .expect("name")
        .to_string();
    assert!(first.starts_with("Org e2e"), "tri desc : {first}");

    // Recherche q sur le NOM (sous-chaîne insensible à la casse).
    let (_, by_name) = get_json(&base, &token, "/api/organizations?q=charlie").await;
    let by_name = by_name.expect("page q nom");
    assert_eq!(by_name["total"].as_i64(), Some(1));
    assert_eq!(by_name["data"][0]["name"], "Charlie qualité");

    // Recherche q sur le SLUG (fragment hex unique du slug d'Alpha — ne peut
    // matcher aucun nom).
    let fragment = &alpha_slug[4..];
    let (_, by_slug) = get_json(&base, &token, &format!("/api/organizations?q={fragment}")).await;
    let by_slug = by_slug.expect("page q slug");
    assert_eq!(by_slug["total"].as_i64(), Some(1));
    assert_eq!(by_slug["data"][0]["name"], "Alpha qualité");

    // Filtre segment : match exact — un segment NULL (Charlie) ne matche jamais.
    let (_, b2g) = get_json(&base, &token, "/api/organizations?segment=B2G").await;
    assert_eq!(b2g.expect("segment B2G")["total"].as_i64(), Some(1));
    let (_, b2b) = get_json(&base, &token, "/api/organizations?segment=B2B").await;
    assert_eq!(
        b2b.expect("segment B2B")["total"].as_i64(),
        Some(1),
        "seule l'org du harnais est B2B"
    );
}

#[tokio::test]
async fn list_rejects_unknown_sort_and_out_of_bounds_params() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;

    // Tri hors allowlist → 400 (la chaîne client n'atteint jamais le SQL).
    let (status, body) = get_json(&base, &token, "/api/organizations?sort=password").await;
    assert_eq!(status, 400, "{body:?}");

    // Bornes de pagination (conventions communes aux listings CRUD).
    let (status, _) = get_json(&base, &token, "/api/organizations?page=0").await;
    assert_eq!(status, 400);
    let (status, _) = get_json(&base, &token, "/api/organizations?page_size=101").await;
    assert_eq!(status, 400);

    // Filtre segment hors allowlist → 400 au boundary, pas de SQL.
    let (status, _) = get_json(&base, &token, "/api/organizations?segment=PME").await;
    assert_eq!(status, 400);
}

// ─────────────────────────────────────────────────────────────────────────────
// PATCH / DELETE
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn patch_updates_name_clears_segment_and_slug_is_immutable() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;
    let client = reqwest::Client::new();

    let slug = unique_slug();
    let (s, created) = create_org(
        &base,
        &token,
        json!({ "name": "Avant", "slug": slug.clone(), "segment": "B2B" }),
    )
    .await;
    assert_eq!(s, 201);
    let id = created.expect("corps")["id"].as_i64().expect("id");

    // Renommage + effacement du segment (`""` → NULL, convention PATCH).
    let res = client
        .patch(format!("{base}/api/organizations/{id}"))
        .bearer_auth(&token)
        .json(&json!({ "name": "Après", "segment": "" }))
        .send()
        .await
        .expect("requête PATCH");
    assert_eq!(res.status().as_u16(), 200);
    let body: Value = res.json().await.expect("corps PATCH");
    assert_eq!(body["name"], "Après");
    assert!(body["segment"].is_null(), "segment effacé : {body:?}");

    // Champ absent = inchangé : re-poser un segment ne touche pas le nom.
    let res = client
        .patch(format!("{base}/api/organizations/{id}"))
        .bearer_auth(&token)
        .json(&json!({ "segment": "B2G" }))
        .send()
        .await
        .expect("requête PATCH segment");
    assert_eq!(res.status().as_u16(), 200);
    let body: Value = res.json().await.expect("corps PATCH segment");
    assert_eq!(body["name"], "Après");
    assert_eq!(body["segment"], "B2G");

    // Segment hors allowlist : 400 au boundary.
    let res = client
        .patch(format!("{base}/api/organizations/{id}"))
        .bearer_auth(&token)
        .json(&json!({ "segment": "PME" }))
        .send()
        .await
        .expect("requête PATCH segment KO");
    assert_eq!(res.status().as_u16(), 400);

    // PATCH vide : 400 explicite (aucun champ).
    let res = client
        .patch(format!("{base}/api/organizations/{id}"))
        .bearer_auth(&token)
        .json(&json!({}))
        .send()
        .await
        .expect("requête PATCH vide");
    assert_eq!(res.status().as_u16(), 400);

    // Le slug est IMMUABLE en B6 : le champ n'existe pas au contrat — un corps
    // qui ne porte QUE `slug` équivaut à un PATCH vide (400)… et le slug ne
    // bouge pas d'un iota.
    let res = client
        .patch(format!("{base}/api/organizations/{id}"))
        .bearer_auth(&token)
        .json(&json!({ "slug": "slug-pirate" }))
        .send()
        .await
        .expect("requête PATCH slug");
    assert_eq!(res.status().as_u16(), 400);
    let (status, body) = get_json(&base, &token, &format!("/api/organizations/{id}")).await;
    assert_eq!(status, 200);
    assert_eq!(body.expect("corps")["slug"].as_str(), Some(slug.as_str()));
}

#[tokio::test]
async fn delete_soft_deletes_org_and_kills_its_logins() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let admin = access_token(&base, &org.admin_email).await;
    let client = reqwest::Client::new();

    let res = client
        .delete(format!("{base}/api/organizations/{}", org.org_id))
        .bearer_auth(&admin)
        .send()
        .await
        .expect("requête DELETE");
    assert_eq!(res.status().as_u16(), 204);

    // L'org disparaît du listing — le jeton, lui, reste techniquement valide
    // jusqu'à son expiration : il ne voit simplement plus rien.
    let (status, page) = get_json(&base, &admin, "/api/organizations?page_size=100").await;
    assert_eq!(status, 200);
    let data = page.expect("page")["data"]
        .as_array()
        .expect("data")
        .clone();
    assert!(
        data.iter().all(|o| o["id"].as_i64() != Some(org.org_id)),
        "l'org soft-supprimée ne doit plus être listée"
    );

    // GET direct : 404 (même invisibilité qu'une org inexistante).
    let (status, _) = get_json(&base, &admin, &format!("/api/organizations/{}", org.org_id)).await;
    assert_eq!(status, 404);

    // Le login d'un membre MONO-org meurt : db.rs filtre `deleted_at` au login
    // (et au refresh) — la membership seule ne suffit plus.
    let (status, body) = login(&base, &org.reader_email, DEMO_PASSWORD).await;
    assert_eq!(status, 401, "login après soft-delete : {body:?}");

    // DELETE rejoué : 404 (fetch_role_in_org exclut l'org soft-supprimée).
    let res = client
        .delete(format!("{base}/api/organizations/{}", org.org_id))
        .bearer_auth(&admin)
        .send()
        .await
        .expect("requête DELETE rejouée");
    assert_eq!(res.status().as_u16(), 404);

    // SOFT-delete vérifié EN BASE : la ligne survit, `deleted_at` posé — les
    // alert_events (FK RESTRICT) ne perdent donc jamais leur org.
    let soft_deleted: bool =
        sqlx::query_scalar("SELECT deleted_at IS NOT NULL FROM organizations WHERE id = $1")
            .bind(org.org_id)
            .fetch_one(&pool)
            .await
            .expect("la ligne org doit rester en base (soft-delete, pas DELETE)");
    assert!(soft_deleted, "deleted_at doit être posé");
}

// ─────────────────────────────────────────────────────────────────────────────
// Authentification requise
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn endpoints_require_bearer() {
    let base = spawn_app().await;
    let client = reqwest::Client::new();

    for (method, path) in [
        ("GET", "/api/organizations"),
        ("POST", "/api/organizations"),
        ("GET", "/api/organizations/1"),
        ("PATCH", "/api/organizations/1"),
        ("DELETE", "/api/organizations/1"),
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
