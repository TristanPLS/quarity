//! GET /api/measurements — lecture ClickHouse, auth requise, isolation multi-tenant.

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use crate::ch::validate_parameter;
use crate::error::AppError;
use crate::security::AuthUser;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct MeasurementsQuery {
    pub location_id: u64,
    pub parameter: String,
    pub from: String,
    pub to: String,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

pub async fn list_measurements(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<MeasurementsQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let parameter = validate_parameter(&q.parameter).ok_or(AppError::BadRequest(
        "parameter invalide (allowlist : pm25, pm10, no2, o3, so2, co)".into(),
    ))?;

    // Isolation multi-tenant : l'org du JWT doit suivre cette station.
    let owns = crate::db::org_owns_location(&state.pg, user.org_id, q.location_id as i64).await?;
    if !owns {
        return Err(AppError::Forbidden("location_not_in_org"));
    }

    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(100).clamp(1, 1000);
    let offset = (page - 1) * page_size;

    let rows = state
        .ch
        .query_measurements(q.location_id, parameter, &q.from, &q.to, page_size, offset)
        .await?;

    Ok(Json(json!({
        "page": page,
        "page_size": page_size,
        "count": rows.len(),
        "data": rows,
    })))
}
