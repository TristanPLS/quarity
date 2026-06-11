//! Assemblage du routeur + couches transverses.

pub mod alert_rules;
pub mod aqi;
pub mod auth;
pub mod exposure_profiles;
pub mod health;
pub mod measurements;
pub mod organizations;
pub mod tracked_locations;
pub mod users;
pub mod ws;

use axum::extract::DefaultBodyLimit;
use axum::http::{header, HeaderValue, Method};
use axum::routing::{delete, get, post};
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

use crate::state::AppState;

/// CORS strict : allowlist d'origines (`CORS_ALLOWED_ORIGINS`). Pas de `allow_credentials`,
/// l'auth étant par jeton Bearer (aucun cookie d'auth ambiant) — cf. revue sécurité.
fn cors_layer(allowed_origins: &[String]) -> CorsLayer {
    let origins: Vec<HeaderValue> = allowed_origins
        .iter()
        .filter_map(|o| o.parse::<HeaderValue>().ok())
        .collect();
    CorsLayer::new()
        .allow_origin(origins)
        // PATCH/DELETE : requis par le CRUD B6 (mutations partielles + suppressions).
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

pub fn build_router(state: AppState) -> Router {
    let cors = cors_layer(&state.cfg.cors_allowed_origins);

    // Routes d'authentification : corps minuscules (identifiants, ou un refresh token).
    // Borne de corps SERRÉE et explicite (8 Kio) — sans elle, la seule protection serait
    // la limite axum par défaut (2 Mio) : la validation `max=512` du mot de passe ne
    // s'applique qu'APRÈS désérialisation complète du JSON. Rend la borne anti-DoS visible
    // et auditable (un corps démesuré est rejeté en 413 AVANT tout travail).
    let auth_routes = Router::new()
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/refresh", post(auth::refresh))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/auth/me", get(auth::me))
        .layer(DefaultBodyLimit::max(8 * 1024));

    // Routes CRUD (B6) : corps JSON bornés à 64 Kio — le plus gros corps légitime
    // (POST lieu : 50 stations + 50 règles) reste sous 16 Kio, marge ×4.
    let crud_routes = Router::new()
        .route(
            "/api/tracked-locations",
            get(tracked_locations::list).post(tracked_locations::create),
        )
        .route(
            "/api/tracked-locations/{id}",
            get(tracked_locations::get_one)
                .patch(tracked_locations::update)
                .delete(tracked_locations::delete),
        )
        .route(
            "/api/alert-rules",
            get(alert_rules::list).post(alert_rules::create),
        )
        .route(
            "/api/alert-rules/{id}",
            get(alert_rules::get_one)
                .patch(alert_rules::update)
                .delete(alert_rules::delete),
        )
        // Force-check B7 : réévaluation immédiate d'une règle (hot path matching).
        .route("/api/alert-rules/{id}/run", post(alert_rules::run))
        // Profils d'exposition (B9a) + leurs seuils adaptés (sous-ressource).
        .route(
            "/api/exposure-profiles",
            get(exposure_profiles::list).post(exposure_profiles::create),
        )
        .route(
            "/api/exposure-profiles/{id}",
            get(exposure_profiles::get_one)
                .patch(exposure_profiles::update)
                .delete(exposure_profiles::delete),
        )
        .route(
            "/api/exposure-profiles/{id}/thresholds",
            get(exposure_profiles::list_thresholds).post(exposure_profiles::create_threshold),
        )
        .route(
            "/api/exposure-profiles/{id}/thresholds/{threshold_id}",
            delete(exposure_profiles::delete_threshold),
        )
        .route("/api/users", get(users::list).post(users::create))
        .route(
            "/api/users/{id}",
            get(users::get_one)
                .patch(users::update)
                .delete(users::delete),
        )
        .route(
            "/api/organizations",
            get(organizations::list).post(organizations::create),
        )
        .route(
            "/api/organizations/{id}",
            get(organizations::get_one)
                .patch(organizations::update)
                .delete(organizations::delete),
        )
        .layer(DefaultBodyLimit::max(64 * 1024));

    Router::new()
        .route("/health", get(health::health))
        .merge(auth_routes)
        .merge(crud_routes)
        .route("/api/measurements", get(measurements::list_measurements))
        // AQI courant des lieux suivis (B10) — jauge du dashboard, réutilise B5 Q2.
        .route("/api/aqi", get(aqi::overview))
        // Alertes temps réel B8 : WebSocket par org (auth via jeton en query string —
        // un navigateur ne pose pas d'en-tête Authorization sur une WebSocket native).
        .route("/api/ws", get(ws::alerts_ws))
        // En-têtes de sécurité — API JSON only ⇒ CSP « default-src 'none' ».
        .layer(SetResponseHeaderLayer::overriding(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
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
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
        // Doc OpenAPI (A2) — montée APRÈS les couches ci-dessus : la CSP API
        // (`default-src 'none'`) casserait l'UI HTML/JS de Swagger. Le routeur docs
        // porte ses propres en-têtes de sécurité (cf. `openapi::docs_router`).
        .merge(crate::openapi::docs_router())
}
