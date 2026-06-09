//! Harnais partagé des tests d'intégration (e2e auth/measurements + CRUD B6).
//!
//! Mêmes prérequis que `tests/e2e.rs` : les 3 bases RÉELLES en marche, schéma +
//! seed chargés, variables d'env du back exportées (cf. en-tête de `e2e.rs`).
//!
//! ## Isolation des tests CRUD (B6)
//!
//! Les tests tournent EN PARALLÈLE sur des bases partagées : un test CRUD ne doit
//! JAMAIS muter les données de la seed (d'autres tests s'y adossent — comptes démo,
//! station 1001, compteurs). [`create_test_org`] fournit donc à chaque test une org
//! JETABLE (slug `e2e-<uuid>`) avec ses trois rôles : les mutations restent
//! confinées au tenant du test. Pas de nettoyage : les slugs sont uniques, la CI
//! repart d'une base fraîche, et un reliquat local n'influence aucun autre test.
//!
//! Tous les items sont `#[allow(dead_code)]` : chaque binaire de test n'utilise
//! qu'une partie du harnais (le lint verrait des morts différents selon le binaire).

#![allow(dead_code)]

use quarity_back::{config::Config, routes::build_router, state::AppState};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

/// Mot de passe partagé de tous les comptes de démo (cf. seed) — les comptes des
/// orgs jetables réutilisent le même hash (cf. [`create_test_org`]).
pub const DEMO_PASSWORD: &str = "Quarity2026!";

/// Démarre le back sur un port éphémère et renvoie l'URL de base (`http://127.0.0.1:PORT`).
///
/// Limites de rate-limit relevées très haut : Redis est PARTAGÉ entre les tests (qui
/// tournent en parallèle), les compteurs par email/IP s'accumuleraient sinon d'un test
/// à l'autre et déclencheraient des 429 parasites. Le comportement du rate-limit est
/// testé explicitement (avec sa limite réelle) dans
/// `e2e.rs::sixth_failed_login_on_same_email_is_rate_limited`.
pub async fn spawn_app() -> String {
    spawn_app_with(|c| {
        c.rate_limit_login_email_per_min = 10_000;
        c.rate_limit_login_ip_per_min = 10_000;
        c.rate_limit_refresh_ip_per_min = 10_000;
    })
    .await
}

/// Variante de `spawn_app` permettant d'ajuster la config pour UN test (chaque test
/// lance sa propre instance du back, seules les bases sont partagées).
pub async fn spawn_app_with(tweak: impl FnOnce(&mut Config)) -> String {
    let cfg = Config::from_env().expect(
        "config depuis l'env (DATABASE_URL, REDIS_URL, CLICKHOUSE_URL/USER/PASSWORD, JWT_SECRET>=32)",
    );
    let mut cfg = (*cfg).clone();
    tweak(&mut cfg);
    let state = AppState::connect(Arc::new(cfg))
        .await
        .expect("connexion Postgres/Redis/ClickHouse (les 3 bases doivent tourner + être seedées)");
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind port éphémère");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serveur de test");
    });
    format!("http://{addr}")
}

/// POST /api/auth/login → (status, corps JSON éventuel).
pub async fn login(base: &str, email: &str, password: &str) -> (u16, Option<Value>) {
    let res = reqwest::Client::new()
        .post(format!("{base}/api/auth/login"))
        .json(&json!({ "email": email, "password": password }))
        .send()
        .await
        .expect("requête login");
    let status = res.status().as_u16();
    let body = res.json::<Value>().await.ok();
    (status, body)
}

/// Login d'un compte (mot de passe démo) et renvoie son access token.
pub async fn access_token(base: &str, email: &str) -> String {
    let (status, body) = login(base, email, DEMO_PASSWORD).await;
    assert_eq!(status, 200, "login {email} doit réussir : {body:?}");
    body.expect("corps login")["access_token"]
        .as_str()
        .expect("access_token présent")
        .to_string()
}

/// Pool Postgres direct (setup des orgs jetables — hors du chemin testé).
pub async fn pg_pool() -> PgPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL pour le setup des tests");
    PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("connexion Postgres (setup tests)")
}

/// Org jetable créée par [`create_test_org`] : un compte par rôle, tous avec
/// [`DEMO_PASSWORD`].
pub struct TestOrg {
    pub org_id: i64,
    pub slug: String,
    /// Rôle `admin` (can_write + can_manage_org).
    pub admin_email: String,
    /// Rôle `gestionnaire` (can_write, pas de gestion d'org).
    pub manager_email: String,
    /// Rôle `lecteur` (lecture seule → 403 `read_only_role` sur les mutations).
    pub reader_email: String,
}

/// Crée une org jetable + 3 comptes (admin / gestionnaire / lecteur), directement
/// en base (le setup ne passe PAS par l'API : c'est elle qu'on teste). Le hash du
/// mot de passe est REPRIS du compte seed `sophie@…` — zéro coût Argon2 par test,
/// et `DEMO_PASSWORD` reste le sésame unique de toute la suite.
pub async fn create_test_org(pool: &PgPool) -> TestOrg {
    let suffix = Uuid::new_v4().simple().to_string();
    let suffix = &suffix[..12];
    let slug = format!("e2e-{suffix}");

    let phc: String = sqlx::query_scalar(
        "SELECT password_hash FROM users WHERE email = 'sophie@agglo-riviera.fr'",
    )
    .fetch_one(pool)
    .await
    .expect("hash du compte seed sophie@agglo-riviera.fr (seed chargée ?)");

    let org_id: i64 = sqlx::query_scalar(
        "INSERT INTO organizations (name, slug, segment) VALUES ($1, $2, 'B2B') RETURNING id",
    )
    .bind(format!("Org e2e {suffix}"))
    .bind(&slug)
    .fetch_one(pool)
    .await
    .expect("insertion org jetable");

    let mut emails = Vec::with_capacity(3);
    for (local, role) in [
        ("admin", "admin"),
        ("manager", "gestionnaire"),
        ("reader", "lecteur"),
    ] {
        let email = format!("{local}-{suffix}@e2e.quarity.test");
        let user_id: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, full_name) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(&email)
        .bind(&phc)
        .bind(format!("{local} e2e {suffix}"))
        .fetch_one(pool)
        .await
        .expect("insertion user jetable");

        sqlx::query(
            "INSERT INTO memberships (org_id, user_id, role_id)
             SELECT $1, $2, id FROM roles WHERE code = $3",
        )
        .bind(org_id)
        .bind(user_id)
        .bind(role)
        .execute(pool)
        .await
        .expect("insertion membership jetable");

        emails.push(email);
    }

    TestOrg {
        org_id,
        slug,
        admin_email: emails[0].clone(),
        manager_email: emails[1].clone(),
        reader_email: emails[2].clone(),
    }
}
