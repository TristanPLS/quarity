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
use std::sync::Arc;
use uuid::Uuid;

/// Mot de passe partagé de tous les comptes de démo (cf. seed).
const DEMO_PASSWORD: &str = "Quarity2026!";

/// Démarre le back sur un port éphémère et renvoie l'URL de base (`http://127.0.0.1:PORT`).
///
/// Limites de rate-limit relevées très haut : Redis est PARTAGÉ entre les tests (qui
/// tournent en parallèle), les compteurs par email/IP s'accumuleraient sinon d'un test
/// à l'autre et déclencheraient des 429 parasites. Le comportement du rate-limit est
/// testé explicitement (avec sa limite réelle) dans
/// `sixth_failed_login_on_same_email_is_rate_limited`.
async fn spawn_app() -> String {
    spawn_app_with(|c| {
        c.rate_limit_login_email_per_min = 10_000;
        c.rate_limit_login_ip_per_min = 10_000;
        c.rate_limit_refresh_ip_per_min = 10_000;
    })
    .await
}

/// Variante de `spawn_app` permettant d'ajuster la config pour UN test (chaque test
/// lance sa propre instance du back, seules les bases sont partagées).
async fn spawn_app_with(tweak: impl FnOnce(&mut Config)) -> String {
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
// Durcissement auth (revue sécurité 2026-06-06)
// ─────────────────────────────────────────────────────────────────────────────

/// Décode un segment base64url SANS padding (en-tête/payload JWT).
/// Implémentation locale minimaliste : pas de crate `base64` dans les dépendances,
/// et on veut précisément prouver que le payload JWT est lisible par n'importe qui.
fn b64url_decode(s: &str) -> Vec<u8> {
    fn val(c: u8) -> u32 {
        match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'-' => 62,
            b'_' => 63,
            _ => panic!("caractère base64url invalide : {}", c as char),
        }
    }
    let mut out = Vec::new();
    for chunk in s.trim_end_matches('=').as_bytes().chunks(4) {
        let mut acc: u32 = 0;
        let mut bits: u32 = 0;
        for &c in chunk {
            acc = (acc << 6) | val(c);
            bits += 6;
        }
        acc >>= bits % 8; // bits de bourrage du dernier quartet
        bits -= bits % 8;
        while bits > 0 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    out
}

/// S4 : le refresh token doit être INDÉPENDANT du claim `jti` de l'access token.
/// (Avant durcissement, refresh_token == jti : le payload JWT étant du base64 lisible,
/// une fuite d'access token 15 min donnait une session renouvelable 7 j.)
#[tokio::test]
async fn refresh_token_is_independent_from_access_jti() {
    let base = spawn_app().await;
    let (status, body) = login(&base, "sophie@agglo-riviera.fr", DEMO_PASSWORD).await;
    assert_eq!(status, 200);
    let body = body.expect("corps");
    let access = body["access_token"].as_str().expect("access_token");
    let refresh = body["refresh_token"].as_str().expect("refresh_token");

    // Payload JWT = 2e segment — déchiffrable sans le secret.
    let payload_b64 = access.split('.').nth(1).expect("JWT à 3 segments");
    let payload: Value =
        serde_json::from_slice(&b64url_decode(payload_b64)).expect("payload JWT en JSON");
    let jti = payload["jti"].as_str().expect("claim jti");

    assert_ne!(
        refresh, jti,
        "le refresh token ne doit JAMAIS être dérivable de l'access token (claim jti)"
    );
}

/// Un compte désactivé ne doit plus pouvoir rafraîchir sa session (le refresh est révoqué).
/// Compte utilisé : `lea@cityair.app` — seul compte de son org et utilisé par AUCUN autre
/// test (les tests tournent en parallèle : désactiver sophie/audit créerait des courses).
#[tokio::test]
async fn deactivated_user_cannot_refresh() {
    let base = spawn_app().await;
    let email = "lea@cityair.app";
    let pool =
        quarity_back::db::make_pg_pool(&std::env::var("DATABASE_URL").expect("DATABASE_URL"))
            .await
            .expect("pool Postgres de test");

    let (status, body) = login(&base, email, DEMO_PASSWORD).await;
    assert_eq!(
        status, 200,
        "login initial (compte encore actif) : {body:?}"
    );
    let refresh_token = body.expect("corps")["refresh_token"]
        .as_str()
        .expect("refresh_token")
        .to_string();

    // Désactive le compte…
    sqlx::query("UPDATE users SET is_active = FALSE WHERE lower(email::text) = lower($1)")
        .bind(email)
        .execute(&pool)
        .await
        .expect("désactivation du compte de test");

    // …puis zone SANS panic jusqu'à la restauration (sinon le seed resterait corrompu
    // pour les exécutions suivantes). On capture le statut, on restaure, on asserte.
    let refresh_status = reqwest::Client::new()
        .post(format!("{base}/api/auth/refresh"))
        .json(&json!({ "refresh_token": refresh_token }))
        .send()
        .await
        .map(|r| r.status().as_u16());

    // Restauration SYSTÉMATIQUE avant toute assertion susceptible d'échouer.
    sqlx::query("UPDATE users SET is_active = TRUE WHERE lower(email::text) = lower($1)")
        .bind(email)
        .execute(&pool)
        .await
        .expect("restauration is_active du compte de test");

    assert_eq!(
        refresh_status.expect("requête refresh"),
        401,
        "un compte désactivé ne doit plus pouvoir rafraîchir sa session"
    );
}

/// A4 : 6 tentatives de login consécutives sur un même email → la 6e est rate-limitée (429).
/// Email dédié inexistant ET unique par exécution : ne pollue aucun autre test, et évite
/// les faux 429 si la suite est relancée en moins de 60 s — le compteur Redis de la
/// fenêtre fixe (`ratelimit:login:email:*`) expire en 60 s.
#[tokio::test]
async fn sixth_failed_login_on_same_email_is_rate_limited() {
    // Limite email RÉELLE (5/min) pour cette instance ; limites IP relevées pour ne pas
    // interférer avec le compteur IP partagé entre tous les tests de la suite.
    let base = spawn_app_with(|c| {
        c.rate_limit_login_email_per_min = 5;
        c.rate_limit_login_ip_per_min = 10_000;
        c.rate_limit_refresh_ip_per_min = 10_000;
    })
    .await;
    let email = format!("rate-limit-{}@example.invalid", Uuid::new_v4());

    let mut statuses = Vec::new();
    let mut last_body = None;
    for _ in 0..6 {
        let (status, body) = login(&base, &email, "mauvais-mot-de-passe").await;
        statuses.push(status);
        last_body = body;
    }

    assert!(
        statuses[..5].iter().all(|s| *s == 401),
        "les 5 premières tentatives doivent échouer en 401 (limite email : 5/min) : {statuses:?}"
    );
    assert_eq!(
        statuses[5], 429,
        "la 6e tentative sur le même email doit être rate-limitée : {statuses:?}"
    );
    assert_eq!(
        last_body.expect("corps 429")["error"],
        "rate_limited",
        "le corps du 429 doit porter le code stable `rate_limited`"
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
