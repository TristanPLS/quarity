//! Assemblage du routeur + couches transverses.

pub mod auth;
pub mod health;
pub mod measurements;

use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    // CORS permissif pour le walking skeleton ; allowlist stricte des origins = Jalon 3.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/health", get(health::health))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/refresh", post(auth::refresh))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/auth/me", get(auth::me))
        .route("/api/measurements", get(measurements::list_measurements))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}
