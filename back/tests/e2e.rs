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

use serde_json::{json, Value};
use uuid::Uuid;

// Harnais partagé (B6) : spawn_app/login/access_token vivent dans `tests/common/`
// — mêmes helpers pour ce fichier et les suites CRUD `b6_*.rs`.
mod common;
use common::{access_token, login, spawn_app, spawn_app_with, DEMO_PASSWORD};

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
// Validation au boundary (A5 — crate validator)
// ─────────────────────────────────────────────────────────────────────────────

/// A5 : un mot de passe démesuré est rejeté AVANT la vérification Argon2 (anti-DoS).
#[tokio::test]
async fn login_overlong_password_is_bad_request() {
    let base = spawn_app().await;
    let (status, body) = login(&base, "sophie@agglo-riviera.fr", &"x".repeat(600)).await;
    assert_eq!(
        status, 400,
        "un mot de passe > 512 caractères doit être rejeté en 400 (validation, pas Argon2)"
    );
    assert_eq!(
        body.expect("corps 400")["error"],
        "bad_request",
        "le corps doit porter le code stable `bad_request` (ErrorBody)"
    );
}

/// A5 : une date `from` arbitraire est rejetée en 400 au boundary.
/// (Avant A5, elle traversait jusqu'à ClickHouse et ressortait en 500.)
#[tokio::test]
async fn measurements_garbage_date_is_bad_request() {
    let base = spawn_app().await;
    let token = access_token(&base, "sophie@agglo-riviera.fr").await;
    let res = reqwest::Client::new()
        .get(format!(
            "{base}/api/measurements?location_id=1001&parameter=pm25&from=n-importe-quoi&to=2026-07-01"
        ))
        .bearer_auth(token)
        .send()
        .await
        .expect("requête measurements");
    assert_eq!(
        res.status().as_u16(),
        400,
        "une date invalide doit donner 400, jamais 500"
    );
    let body = res.json::<Value>().await.expect("corps 400");
    assert_eq!(
        body["error"], "bad_request",
        "le corps doit porter le code stable `bad_request` (ErrorBody)"
    );
}

/// A5 : `from` strictement postérieur à `to` → 400 (l'égalité reste permise,
/// le front autorise from == to).
#[tokio::test]
async fn measurements_inverted_range_is_bad_request() {
    let base = spawn_app().await;
    let token = access_token(&base, "sophie@agglo-riviera.fr").await;
    let status = reqwest::Client::new()
        .get(format!(
            "{base}/api/measurements?location_id=1001&parameter=pm25&from=2026-07-01&to=2026-04-01"
        ))
        .bearer_auth(token)
        .send()
        .await
        .expect("requête measurements")
        .status()
        .as_u16();
    assert_eq!(status, 400, "from > to doit être rejeté en 400");
}

/// A5 : un refresh_token vide est rejeté par la validation (400 ErrorBody),
/// sans aller interroger Redis.
#[tokio::test]
async fn refresh_empty_token_is_bad_request() {
    let base = spawn_app().await;
    let res = reqwest::Client::new()
        .post(format!("{base}/api/auth/refresh"))
        .json(&json!({ "refresh_token": "" }))
        .send()
        .await
        .expect("requête refresh");
    assert_eq!(res.status().as_u16(), 400);
    let body = res.json::<Value>().await.expect("corps 400");
    assert_eq!(body["error"], "bad_request");
}

// ─────────────────────────────────────────────────────────────────────────────
// Doc OpenAPI /api/docs (A2)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn api_docs_serves_swagger_ui() {
    let base = spawn_app().await;
    let res = reqwest::Client::new()
        .get(format!("{base}/api/docs/"))
        .send()
        .await
        .expect("requête /api/docs/");
    assert_eq!(res.status().as_u16(), 200);

    let headers = res.headers().clone();
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with("text/html"),
        "content-type HTML attendu : {ct}"
    );

    // Le routeur docs porte SA PROPRE CSP (pas la CSP API `default-src 'none'`,
    // qui bloquerait les scripts/styles de l'UI) + les autres en-têtes durcis.
    let csp = headers
        .get("content-security-policy")
        .and_then(|v| v.to_str().ok())
        .expect("CSP attendue sur la doc");
    assert!(
        csp.contains("script-src 'self'"),
        "CSP docs doit autoriser les scripts same-origin : {csp}"
    );
    assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
    assert_eq!(headers.get("x-frame-options").unwrap(), "DENY");

    let body = res.text().await.expect("corps HTML");
    assert!(
        body.contains("swagger-ui"),
        "la page doit embarquer Swagger UI (assets vendored)"
    );
}

#[tokio::test]
async fn api_docs_redirects_to_trailing_slash() {
    let base = spawn_app().await;
    // Client SANS suivi de redirection : on veut observer la redirection elle-même.
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client sans redirect");
    let res = client
        .get(format!("{base}/api/docs"))
        .send()
        .await
        .expect("requête /api/docs");
    assert!(
        res.status().is_redirection(),
        "/api/docs doit rediriger vers /api/docs/ : {}",
        res.status()
    );
    assert_eq!(
        res.headers().get("location").and_then(|v| v.to_str().ok()),
        Some("/api/docs/")
    );
}

#[tokio::test]
async fn openapi_json_is_complete() {
    let base = spawn_app().await;
    let res = reqwest::Client::new()
        .get(format!("{base}/api/docs/openapi.json"))
        .send()
        .await
        .expect("requête openapi.json");
    assert_eq!(res.status().as_u16(), 200);
    let spec = res.json::<Value>().await.expect("spec JSON");

    assert!(
        spec["openapi"].as_str().unwrap_or("").starts_with("3."),
        "version OpenAPI 3.x attendue : {:?}",
        spec["openapi"]
    );
    assert_eq!(spec["info"]["title"], "Quarity API");

    // Les 6 routes du walking skeleton doivent être documentées.
    for path in [
        "/health",
        "/api/auth/login",
        "/api/auth/refresh",
        "/api/auth/logout",
        "/api/auth/me",
        "/api/measurements",
    ] {
        assert!(
            spec["paths"][path].is_object(),
            "chemin {path} absent de la spec"
        );
    }

    // Schéma de sécurité bearer JWT déclaré et référencé par les routes protégées.
    let scheme = &spec["components"]["securitySchemes"]["bearer_jwt"];
    assert_eq!(scheme["type"], "http");
    assert_eq!(scheme["scheme"], "bearer");
    assert!(
        spec["paths"]["/api/measurements"]["get"]["security"]
            .as_array()
            .is_some_and(|s| !s.is_empty()),
        "GET /api/measurements doit exiger bearer_jwt"
    );

    // L'isolation multi-tenant (403) est un invariant : elle doit être documentée.
    assert!(
        spec["paths"]["/api/measurements"]["get"]["responses"]["403"].is_object(),
        "la réponse 403 (isolation multi-tenant) doit être documentée"
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

// ─────────────────────────────────────────────────────────────────────────────
// Rotation atomique du refresh (P1) + logout anti-oracle (P3) + expiration (P4)
// ─────────────────────────────────────────────────────────────────────────────

/// Login complet → (access_token, refresh_token).
async fn login_tokens(base: &str, email: &str) -> (String, String) {
    let (status, body) = login(base, email, DEMO_PASSWORD).await;
    assert_eq!(status, 200, "login {email} doit réussir : {body:?}");
    let body = body.expect("corps login");
    (
        body["access_token"]
            .as_str()
            .expect("access_token")
            .to_string(),
        body["refresh_token"]
            .as_str()
            .expect("refresh_token")
            .to_string(),
    )
}

/// POST /api/auth/logout (Bearer access + corps `refresh_token`) → (status, corps JSON).
async fn logout(base: &str, access: &str, refresh_token: &str) -> (u16, Value) {
    let res = reqwest::Client::new()
        .post(format!("{base}/api/auth/logout"))
        .bearer_auth(access)
        .json(&json!({ "refresh_token": refresh_token }))
        .send()
        .await
        .expect("requête logout");
    let status = res.status().as_u16();
    let body = res.json::<Value>().await.expect("corps logout");
    (status, body)
}

/// POST /api/auth/refresh → status (sonde de validité d'un refresh token).
async fn refresh_status(base: &str, refresh_token: &str) -> u16 {
    reqwest::Client::new()
        .post(format!("{base}/api/auth/refresh"))
        .json(&json!({ "refresh_token": refresh_token }))
        .send()
        .await
        .expect("requête refresh")
        .status()
        .as_u16()
}

/// P1 : la rotation du refresh est ATOMIQUE (GETDEL). Deux requêtes concurrentes portant le
/// MÊME refresh token ne peuvent pas réussir toutes les deux : exactement une obtient un
/// nouveau couple (200), l'autre est rejetée (401). Sans atomicité (read puis delete
/// séparés), les deux pouvaient passer le read avant le delete et dupliquer la session.
#[tokio::test]
async fn concurrent_refresh_with_same_token_consumes_it_once() {
    let base = spawn_app().await;
    let (_, refresh) = login_tokens(&base, "sophie@agglo-riviera.fr").await;

    let url = format!("{base}/api/auth/refresh");
    let payload = json!({ "refresh_token": refresh });
    let (r1, r2) = tokio::join!(
        reqwest::Client::new().post(&url).json(&payload).send(),
        reqwest::Client::new().post(&url).json(&payload).send(),
    );
    let mut got = [
        r1.expect("refresh #1").status().as_u16(),
        r2.expect("refresh #2").status().as_u16(),
    ];
    got.sort_unstable();
    assert_eq!(
        got,
        [200, 401],
        "un seul des deux refresh concurrents doit réussir, l'autre être rejeté : {got:?}"
    );
}

/// P3 : le propriétaire qui se déconnecte révoque bien SON refresh token (il ne marche plus).
#[tokio::test]
async fn logout_revokes_own_refresh_token() {
    let base = spawn_app().await;
    let (access, refresh) = login_tokens(&base, "sophie@agglo-riviera.fr").await;

    let (status, body) = logout(&base, &access, &refresh).await;
    assert_eq!(status, 200, "le logout doit répondre 200");
    assert_eq!(body["status"], "logged_out");

    assert_eq!(
        refresh_status(&base, &refresh).await,
        401,
        "après logout, l'ancien refresh token ne doit plus être valide"
    );
}

/// P3 ANTI-ORACLE : déconnecter le refresh d'AUTRUI ne le révoque PAS (le token de la
/// victime reste valide). Sinon le logout serait un oracle de révocation des tokens d'autrui.
#[tokio::test]
async fn logout_does_not_revoke_another_users_refresh_token() {
    let base = spawn_app().await;
    let (attacker_access, _) = login_tokens(&base, "sophie@agglo-riviera.fr").await;
    let (_, victim_refresh) = login_tokens(&base, "audit@groupeindus.com").await;

    // L'attaquant (sophie) tente de déconnecter le refresh de la victime (audit).
    let (status, body) = logout(&base, &attacker_access, &victim_refresh).await;
    assert_eq!(status, 200, "réponse identique (anti-oracle), jamais 403");
    assert_eq!(body["status"], "logged_out");

    assert_eq!(
        refresh_status(&base, &victim_refresh).await,
        200,
        "le refresh d'un autre utilisateur ne doit JAMAIS être révoqué par le logout d'un tiers"
    );
}

/// P3 ANTI-ORACLE : la réponse du logout est INDISTINGUABLE — statut + corps identiques —
/// que le refresh soit le sien, inconnu, ou celui d'autrui. Un attaquant ne peut donc rien
/// déduire sur la validité ni l'appartenance d'un refresh token.
#[tokio::test]
async fn logout_response_is_indistinguishable_across_cases() {
    let base = spawn_app().await;
    let (access, own_refresh) = login_tokens(&base, "sophie@agglo-riviera.fr").await;
    let (_, foreign_refresh) = login_tokens(&base, "audit@groupeindus.com").await;
    let unknown_refresh = Uuid::new_v4().to_string();

    // Cas « autrui » et « inconnu » d'abord (ils ne consomment rien) ; le cas
    // « propriétaire » en dernier car il révoque réellement son propre token.
    let foreign = logout(&base, &access, &foreign_refresh).await;
    let unknown = logout(&base, &access, &unknown_refresh).await;
    let own = logout(&base, &access, &own_refresh).await;

    assert_eq!(
        foreign, unknown,
        "autrui vs inconnu : réponses indistinguables"
    );
    assert_eq!(
        unknown, own,
        "inconnu vs propriétaire : réponses indistinguables"
    );
    assert_eq!(own.0, 200);
    assert_eq!(own.1["status"], "logged_out");
}

/// P4 : un access token EXPIRÉ (au-delà de la leeway de 5 s) est rejeté en 401
/// `token_expired` sur les routes protégées — chemin central de durée de vie de session.
#[tokio::test]
async fn expired_access_token_is_rejected() {
    let base = spawn_app().await;
    let secret = std::env::var("JWT_SECRET").expect("JWT_SECRET");

    // Token forgé avec le BON secret mais déjà expiré (exp = now - 1 h, bien au-delà de la
    // leeway de 5 s) : signature valide, expiration dépassée → rejet attendu.
    let expired = quarity_back::security::issue_access_token(
        secret.as_bytes(),
        1, // user_id (sans importance : rejet à l'extraction, avant tout accès base)
        1, // org_id
        "admin",
        true,
        &Uuid::new_v4().to_string(),
        -3600, // ttl négatif → exp dans le passé
    )
    .expect("forge d'un token expiré");

    let res = reqwest::Client::new()
        .get(format!("{base}/api/auth/me"))
        .bearer_auth(&expired)
        .send()
        .await
        .expect("requête me");
    assert_eq!(
        res.status().as_u16(),
        401,
        "un token expiré doit donner 401"
    );
    let body = res.json::<Value>().await.expect("corps 401");
    assert_eq!(
        body["error"], "token_expired",
        "le code d'erreur doit distinguer l'expiration (`token_expired`)"
    );

    // Même rejet sur /measurements, AVANT toute requête base.
    let status = measurements_status(&base, Some(&expired), 1001, "pm25").await;
    assert_eq!(
        status, 401,
        "un token expiré doit aussi être refusé sur /measurements"
    );
}

/// P4 (défense en profondeur) : un token signé avec un AUTRE secret (forgé) est rejeté en
/// 401 `invalid_token` — la confiance repose sur la signature HS256, jamais sur le contenu.
/// Verrouille la résistance à la falsification du jeton (et donc du claim `org_id`).
#[tokio::test]
async fn token_signed_with_wrong_secret_is_rejected() {
    let base = spawn_app().await;
    // Secret différent de celui du serveur (assez varié pour ne pas ressembler au vrai).
    let forged = quarity_back::security::issue_access_token(
        b"un-tout-autre-secret-de-test-0123456789",
        1,
        1,
        "admin",
        true,
        &Uuid::new_v4().to_string(),
        3600,
    )
    .expect("forge d'un token mal signé");

    let res = reqwest::Client::new()
        .get(format!("{base}/api/auth/me"))
        .bearer_auth(&forged)
        .send()
        .await
        .expect("requête me");
    assert_eq!(
        res.status().as_u16(),
        401,
        "un token signé avec un autre secret doit être refusé"
    );
    let body = res.json::<Value>().await.expect("corps 401");
    assert_eq!(body["error"], "invalid_token");
}

// ─────────────────────────────────────────────────────────────────────────────
// Durcissement mineurs back : offset de pagination borné + corps /auth borné
// ─────────────────────────────────────────────────────────────────────────────

/// L'offset de pagination ne peut pas déborder : une `page` hors borne (1..=1_000_000)
/// est rejetée en 400. Avant, `page` énorme × `page_size` wrappait l'offset u32 en build
/// release (profil sans overflow-checks) → page de résultats fausse silencieusement.
#[tokio::test]
async fn measurements_out_of_range_page_is_bad_request() {
    let base = spawn_app().await;
    let token = access_token(&base, "sophie@agglo-riviera.fr").await;
    let res = reqwest::Client::new()
        .get(format!(
            "{base}/api/measurements?location_id=1001&parameter=pm25&from=2026-04-01&to=2026-07-01&page=5000000"
        ))
        .bearer_auth(token)
        .send()
        .await
        .expect("requête measurements");
    assert_eq!(
        res.status().as_u16(),
        400,
        "une page hors borne (1..=1_000_000) doit donner 400, pas un offset wrappé"
    );
    let body = res.json::<Value>().await.expect("corps 400");
    assert_eq!(body["error"], "bad_request");
}

/// `page_size` hors borne (1..=1000) est rejeté en 400 (validation) au lieu d'un clamp
/// SILENCIEUX à 1000 — cohérence avec les listings CRUD (`listing.rs`). Le `.clamp()` aval
/// reste un filet de défense en profondeur.
#[tokio::test]
async fn measurements_out_of_range_page_size_is_bad_request() {
    let base = spawn_app().await;
    let token = access_token(&base, "sophie@agglo-riviera.fr").await;
    let res = reqwest::Client::new()
        .get(format!(
            "{base}/api/measurements?location_id=1001&parameter=pm25&from=2026-04-01&to=2026-07-01&page_size=5000"
        ))
        .bearer_auth(token)
        .send()
        .await
        .expect("requête measurements");
    assert_eq!(
        res.status().as_u16(),
        400,
        "un page_size hors borne (1..=1000) doit donner 400, pas un clamp silencieux"
    );
    let body = res.json::<Value>().await.expect("corps 400");
    assert_eq!(body["error"], "bad_request");
}

/// La borne de corps serrée sur `/auth` (8 Kio) rejette un payload démesuré AVANT
/// désérialisation (413), rendant explicite la protection anti-DoS plutôt que de
/// s'appuyer sur la limite axum implicite de 2 Mio.
#[tokio::test]
async fn oversized_auth_body_is_rejected() {
    let base = spawn_app().await;
    // Corps ~64 Kio (mot de passe gigantesque) — dépasse la borne 8 Kio de /auth.
    let big = "x".repeat(64 * 1024);
    let res = reqwest::Client::new()
        .post(format!("{base}/api/auth/login"))
        .json(&json!({ "email": "sophie@agglo-riviera.fr", "password": big }))
        .send()
        .await
        .expect("requête login");
    assert_eq!(
        res.status().as_u16(),
        413,
        "un corps > 8 Kio sur /auth doit être rejeté en 413 (Payload Too Large)"
    );
}
