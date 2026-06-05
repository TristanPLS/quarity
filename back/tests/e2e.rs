//! Tests d'intégration end-to-end du back Quarity.
//!
//! ⚠️ Nécessitent les 3 dépendances RÉELLES en marche, chargées avec schéma + seed :
//!   - PostgreSQL  (`db/sql/01_schema.sql` + `db/sql/02_seed.sql`)
//!   - Redis
//!   - ClickHouse  (`db/clickhouse/01_schema.sql` + `db/clickhouse/02_seed.sql`)
//!
//! Variables d'environnement attendues (mêmes que le back) :
//!   DATABASE_URL, REDIS_URL, CLICKHOUSE_URL, CLICKHOUSE_USER, CLICKHOUSE_PASSWORD,
//!   CLICKHOUSE_DB, JWT_SECRET (>= 32), JWT_ACCESS_TTL_SECONDS (optionnel).
//!
//! En local :
//!   docker compose up -d                      # lève + auto-seed les 3 bases
//!   # exporter les variables (cf. .env), puis depuis back/ :
//!   cargo test
//! En CI : voir `.github/workflows/ci.yml` (services + chargement schéma/seed).
//!
//! Carte multi-tenant utilisée (cf. `db/sql/02_seed.sql`) :
//!   - org `agglo-riviera` SUIT la station 1001 ; `sophie@agglo-riviera.fr` (admin).
//!   - org `groupeindus`   NE suit PAS 1001 ; `audit@groupeindus.com` (lecteur, mono-org).
//!
//! Donc `audit@groupeindus` interrogeant 1001 doit recevoir 403 (isolation multi-tenant).

use quarity_back::{config::Config, routes::build_router, state::AppState};
use serde_json::{json, Value};

/// Mot de passe partagé de tous les comptes de démo (cf. seed).
const DEMO_PASSWORD: &str = "Quarity2026!";

/// Démarre le back sur un port éphémère et renvoie l'URL de base (`http://127.0.0.1:PORT`).
async fn spawn_app() -> String {
    let cfg = Config::from_env().expect(
        "config depuis l'env (DATABASE_URL, REDIS_URL, CLICKHOUSE_URL/USER/PASSWORD, JWT_SECRET>=32)",
    );
    let state = AppState::connect(cfg)
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
async fn login(base: &str, email: &str, password: &str) -> (u16, Option<Value>) {
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

/// Login d'un compte de démo et renvoie son access token (échoue si le login échoue).
async fn access_token(base: &str, email: &str) -> String {
    let (status, body) = login(base, email, DEMO_PASSWORD).await;
    assert_eq!(status, 200, "login {email} doit réussir : {body:?}");
    body.expect("corps login")["access_token"]
        .as_str()
        .expect("access_token présent")
        .to_string()
}

/// GET /api/measurements (token optionnel) → status code.
async fn measurements_status(
    base: &str,
    token: Option<&str>,
    location_id: u64,
    parameter: &str,
) -> u16 {
    let mut req = reqwest::Client::new().get(format!(
        "{base}/api/measurements?location_id={location_id}&parameter={parameter}&from=2026-04-01&to=2026-07-01"
    ));
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    req.send()
        .await
        .expect("requête measurements")
        .status()
        .as_u16()
}

// ─────────────────────────────────────────────────────────────────────────────
// LE rempart : isolation multi-tenant
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cross_tenant_read_is_forbidden() {
    let base = spawn_app().await;
    // `audit@groupeindus.com` ∈ `groupeindus`, qui NE suit PAS la station 1001 (suivie par agglo-riviera).
    let token = access_token(&base, "audit@groupeindus.com").await;
    let status = measurements_status(&base, Some(&token), 1001, "pm25").await;
    assert_eq!(
        status, 403,
        "un tenant ne doit JAMAIS lire les mesures d'une station d'un autre tenant"
    );
}

#[tokio::test]
async fn same_tenant_read_is_allowed() {
    let base = spawn_app().await;
    // `sophie@agglo-riviera.fr` ∈ `agglo-riviera`, qui SUIT la station 1001.
    let token = access_token(&base, "sophie@agglo-riviera.fr").await;
    let status = measurements_status(&base, Some(&token), 1001, "pm25").await;
    assert_eq!(
        status, 200,
        "un tenant doit pouvoir lire les mesures d'une de ses stations"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Authentification
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn login_success_returns_tokens() {
    let base = spawn_app().await;
    let (status, body) = login(&base, "sophie@agglo-riviera.fr", DEMO_PASSWORD).await;
    assert_eq!(status, 200);
    let body = body.expect("corps");
    assert!(
        body["access_token"].as_str().is_some(),
        "access_token attendu"
    );
    assert!(
        body["refresh_token"].as_str().is_some(),
        "refresh_token attendu"
    );
    assert_eq!(body["token_type"], "Bearer");
}

#[tokio::test]
async fn login_wrong_password_is_unauthorized() {
    let base = spawn_app().await;
    let (status, _) = login(&base, "sophie@agglo-riviera.fr", "mauvais-mot-de-passe").await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn login_unknown_user_is_unauthorized() {
    let base = spawn_app().await;
    let (status, _) = login(&base, "inconnu@example.com", DEMO_PASSWORD).await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn me_returns_authenticated_identity() {
    let base = spawn_app().await;
    let token = access_token(&base, "sophie@agglo-riviera.fr").await;
    let res = reqwest::Client::new()
        .get(format!("{base}/api/auth/me"))
        .bearer_auth(token)
        .send()
        .await
        .expect("requête me");
    assert_eq!(res.status().as_u16(), 200);
    let body = res.json::<Value>().await.expect("corps me");
    assert_eq!(body["email"], "sophie@agglo-riviera.fr");
    assert!(body["org_id"].is_number(), "org_id doit être présent");
}

#[tokio::test]
async fn refresh_rotates_and_invalidates_old_token() {
    let base = spawn_app().await;
    let (status, body) = login(&base, "sophie@agglo-riviera.fr", DEMO_PASSWORD).await;
    assert_eq!(status, 200);
    let old_refresh = body.expect("corps")["refresh_token"]
        .as_str()
        .expect("refresh_token")
        .to_string();

    // 1er refresh avec l'ancien token : OK.
    let res1 = reqwest::Client::new()
        .post(format!("{base}/api/auth/refresh"))
        .json(&json!({ "refresh_token": old_refresh }))
        .send()
        .await
        .expect("refresh #1");
    assert_eq!(res1.status().as_u16(), 200, "refresh valide doit réussir");

    // Réutiliser l'ANCIEN refresh (rotation) : doit échouer, il a été révoqué.
    let res2 = reqwest::Client::new()
        .post(format!("{base}/api/auth/refresh"))
        .json(&json!({ "refresh_token": old_refresh }))
        .send()
        .await
        .expect("refresh #2");
    assert_eq!(
        res2.status().as_u16(),
        401,
        "l'ancien refresh token ne doit plus être valide après rotation"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Validation / surface d'entrée
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn measurements_without_bearer_is_unauthorized() {
    let base = spawn_app().await;
    let status = measurements_status(&base, None, 1001, "pm25").await;
    assert_eq!(
        status, 401,
        "sans Bearer, l'accès aux mesures doit être refusé"
    );
}

#[tokio::test]
async fn invalid_parameter_is_bad_request() {
    let base = spawn_app().await;
    let token = access_token(&base, "sophie@agglo-riviera.fr").await;
    // 'xyz' hors allowlist (pm25/pm10/no2/o3/so2/co) → 400 avant toute requête base.
    let status = measurements_status(&base, Some(&token), 1001, "xyz").await;
    assert_eq!(
        status, 400,
        "un paramètre hors allowlist doit être rejeté en 400"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// CORS & en-têtes de sécurité (A3)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn security_headers_are_present() {
    let base = spawn_app().await;
    let res = reqwest::Client::new()
        .get(format!("{base}/health"))
        .send()
        .await
        .expect("requête health");
    let h = res.headers();
    assert_eq!(h.get("x-content-type-options").unwrap(), "nosniff");
    assert_eq!(h.get("x-frame-options").unwrap(), "DENY");
    assert!(h.get("content-security-policy").is_some(), "CSP attendue");
    assert!(
        h.get("referrer-policy").is_some(),
        "Referrer-Policy attendu"
    );
}

#[tokio::test]
async fn cors_allows_configured_origin() {
    let base = spawn_app().await;
    // Origine par défaut autorisée (cf. Config : http://localhost:3000).
    let res = reqwest::Client::new()
        .get(format!("{base}/health"))
        .header("Origin", "http://localhost:3000")
        .send()
        .await
        .expect("requête health");
    assert_eq!(
        res.headers()
            .get("access-control-allow-origin")
            .and_then(|v| v.to_str().ok()),
        Some("http://localhost:3000")
    );
}

#[tokio::test]
async fn cors_rejects_unknown_origin() {
    let base = spawn_app().await;
    let res = reqwest::Client::new()
        .get(format!("{base}/health"))
        .header("Origin", "http://evil.example")
        .send()
        .await
        .expect("requête health");
    // Origine non autorisée → pas d'en-tête ACAO (le navigateur bloquerait la lecture).
    assert!(res.headers().get("access-control-allow-origin").is_none());
}
