//! API publique (B9b-2) — endpoints **read-only** authentifiés par **clé API**
//! (`X-API-Key`), scopés à l'org de la clé et soumis au quota/rate-limit de son
//! abonnement (cf. extracteur [`crate::security::ApiKeyAuth`]). Réutilisent la logique
//! des endpoints JWT via des fonctions partagées — un seul endroit de vérité par calcul.

use axum::extract::State;
use axum::Json;

use crate::error::AppError;
use crate::routes::aqi::AqiOverview;
use crate::routes::measurements::MeasurementsQuery;
use crate::security::ApiKeyAuth;
use crate::state::AppState;
use crate::validation::ValidatedQuery;

/// AQI courant des lieux suivis de l'org de la clé.
#[utoipa::path(
    get,
    path = "/api/public/aqi",
    tag = "public-api",
    security(("api_key" = [])),
    responses(
        (status = 200, description = "AQI courant par lieu suivi de l'org de la clé", body = AqiOverview),
        (status = 401, description = "Clé API manquante, invalide, révoquée ou expirée", body = crate::openapi::ErrorBody),
        (status = 429, description = "Rate-limit/minute ou quota mensuel du plan dépassé (`rate_limited`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn aqi(
    State(state): State<AppState>,
    auth: ApiKeyAuth,
) -> Result<Json<AqiOverview>, AppError> {
    Ok(Json(
        crate::routes::aqi::overview_for_org(&state, auth.org_id).await?,
    ))
}

/// Mesures dédupliquées d'une station suivie par l'org de la clé.
#[utoipa::path(
    get,
    path = "/api/public/measurements",
    tag = "public-api",
    params(MeasurementsQuery),
    security(("api_key" = [])),
    responses(
        (status = 200, description = "Page de mesures (tri `measured_at` décroissant)", body = crate::openapi::MeasurementsPage),
        (status = 400, description = "Paramètres de requête invalides", body = crate::openapi::ErrorBody),
        (status = 401, description = "Clé API manquante, invalide, révoquée ou expirée", body = crate::openapi::ErrorBody),
        (status = 403, description = "Station non suivie par l'org de la clé (`location_not_in_org`)", body = crate::openapi::ErrorBody),
        (status = 429, description = "Rate-limit/minute ou quota mensuel du plan dépassé (`rate_limited`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn measurements(
    State(state): State<AppState>,
    auth: ApiKeyAuth,
    ValidatedQuery(q): ValidatedQuery<MeasurementsQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    Ok(Json(
        crate::routes::measurements::measurements_page_for_org(&state, auth.org_id, &q).await?,
    ))
}
