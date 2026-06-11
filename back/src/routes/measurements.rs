//! GET /api/measurements — lecture ClickHouse, auth requise, isolation multi-tenant.

use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use utoipa::IntoParams;
use validator::Validate;

use crate::ch::validate_parameter;
use crate::error::AppError;
use crate::security::AuthUser;
use crate::state::AppState;
use crate::validation::{parse_datetime_ish, validate_datetime_ish, ValidatedQuery};

#[derive(Deserialize, IntoParams, Validate)]
#[into_params(parameter_in = Query)]
#[validate(schema(function = "validate_date_range"))]
pub struct MeasurementsQuery {
    /// Identifiant OpenAQ de la station — doit être suivie par l'org du JWT.
    #[param(example = 4085)]
    pub location_id: u64,
    /// Polluant (allowlist : `pm25`, `pm10`, `no2`, `o3`, `so2`, `co`).
    #[param(example = "pm25")]
    pub parameter: String,
    /// Début de la plage (inclus). Formats : `YYYY-MM-DD`, `YYYY-MM-DD[T ]HH:MM[:SS[.fff]]`, RFC 3339.
    #[validate(custom(function = "validate_datetime_ish"))]
    #[param(example = "2026-04-01")]
    pub from: String,
    /// Fin de la plage (exclue), mêmes formats — doit être ≥ `from`.
    #[validate(custom(function = "validate_datetime_ish"))]
    #[param(example = "2026-07-01")]
    pub to: String,
    /// Page 1-indexée (défaut : 1 ; max 1 000 000 — borne anti-débordement de l'offset).
    #[validate(range(min = 1, max = 1000000, message = "page attendue 1..=1000000"))]
    pub page: Option<u32>,
    /// Taille de page, clampée à 1..=1000 (défaut : 100).
    pub page_size: Option<u32>,
}

/// Validation croisée : `from` ≤ `to` (l'égalité reste permise — plage vide valide,
/// le front autorise from == to via ses attributs min/max). Ne se prononce que si les
/// deux champs parsent — sinon les erreurs par champ suffisent.
fn validate_date_range(q: &MeasurementsQuery) -> Result<(), validator::ValidationError> {
    if let (Some(from), Some(to)) = (parse_datetime_ish(&q.from), parse_datetime_ish(&q.to)) {
        if from > to {
            return Err(validator::ValidationError::new("plage_invalide")
                .with_message("`from` doit être antérieur ou égal à `to`".into()));
        }
    }
    Ok(())
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
        (status = 400, description = "`parameter` hors allowlist, dates `from`/`to` invalides ou `from` > `to` (corps `ErrorBody`) — ou paramètre de requête obligatoire manquant/mal typé (rejet de l'extracteur `Query`, corps texte)", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Station non suivie par l'org du JWT (`location_not_in_org`) — isolation multi-tenant", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_measurements(
    State(state): State<AppState>,
    user: AuthUser,
    ValidatedQuery(q): ValidatedQuery<MeasurementsQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    Ok(Json(
        measurements_page_for_org(&state, user.org_id, &q).await?,
    ))
}

/// Cœur de la lecture de mesures d'une org — partagé entre `/api/measurements` (JWT) et
/// `/api/public/measurements` (clé API, B9b-2). `org_id` provient TOUJOURS de l'auth ;
/// 403 si la station n'est pas suivie par l'org (isolation multi-tenant).
pub async fn measurements_page_for_org(
    state: &AppState,
    org_id: i64,
    q: &MeasurementsQuery,
) -> Result<serde_json::Value, AppError> {
    let parameter = validate_parameter(&q.parameter).ok_or(AppError::BadRequest(
        "parameter invalide (allowlist : pm25, pm10, no2, o3, so2, co)".into(),
    ))?;

    // Isolation multi-tenant : l'org doit suivre cette station.
    let owns = crate::db::org_owns_location(&state.pg, org_id, q.location_id as i64).await?;
    if !owns {
        return Err(AppError::Forbidden("location_not_in_org"));
    }

    // `page` est borné par la validation (1..=1_000_000) ; le calcul en u64 évite le
    // débordement SILENCIEUX d'un u32*u32 en build release (profil sans overflow-checks),
    // qui renverrait une page de résultats fausse. L'offset reste < u32::MAX par
    // construction ; `try_from` sature par sécurité plutôt que de wrapper.
    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(100).clamp(1, 1000);
    let offset = u32::try_from((u64::from(page) - 1) * u64::from(page_size)).unwrap_or(u32::MAX);

    let rows = state
        .ch
        .query_measurements(q.location_id, parameter, &q.from, &q.to, page_size, offset)
        .await?;

    Ok(json!({
        "page": page,
        "page_size": page_size,
        "count": rows.len(),
        "data": rows,
    }))
}
