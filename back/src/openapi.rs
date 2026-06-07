//! Doc OpenAPI (A2) : agrégation des annotations `#[utoipa::path]` + Swagger UI sur `/api/docs`.
//!
//! Choix de conception :
//! - **Zéro changement de comportement** des endpoints : les handlers qui répondent en
//!   `Json<serde_json::Value>` gardent leur forme ; leur contrat est documenté ici via des
//!   schémas « doc-only » (`MeResponse`, `HealthResponse`, …) qui décrivent le JSON réel.
//! - **`vendored`** (Cargo.toml) : les assets Swagger UI sont embarqués dans le binaire,
//!   aucun téléchargement au build — requis pour les builds Docker/CI hors réseau.
//! - **CSP dédiée** : la CSP API (`default-src 'none'`) casserait l'UI HTML/JS ; le routeur
//!   docs est donc monté APRÈS les couches API (cf. `routes/mod.rs`) et porte ses propres
//!   en-têtes (CSP adaptée à Swagger UI + nosniff + X-Frame + Referrer-Policy).

use axum::http::{header, HeaderValue};
use axum::Router;
use tower_http::set_header::SetResponseHeaderLayer;
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

use crate::routes::{auth, health, measurements};

/// Nom du schéma de sécurité référencé par les annotations `security(("bearer_jwt" = []))`.
const BEARER_JWT: &str = "bearer_jwt";

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Quarity API",
        description = "API de surveillance de la qualité de l'air (mesures OpenAQ, \
                       multi-tenant). Auth par JWT Bearer : `POST /api/auth/login` puis \
                       en-tête `Authorization: Bearer <access_token>`.",
        version = env!("CARGO_PKG_VERSION"),
    ),
    paths(
        health::health,
        auth::login,
        auth::refresh,
        auth::logout,
        auth::me,
        measurements::list_measurements,
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "health", description = "Liveness / readiness du service et de ses dépendances"),
        (name = "auth", description = "Authentification JWT : login, refresh (rotation), logout, identité"),
        (name = "measurements", description = "Mesures de qualité de l'air (ClickHouse, isolation multi-tenant)"),
    )
)]
pub struct ApiDoc;

/// Déclare le schéma `bearer_jwt` (HTTP Bearer, format JWT) dans `components.securitySchemes`.
struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        // `components` existe toujours ici : les schémas des paths sont déjà collectés.
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            BEARER_JWT,
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Schémas « doc-only » : décrivent les réponses construites en `serde_json::json!`
// dans les handlers. Toute évolution d'un handler doit être répercutée ici.
// ─────────────────────────────────────────────────────────────────────────────

/// Corps d'erreur uniforme de l'API (cf. `error.rs::AppError`).
#[derive(ToSchema)]
#[schema(example = json!({ "error": "bad_request", "message": "parameter invalide (allowlist : pm25, pm10, no2, o3, so2, co)" }))]
pub struct ErrorBody {
    /// Code d'erreur stable, exploitable par les clients (`bad_request`, `invalid_credentials`,
    /// `missing_bearer`, `token_expired`, `invalid_token`, `location_not_in_org`,
    /// `rate_limited`, `internal_error`, …).
    pub error: String,
    /// Message humain (peut être identique au code).
    pub message: String,
}

/// Réponse de `GET /health`.
#[derive(ToSchema)]
#[schema(example = json!({ "status": "ok", "postgres": true, "clickhouse": true, "redis": true }))]
pub struct HealthResponse {
    /// `ok` si les 3 dépendances répondent, sinon `degraded`.
    pub status: String,
    pub postgres: bool,
    pub clickhouse: bool,
    pub redis: bool,
}

/// Réponse de `POST /api/auth/logout`.
#[derive(ToSchema)]
#[schema(example = json!({ "status": "logged_out" }))]
pub struct LogoutResponse {
    pub status: String,
}

/// Réponse de `GET /api/auth/me` : identité dérivée du JWT + profil Postgres.
#[derive(ToSchema)]
#[schema(example = json!({
    "user_id": 1, "email": "sophie@agglo-riviera.fr", "full_name": "Sophie Marchand",
    "org_id": 1, "role": "admin", "can_write": true
}))]
pub struct MeResponse {
    pub user_id: i64,
    pub email: String,
    pub full_name: String,
    /// Org du JWT — JAMAIS un paramètre client (isolation multi-tenant).
    pub org_id: i64,
    /// Code du rôle (`roles.code`).
    pub role: String,
    pub can_write: bool,
}

/// Page de mesures renvoyée par `GET /api/measurements`.
#[derive(ToSchema)]
pub struct MeasurementsPage {
    /// Page demandée (1-indexée).
    pub page: u32,
    /// Taille de page effective (clampée à 1..=1000).
    pub page_size: u32,
    /// Nombre d'éléments dans `data`.
    pub count: usize,
    pub data: Vec<crate::ch::MeasurementRow>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Routeur Swagger UI
// ─────────────────────────────────────────────────────────────────────────────

/// Routeur de la doc : UI sur `/api/docs` (+ redirection `/api/docs` → `/api/docs/`),
/// spécification JSON sur `/api/docs/openapi.json`.
///
/// CSP spécifique : Swagger UI a besoin de scripts/styles servis en same-origin
/// (`'self'`), de styles inline injectés au runtime (`'unsafe-inline'` sur style-src
/// uniquement) et d'images `data:`. Tout le reste reste fermé.
pub fn docs_router() -> Router {
    let swagger: Router = SwaggerUi::new("/api/docs")
        .url("/api/docs/openapi.json", ApiDoc::openapi())
        .into();

    swagger
        .layer(SetResponseHeaderLayer::overriding(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
                 img-src 'self' data:; font-src 'self'; connect-src 'self'; \
                 frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
            ),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=63072000; includeSubDomains"),
        ))
}
