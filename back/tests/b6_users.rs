//! Tests e2e du CRUD `/api/users` (B6) — membres de l'org du JWT.
//!
//! Prérequis : identiques à `tests/e2e.rs` (3 bases réelles + schéma + seed).
//! Chaque test crée son/ses orgs JETABLES (cf. `tests/common/mod.rs`) : la seed
//! n'est JAMAIS mutée. Une org jetable naît avec TROIS membres (admin /
//! gestionnaire / lecteur) — les assertions de `total` en tiennent compte.
//!
//! Spécificités de la ressource :
//! - mutations réservées au rôle `admin` : le code stable du 403 est
//!   `admin_required` (PAS `read_only_role` — même pour le lecteur) ;
//! - emails créés UNIQUES par test : la base persiste entre runs locaux et les
//!   tests tournent en parallèle, un email fixe collisionnerait (409) ;
//! - DELETE retire la membership, pas le compte : le login d'un compte mono-org
//!   supprimé répond 401 `invalid_credentials`.

use serde_json::{json, Value};

mod common;
use common::{access_token, create_test_org, login, pg_pool, spawn_app};

/// Mot de passe des membres créés PAR les tests (≥ 12 caractères — politique de
/// création, plus stricte que `DEMO_PASSWORD` ne l'exige au login).
const MEMBER_PASSWORD: &str = "MotDePasse#2026";

/// Suffixe unique (12 hex) pour fabriquer des emails sans collision inter-tests.
fn unique_suffix() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

/// Email jetable unique, en minuscules.
fn unique_email(local: &str) -> String {
    format!("{local}-{}@e2e.quarity.test", unique_suffix())
}

/// Corps de création minimal valide.
fn member_body(email: &str, full_name: &str, role: &str) -> Value {
    json!({
        "email": email,
        "full_name": full_name,
        "password": MEMBER_PASSWORD,
        "role": role,
    })
}

/// POST /api/users → (status, corps JSON éventuel).
async fn create_member(base: &str, token: &str, body: Value) -> (u16, Option<Value>) {
    let res = reqwest::Client::new()
        .post(format!("{base}/api/users"))
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

/// Id du compte de l'appelant (via `/api/auth/me` — les tests « soi-même »).
async fn my_user_id(base: &str, token: &str) -> i64 {
    let (status, me) = get_json(base, token, "/api/auth/me").await;
    assert_eq!(status, 200, "GET /api/auth/me : {me:?}");
    me.expect("corps me")["user_id"].as_i64().expect("user_id")
}

// ─────────────────────────────────────────────────────────────────────────────
// Création
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn create_returns_201_and_normalizes_email() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;

    // Email envoyé en casse mixte : il doit ressortir NORMALISÉ (minuscules).
    let email_sent = format!("Claire-{}@E2E.Quarity.TEST", unique_suffix());
    let email_expected = email_sent.to_lowercase();

    let (status, body) = create_member(
        &base,
        &token,
        member_body(&email_sent, "Claire Dubois", "lecteur"),
    )
    .await;
    let body = body.expect("corps 201");

    assert_eq!(status, 201, "création : {body:?}");
    assert_eq!(body["email"], email_expected.as_str());
    assert_eq!(body["full_name"], "Claire Dubois");
    assert_eq!(body["role"], "lecteur");
    assert_eq!(body["can_write"], false);
    assert_eq!(body["is_active"], true);
    assert!(body["joined_at"].is_string(), "joined_at : {body:?}");

    // Le membre créé est lisible en détail (même DTO).
    let id = body["id"].as_i64().expect("id");
    let (status, detail) = get_json(&base, &token, &format!("/api/users/{id}")).await;
    assert_eq!(status, 200);
    assert_eq!(
        detail.expect("corps détail")["email"],
        email_expected.as_str()
    );
}

#[tokio::test]
async fn create_duplicate_email_is_409_even_across_orgs() {
    // L'unicité d'email est GLOBALE (users_email_key) — pas par org : un compte
    // existant dans l'org B bloque aussi la création dans l'org A (l'« invitation »
    // d'un compte existant est une évolution future, hors B6).
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;
    let token_a = access_token(&base, &org_a.admin_email).await;
    let token_b = access_token(&base, &org_b.admin_email).await;

    let email = unique_email("doublon");
    let (s1, _) = create_member(&base, &token_a, member_body(&email, "Premier", "lecteur")).await;
    assert_eq!(s1, 201);

    // Même org : 409.
    let (s2, body) = create_member(&base, &token_a, member_body(&email, "Second", "lecteur")).await;
    assert_eq!(s2, 409, "doublon même org : {body:?}");
    assert_eq!(body.expect("corps 409")["error"], "conflict");

    // AUTRE org : 409 aussi (unicité globale).
    let (s3, body) = create_member(&base, &token_b, member_body(&email, "Tiers", "lecteur")).await;
    assert_eq!(s3, 409, "doublon cross-org : {body:?}");
    assert_eq!(body.expect("corps 409")["error"], "conflict");
}

#[tokio::test]
async fn create_rejects_short_password_unknown_role_and_bad_email() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;

    // Mot de passe de 11 caractères : sous la politique de création (12 minimum).
    let mut body = member_body(&unique_email("court"), "Trop Court", "lecteur");
    body["password"] = json!("Court#2026!");
    assert_eq!(body["password"].as_str().expect("pwd").len(), 11);
    let (status, _) = create_member(&base, &token, body).await;
    assert_eq!(status, 400, "mot de passe 11 caractères → 400 au boundary");

    // Rôle hors allowlist (admin/gestionnaire/lecteur).
    let (status, _) = create_member(
        &base,
        &token,
        member_body(&unique_email("role"), "Rôle Inconnu", "root"),
    )
    .await;
    assert_eq!(status, 400);

    // Email sans forme d'adresse.
    let (status, _) = create_member(
        &base,
        &token,
        member_body("pas-un-email", "Email Invalide", "lecteur"),
    )
    .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn created_member_can_login_but_cannot_administrate() {
    // (a) Le compte créé avec un mot de passe CHOISI doit pouvoir se connecter —
    // preuve que le hash Argon2id stocké correspond bien au secret transmis.
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let admin = access_token(&base, &org.admin_email).await;

    let email = unique_email("lecteur-login");
    let (status, _) = create_member(
        &base,
        &admin,
        member_body(&email, "Lecteur Connecté", "lecteur"),
    )
    .await;
    assert_eq!(status, 201);

    let (status, body) = login(&base, &email, MEMBER_PASSWORD).await;
    assert_eq!(status, 200, "login du membre créé : {body:?}");
    let token = body.expect("corps login")["access_token"]
        .as_str()
        .expect("access_token")
        .to_string();

    // … mais ce lecteur n'administre pas les membres : 403 `admin_required`.
    let (status, body) = create_member(
        &base,
        &token,
        member_body(&unique_email("tentative"), "Tentative", "lecteur"),
    )
    .await;
    assert_eq!(status, 403);
    assert_eq!(body.expect("corps 403")["error"], "admin_required");
}

// ─────────────────────────────────────────────────────────────────────────────
// RBAC : les mutations exigent le rôle admin (pas seulement can_write)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn manager_and_reader_get_403_admin_required_on_mutations() {
    // (b) Le gestionnaire a `can_write` mais N'EST PAS admin : gérer les membres
    // est une opération d'administration — code stable `admin_required` partout
    // (y compris pour le lecteur : RequireAdmin teste le rôle, pas can_write).
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let admin = access_token(&base, &org.admin_email).await;
    let manager = access_token(&base, &org.manager_email).await;
    let reader = access_token(&base, &org.reader_email).await;
    let client = reqwest::Client::new();

    // Cible des PATCH/DELETE : un membre créé par l'admin.
    let (_, created) = create_member(
        &base,
        &admin,
        member_body(&unique_email("cible"), "Cible Rbac", "lecteur"),
    )
    .await;
    let id = created.expect("corps")["id"].as_i64().expect("id");

    // POST refusé pour le gestionnaire ET le lecteur.
    for token in [&manager, &reader] {
        let (status, body) = create_member(
            &base,
            token,
            member_body(&unique_email("refus"), "Refusé", "lecteur"),
        )
        .await;
        assert_eq!(status, 403);
        assert_eq!(body.expect("corps 403")["error"], "admin_required");
    }

    // PATCH et DELETE refusés pour le gestionnaire.
    let patch = client
        .patch(format!("{base}/api/users/{id}"))
        .bearer_auth(&manager)
        .json(&json!({ "full_name": "Renommé" }))
        .send()
        .await
        .expect("requête PATCH");
    assert_eq!(patch.status().as_u16(), 403);

    let delete = client
        .delete(format!("{base}/api/users/{id}"))
        .bearer_auth(&manager)
        .send()
        .await
        .expect("requête DELETE");
    assert_eq!(delete.status().as_u16(), 403);

    // Lecture : OK pour le lecteur (les membres de SON org sont visibles).
    let (status, _) = get_json(&base, &reader, "/api/users").await;
    assert_eq!(status, 200);
    let (status, _) = get_json(&base, &reader, &format!("/api/users/{id}")).await;
    assert_eq!(status, 200);
}

// ─────────────────────────────────────────────────────────────────────────────
// LE rempart : isolation multi-tenant (404 anti-énumération)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cross_tenant_member_is_invisible() {
    // (f) Un admin de l'org B ne voit/modifie/supprime PAS un membre de l'org A.
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;
    let token_a = access_token(&base, &org_a.admin_email).await;
    let token_b = access_token(&base, &org_b.admin_email).await;
    let client = reqwest::Client::new();

    let (_, created) = create_member(
        &base,
        &token_a,
        member_body(&unique_email("secret"), "Membre Secret A", "lecteur"),
    )
    .await;
    let id = created.expect("corps")["id"].as_i64().expect("id");

    // GET : 404 — pas 403 — un id étranger est indistinguable d'un id inexistant.
    let (status, body) = get_json(&base, &token_b, &format!("/api/users/{id}")).await;
    assert_eq!(status, 404, "lecture cross-tenant : {body:?}");
    assert_eq!(body.expect("corps 404")["error"], "user_not_found");

    // PATCH et DELETE : même invisibilité (et le membre de A n'est PAS modifié).
    let patch = client
        .patch(format!("{base}/api/users/{id}"))
        .bearer_auth(&token_b)
        .json(&json!({ "full_name": "Piraté" }))
        .send()
        .await
        .expect("requête PATCH");
    assert_eq!(patch.status().as_u16(), 404);

    let delete = client
        .delete(format!("{base}/api/users/{id}"))
        .bearer_auth(&token_b)
        .send()
        .await
        .expect("requête DELETE");
    assert_eq!(delete.status().as_u16(), 404);

    // Le listing de B ne contient jamais le membre de A.
    let (_, page) = get_json(&base, &token_b, "/api/users?page_size=100").await;
    let data = page.expect("page")["data"]
        .as_array()
        .expect("data")
        .clone();
    assert!(
        data.iter().all(|u| u["id"].as_i64() != Some(id)),
        "le membre de l'org A ne doit pas apparaître chez B"
    );

    // Et côté A, rien n'a bougé (toujours membre, nom intact).
    let (status, body) = get_json(&base, &token_a, &format!("/api/users/{id}")).await;
    assert_eq!(status, 200);
    assert_eq!(body.expect("corps")["full_name"], "Membre Secret A");
}

// ─────────────────────────────────────────────────────────────────────────────
// Listing : pagination, tri, recherche, filtres
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_paginates_sorts_searches_and_filters() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;

    // 3 membres au nom marqué « Zz » : `q=Zz` isole les créations du test des
    // 3 comptes de l'org jetable (admin/gestionnaire/lecteur, cf. harnais).
    // Les locals d'emails sont ordonnés (alpha < bravo < charlie) pour le tri email.
    let suffix = unique_suffix();
    for (local, name, role) in [
        ("m-alpha", "Zz Alpha", "lecteur"),
        ("m-bravo", "Zz Bravo", "gestionnaire"),
        ("m-charlie", "Zz Charlie", "lecteur"),
    ] {
        let email = format!("{local}-{suffix}@e2e.quarity.test");
        let (s, b) = create_member(&base, &token, member_body(&email, name, role)).await;
        assert_eq!(s, 201, "création {name} : {b:?}");
    }

    // Sans filtre : 3 comptes du harnais + 3 créés.
    let (status, page) = get_json(&base, &token, "/api/users?page_size=100").await;
    assert_eq!(status, 200);
    assert_eq!(page.expect("page")["total"].as_i64(), Some(6));

    // Pagination + tri par nom, restreints aux créations (q sur full_name).
    let (_, page1) = get_json(&base, &token, "/api/users?q=Zz&page_size=2&sort=full_name").await;
    let page1 = page1.expect("page 1");
    assert_eq!(page1["total"].as_i64(), Some(3));
    assert_eq!(page1["count"].as_i64(), Some(2));
    let names: Vec<&str> = page1["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|u| u["full_name"].as_str().expect("full_name"))
        .collect();
    assert_eq!(names, ["Zz Alpha", "Zz Bravo"]);

    // Page 2.
    let (_, page2) = get_json(
        &base,
        &token,
        "/api/users?q=Zz&page_size=2&sort=full_name&page=2",
    )
    .await;
    let page2 = page2.expect("page 2");
    assert_eq!(page2["count"].as_i64(), Some(1));
    assert_eq!(page2["data"][0]["full_name"], "Zz Charlie");

    // Tri descendant (préfixe `-`).
    let (_, desc) = get_json(&base, &token, "/api/users?q=Zz&sort=-full_name").await;
    assert_eq!(
        desc.expect("page desc")["data"][0]["full_name"],
        "Zz Charlie"
    );

    // Tri par email (les locals sont ordonnés alpha < bravo < charlie).
    let (_, by_email) = get_json(&base, &token, "/api/users?q=Zz&sort=email").await;
    assert_eq!(
        by_email.expect("tri email")["data"][0]["full_name"],
        "Zz Alpha"
    );
    let (_, by_email_desc) = get_json(&base, &token, "/api/users?q=Zz&sort=-email").await;
    assert_eq!(
        by_email_desc.expect("tri -email")["data"][0]["full_name"],
        "Zz Charlie"
    );

    // Tri par joined_at (alias de sous-requête) : les comptes du harnais sont
    // entrés dans l'org AVANT les créations du test, l'admin en premier.
    let (status, joined) = get_json(&base, &token, "/api/users?sort=joined_at").await;
    assert_eq!(status, 200);
    assert_eq!(joined.expect("tri joined_at")["data"][0]["role"], "admin");

    // Recherche par fragment d'EMAIL (q porte sur email ET full_name).
    let (_, by_fragment) = get_json(&base, &token, &format!("/api/users?q=m-bravo-{suffix}")).await;
    let by_fragment = by_fragment.expect("page q email");
    assert_eq!(by_fragment["total"].as_i64(), Some(1));
    assert_eq!(by_fragment["data"][0]["full_name"], "Zz Bravo");

    // Filtre par rôle : Zz Bravo + le gestionnaire du harnais.
    let (_, managers) = get_json(&base, &token, "/api/users?role=gestionnaire").await;
    assert_eq!(managers.expect("page rôle")["total"].as_i64(), Some(2));
    // Combiné avec q : seul Zz Bravo reste.
    let (_, combined) = get_json(&base, &token, "/api/users?role=gestionnaire&q=Zz").await;
    assert_eq!(combined.expect("page rôle+q")["total"].as_i64(), Some(1));

    // Filtre is_active : tous les comptes du test sont actifs.
    let (_, actives) = get_json(&base, &token, "/api/users?is_active=true&page_size=100").await;
    assert_eq!(actives.expect("page actifs")["total"].as_i64(), Some(6));
    let (_, inactives) = get_json(&base, &token, "/api/users?is_active=false").await;
    assert_eq!(inactives.expect("page inactifs")["total"].as_i64(), Some(0));
}

#[tokio::test]
async fn list_rejects_unknown_sort_filter_and_out_of_bounds_page() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;

    // Tri hors allowlist → 400. `password` est ironiquement une VRAIE colonne de
    // la table (password_hash) : l'allowlist garantit qu'elle reste inatteignable.
    let (status, body) = get_json(&base, &token, "/api/users?sort=password").await;
    assert_eq!(status, 400, "{body:?}");
    let (status, _) = get_json(&base, &token, "/api/users?sort=password_hash").await;
    assert_eq!(status, 400);

    // Filtre rôle hors allowlist → 400 (validation déclarative).
    let (status, _) = get_json(&base, &token, "/api/users?role=root").await;
    assert_eq!(status, 400);

    // Bornes de pagination (mêmes conventions que les autres listings B6).
    let (status, _) = get_json(&base, &token, "/api/users?page=0").await;
    assert_eq!(status, 400);
    let (status, _) = get_json(&base, &token, "/api/users?page_size=101").await;
    assert_eq!(status, 400);
}

// ─────────────────────────────────────────────────────────────────────────────
// PATCH / DELETE
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn patch_updates_name_and_role() {
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;
    let client = reqwest::Client::new();

    let (_, created) = create_member(
        &base,
        &token,
        member_body(&unique_email("promu"), "Avant Promotion", "lecteur"),
    )
    .await;
    let id = created.expect("corps")["id"].as_i64().expect("id");

    // Renommage + promotion lecteur → gestionnaire en un seul PATCH.
    let res = client
        .patch(format!("{base}/api/users/{id}"))
        .bearer_auth(&token)
        .json(&json!({ "full_name": "Après Promotion", "role": "gestionnaire" }))
        .send()
        .await
        .expect("requête PATCH");
    assert_eq!(res.status().as_u16(), 200);
    let body: Value = res.json().await.expect("corps PATCH");
    assert_eq!(body["full_name"], "Après Promotion");
    assert_eq!(body["role"], "gestionnaire");
    assert_eq!(
        body["can_write"], true,
        "le rôle gestionnaire écrit : {body:?}"
    );

    // PATCH vide : 400 explicite (aucun champ).
    let res = client
        .patch(format!("{base}/api/users/{id}"))
        .bearer_auth(&token)
        .json(&json!({}))
        .send()
        .await
        .expect("requête PATCH vide");
    assert_eq!(res.status().as_u16(), 400);
}

#[tokio::test]
async fn patch_own_role_is_400_but_own_name_is_allowed() {
    // (c) Modifier SON propre rôle est refusé : un admin qui se rétrograde peut
    // laisser l'org sans admin. Le renommage de soi-même reste permis.
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;
    let client = reqwest::Client::new();
    let my_id = my_user_id(&base, &token).await;

    let res = client
        .patch(format!("{base}/api/users/{my_id}"))
        .bearer_auth(&token)
        .json(&json!({ "role": "lecteur" }))
        .send()
        .await
        .expect("requête PATCH rôle propre");
    assert_eq!(res.status().as_u16(), 400, "rétrogradation de soi-même");

    let res = client
        .patch(format!("{base}/api/users/{my_id}"))
        .bearer_auth(&token)
        .json(&json!({ "full_name": "Admin Renommé" }))
        .send()
        .await
        .expect("requête PATCH nom propre");
    assert_eq!(res.status().as_u16(), 200);
    let body: Value = res.json().await.expect("corps PATCH");
    assert_eq!(body["full_name"], "Admin Renommé");
    assert_eq!(body["role"], "admin", "le rôle n'a pas bougé : {body:?}");
}

#[tokio::test]
async fn delete_self_is_400() {
    // (d) Se retirer soi-même est refusé — symétrique du PATCH de rôle propre.
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;
    let my_id = my_user_id(&base, &token).await;

    let res = reqwest::Client::new()
        .delete(format!("{base}/api/users/{my_id}"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("requête DELETE soi-même");
    assert_eq!(res.status().as_u16(), 400);

    // L'admin est toujours membre de son org.
    let (status, _) = get_json(&base, &token, &format!("/api/users/{my_id}")).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn delete_member_returns_204_then_404_and_kills_login() {
    // (e) DELETE retire la membership : un compte MONO-org supprimé ne peut plus
    // se connecter (le login exige une membership) — 401 `invalid_credentials`.
    let base = spawn_app().await;
    let org = create_test_org(&pg_pool().await).await;
    let token = access_token(&base, &org.admin_email).await;
    let client = reqwest::Client::new();

    let email = unique_email("ephemere");
    let (_, created) = create_member(
        &base,
        &token,
        member_body(&email, "Membre Éphémère", "lecteur"),
    )
    .await;
    let id = created.expect("corps")["id"].as_i64().expect("id");

    // Avant suppression : le compte se connecte (référence pour le 401 d'après).
    let (status, _) = login(&base, &email, MEMBER_PASSWORD).await;
    assert_eq!(status, 200);

    let res = client
        .delete(format!("{base}/api/users/{id}"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("requête DELETE");
    assert_eq!(res.status().as_u16(), 204);

    // Plus membre : invisible en détail…
    let (status, _) = get_json(&base, &token, &format!("/api/users/{id}")).await;
    assert_eq!(status, 404);

    // … suppression rejouée : 404 (idempotence côté observabilité, pas de 500)…
    let res = client
        .delete(format!("{base}/api/users/{id}"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("requête DELETE rejouée");
    assert_eq!(res.status().as_u16(), 404);

    // … et le login est mort (compte sans plus aucune membership).
    let (status, body) = login(&base, &email, MEMBER_PASSWORD).await;
    assert_eq!(status, 401, "login après retrait : {body:?}");
    assert_eq!(body.expect("corps 401")["error"], "invalid_credentials");

    // Mais le COMPTE survit : DELETE retire la MEMBERSHIP, jamais la ligne
    // `users` (un compte peut appartenir à d'autres orgs — désactiver
    // globalement depuis une org serait un débordement cross-tenant). Vérifié
    // en base : la ligne existe toujours, active — une refactorisation qui
    // basculerait sur `DELETE FROM users` ferait échouer CE test.
    let survives: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1 AND is_active)")
            .bind(id)
            .fetch_one(&pg_pool().await)
            .await
            .expect("lecture du compte post-retrait");
    assert!(
        survives,
        "le compte global doit survivre au retrait de la membership"
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
        ("GET", "/api/users"),
        ("POST", "/api/users"),
        ("GET", "/api/users/1"),
        ("PATCH", "/api/users/1"),
        ("DELETE", "/api/users/1"),
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
