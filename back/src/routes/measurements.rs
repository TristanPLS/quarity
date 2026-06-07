//! GET /api/measurements — lecture ClickHouse, auth requise, isolation multi-tenant.

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use utoipa::IntoParams;

use crate::ch::validate_parameter;
use crate::error::AppError;
use crate::security::AuthUser;
use crate::state::AppState;

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct MeasurementsQuery {
    /// Identifiant OpenAQ de la station — doit être suivie par l'org du JWT.
    #[param(example = 4085)]
    pub location_id: u64,
    /// Polluant (allowlist : `pm25`, `pm10`, `no2`, `o3`, `so2`, `co`).
    #[param(example = "pm25")]
    pub parameter: String,
    /// Début de la plage (inclus), datetime « best effort » (ex. `2026-04-01` ou RFC 3339).
    #[param(example = "2026-04-01")]
    pub from: String,
    /// Fin de la plage (exclue).
    #[param(example = "2026-07-01")]
    pub to: String,
    /// Page 1-indexée (défaut : 1).
    pub page: Option<u32>,
    /// Taille de page, clampée à 1..=1000 (défaut : 100).
    pub page_size: Option<u32>,
}

/// Mesures dédupliquées d'une station suivie par l'org de l'appelant.
#[utoipa::path(
    get,
    path = "/api/measurements",
    tag = "measurements",
    params(MeasurementsQuery),
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Page de mesures (tri `measured_at` décroissant)", body = crate::openapi::MeasurementsPage),
        (status = 400, description = "`parameter` hors allowlist (corps `ErrorBody`) — ou paramètre de requête obligatoire manquant/mal typé (rejet de l'extracteur `Query`, corps texte)", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Station non suivie par l'org du JWT (`location_not_in_org`) — isolation multi-tenant", body = crate::openapi::ErrorBody),
    )
)]
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
