//! GET /health — liveness + readiness des 3 dépendances.

use axum::extract::State;
use axum::Json;
use serde_json::json;

use crate::state::AppState;

/// Liveness + readiness des 3 dépendances (Postgres, ClickHouse, Redis).
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "`status` vaut `ok` si les 3 dépendances répondent, `degraded` sinon (le code HTTP reste 200)", body = crate::openapi::HealthResponse),
    )
)]
pub async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let pg = crate::db::ping(&state.pg).await.is_ok();
    let ch = state.ch.ping().await.is_ok();

    let redis = {
        let mut conn = state.redis.clone();
        let pong: redis::RedisResult<String> = redis::cmd("PING").query_async(&mut conn).await;
        matches!(pong, Ok(ref s) if s == "PONG")
    };

    let status = if pg && ch && redis { "ok" } else { "degraded" };
    Json(json!({
        "status": status,
        "postgres": pg,
        "clickhouse": ch,
        "redis": redis,
    }))
}
