//! Calcul de dose d'exposition (B9a-3) — sous-ressources d'un `tracked_location_profile` :
//! - `POST /api/tracked-location-profiles/{id}/compute-dose` : pour CHAQUE seuil 1h/8h/24h
//!   du profil (annual ignoré ici), calcule les heures de dépassement sur la période via
//!   ClickHouse (moyenne glissante MAX multi-stations dans la plage horaire locale), puis
//!   fige le résultat en base via la procédure `compute_exposure_dose` (P3, US-08) ;
//! - `GET …/{id}/results` : relit le cache `exposure_results` du profil.
//!
//! Isolation : l'association doit appartenir à l'org du JWT (via son lieu) — 404 sinon.
//! Le calcul (lecture ClickHouse) est réservé au rôle écriture (il écrit le cache via P3).

use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use futures_util::stream::StreamExt;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use validator::Validate;

use crate::ch::DoseParams;
use crate::error::AppError;
use crate::security::{AuthUser, CanWrite};
use crate::state::AppState;
use crate::validation::ValidatedQuery;

/// Concurrence MAX des requêtes ClickHouse de dose (P3) : un profil à plusieurs seuils
/// (1h/8h/24h × polluants) ne lance pas toutes ses requêtes en série — au plus ce
/// nombre en vol. Les seuils par profil sont peu nombreux : 8 suffit largement.
const DOSE_MAX_CONCURRENT_CH_QUERIES: usize = 8;

/// Durée maximale d'une période de calcul de dose (A3, défense en profondeur).
const DOSE_MAX_PERIOD_DAYS: i64 = 366;

/// Valide une date `YYYY-MM-DD` (rejette aussi les dates impossibles, ex. 2026-13-40).
fn validate_date_ymd(s: &str) -> Result<(), validator::ValidationError> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| {
            validator::ValidationError::new("date")
                .with_message("date attendue au format YYYY-MM-DD".into())
        })
}

/// `1h`/`8h`/`24h` → heures de moyennage ; `annual` (ou inconnu) → `None` (ignoré en B9a-3).
fn avg_hours(period: &str) -> Option<u32> {
    match period {
        "1h" => Some(1),
        "8h" => Some(8),
        "24h" => Some(24),
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DTO
// ─────────────────────────────────────────────────────────────────────────────

/// Dose calculée et figée (`exposure_results`). Fenêtre snapshot incluse (US-08 c3).
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct ExposureResultDto {
    pub id: i64,
    pub tracked_location_profile_id: i64,
    #[schema(example = "pm25")]
    pub parameter: String,
    /// Fenêtre de moyennage du seuil (`1h`/`8h`/`24h`) — discrimine deux doses du
    /// même polluant sur des fenêtres différentes.
    #[schema(example = "24h")]
    pub averaging_period: String,
    /// Seuil appliqué (figé au calcul).
    #[schema(example = 15.0)]
    pub threshold_value: f64,
    #[schema(example = "2026-05-01")]
    pub period_start: String,
    #[schema(example = "2026-05-31")]
    pub period_end: String,
    /// Plage horaire snapshot (reproductibilité).
    pub window_start_time: String,
    pub window_end_time: String,
    pub days_mask: i16,
    pub timezone: String,
    /// Heures de dépassement sur la période (dans la plage).
    #[schema(example = 12.0)]
    pub hours_over_threshold: f64,
    /// Heures évaluées (couverture).
    pub sample_count: i32,
    pub computed_at: DateTime<Utc>,
}

const RESULT_COLUMNS: &str = r#"
    id, tracked_location_profile_id,
    (SELECT p.code FROM parameters p WHERE p.id = exposure_results.parameter_id) AS parameter,
    averaging_period,
    threshold_value::float8 AS threshold_value,
    period_start::text AS period_start, period_end::text AS period_end,
    window_start_time::text AS window_start_time, window_end_time::text AS window_end_time,
    window_days_mask AS days_mask, timezone,
    hours_over_threshold::float8 AS hours_over_threshold, sample_count, computed_at
"#;

/// `tracked_location_profile` chargé pour le calcul (scopé org via son lieu).
#[derive(sqlx::FromRow)]
struct TlpRow {
    tracked_location_id: i64,
    exposure_profile_id: i64,
    /// Plage horaire en minutes du jour (dérivée du TIME).
    start_min: i32,
    end_min: i32,
    days_mask: i16,
    timezone: String,
}

/// Seuil du profil à évaluer.
#[derive(sqlx::FromRow)]
struct ThresholdRow {
    parameter: String,
    threshold_value: f64,
    averaging_period: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Requête entrante
// ─────────────────────────────────────────────────────────────────────────────

/// Query string de `compute-dose`.
#[derive(Debug, Deserialize, IntoParams, Validate)]
#[into_params(parameter_in = Query)]
pub struct ComputeDoseQuery {
    /// Début de période (date locale `YYYY-MM-DD`, incluse).
    #[validate(custom(function = "validate_date_ymd"))]
    #[param(example = "2026-05-01")]
    pub period_start: String,
    /// Fin de période (date locale `YYYY-MM-DD`, incluse, ≥ `period_start`).
    #[validate(custom(function = "validate_date_ymd"))]
    #[param(example = "2026-05-31")]
    pub period_end: String,
}

async fn read_results(
    pg: &sqlx::PgPool,
    tlp_id: i64,
    from: &str,
    to: &str,
) -> Result<Vec<ExposureResultDto>, sqlx::Error> {
    sqlx::query_as::<_, ExposureResultDto>(&format!(
        "SELECT {RESULT_COLUMNS} FROM exposure_results \
         WHERE tracked_location_profile_id = $1 AND period_start = $2::date AND period_end = $3::date \
         ORDER BY parameter, averaging_period"
    ))
    .bind(tlp_id)
    .bind(from)
    .bind(to)
    .fetch_all(pg)
    .await
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// Calcule et fige la dose d'exposition de l'association pour la période, pour chaque
/// seuil 1h/8h/24h du profil (`annual` ignoré en B9a-3).
#[utoipa::path(
    post,
    path = "/api/tracked-location-profiles/{id}/compute-dose",
    tag = "exposure-profiles",
    params(("id" = i64, Path, description = "Identifiant de l'association"), ComputeDoseQuery),
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Doses calculées et figées (une par seuil 1h/8h/24h ; vide si le profil n'a aucun seuil)", body = [ExposureResultDto]),
        (status = 400, description = "Dates invalides ou `period_end` < `period_start`", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle sans droit d'écriture (`read_only_role`)", body = crate::openapi::ErrorBody),
        (status = 404, description = "Association inconnue ou d'une autre org (`tracked_location_profile_not_found`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn compute_dose(
    State(state): State<AppState>,
    CanWrite(user): CanWrite,
    Path(id): Path<i64>,
    ValidatedQuery(q): ValidatedQuery<ComputeDoseQuery>,
) -> Result<Json<Vec<ExposureResultDto>>, AppError> {
    // Dates déjà validées au format par `ValidatedQuery` : on les parse pour comparer
    // chronologiquement ET borner la durée (A3 — défense en profondeur : le TTL 90 j de
    // ClickHouse borne déjà le scan, mais on refuse une plage déraisonnable).
    let start = chrono::NaiveDate::parse_from_str(&q.period_start, "%Y-%m-%d")
        .map_err(|_| AppError::BadRequest("period_start invalide".into()))?;
    let end = chrono::NaiveDate::parse_from_str(&q.period_end, "%Y-%m-%d")
        .map_err(|_| AppError::BadRequest("period_end invalide".into()))?;
    if end < start {
        return Err(AppError::BadRequest(
            "period_end doit être >= period_start".into(),
        ));
    }
    if (end - start).num_days() > DOSE_MAX_PERIOD_DAYS {
        return Err(AppError::BadRequest(
            "période trop longue (max 366 jours)".into(),
        ));
    }

    // L'association doit appartenir à l'org (via son lieu) — 404 sinon. On en tire la
    // plage (en minutes), le masque jours, le fuseau et le lieu (pour ses stations).
    let tlp: Option<TlpRow> = sqlx::query_as(
        r#"
        SELECT tlp.tracked_location_id, tlp.exposure_profile_id,
               (EXTRACT(HOUR FROM tlp.start_time) * 60 + EXTRACT(MINUTE FROM tlp.start_time))::int AS start_min,
               (EXTRACT(HOUR FROM tlp.end_time)   * 60 + EXTRACT(MINUTE FROM tlp.end_time))::int   AS end_min,
               tlp.days_mask, tlp.timezone
        FROM tracked_location_profiles tlp
        WHERE tlp.id = $1
          AND EXISTS(SELECT 1 FROM tracked_locations tl
                      WHERE tl.id = tlp.tracked_location_id AND tl.org_id = $2)
        "#,
    )
    .bind(id)
    .bind(user.org_id)
    .fetch_optional(&state.pg)
    .await?;
    let tlp = tlp.ok_or(AppError::NotFound("tracked_location_profile_not_found"))?;

    // Stations du lieu (règle MAX multi-stations).
    let stations: Vec<i64> = sqlx::query_scalar(
        "SELECT rl.openaq_location_id FROM tracked_location_stations tls \
         JOIN ref_locations rl ON rl.id = tls.ref_location_id WHERE tls.tracked_location_id = $1",
    )
    .bind(tlp.tracked_location_id)
    .fetch_all(&state.pg)
    .await?;

    // Seuils du profil (l'unité reste portée par `parameters`).
    let thresholds: Vec<ThresholdRow> = sqlx::query_as(
        "SELECT (SELECT p.code FROM parameters p WHERE p.id = et.parameter_id) AS parameter, \
                et.threshold_value::float8 AS threshold_value, et.averaging_period \
         FROM exposure_thresholds et WHERE et.exposure_profile_id = $1",
    )
    .bind(tlp.exposure_profile_id)
    .fetch_all(&state.pg)
    .await?;

    // P3 : les requêtes ClickHouse de dose (une par seuil 1h/8h/24h) tournent en PARALLÈLE
    // bornée — l'ordre n'importe pas, chaque résultat est rattaché à son seuil par index.
    // `annual` (et inconnu) est ignoré (hors périmètre B9a-3).
    let jobs: Vec<(usize, u32)> = thresholds
        .iter()
        .enumerate()
        .filter_map(|(i, t)| avg_hours(&t.averaging_period).map(|h| (i, h)))
        .collect();
    let doses: Vec<_> = futures_util::stream::iter(jobs.into_iter().map(|(i, avg_h)| {
        let ch = state.ch.clone();
        let stations = stations.clone();
        let parameter = thresholds[i].parameter.clone();
        let threshold = thresholds[i].threshold_value;
        let from = q.period_start.clone();
        let to = q.period_end.clone();
        let tz = tlp.timezone.clone();
        let start_min = tlp.start_min as u16;
        let end_min = tlp.end_min as u16;
        let days_mask = tlp.days_mask;
        async move {
            let r = ch
                .query_exposure_dose(&DoseParams {
                    locs: &stations,
                    parameter: &parameter,
                    avg_hours: avg_h,
                    threshold,
                    from: &from,
                    to: &to,
                    start_min,
                    end_min,
                    days_mask,
                    tz: &tz,
                })
                .await;
            (i, r)
        }
    }))
    .buffer_unordered(DOSE_MAX_CONCURRENT_CH_QUERIES)
    .collect()
    .await;

    // Upsert PG (séquentiel : écriture rapide). La procédure fige seuil + fenêtre
    // (snapshot) et re-résout le seuil depuis (profil, polluant, période de moyennage).
    for (i, dose) in doses {
        let dose = dose.map_err(|e| AppError::Internal(e.into()))?;
        let t = &thresholds[i];
        let _result_id: i64 = sqlx::query_scalar(
            "CALL compute_exposure_dose($1, $2, $3::date, $4::date, $5::numeric, $6, $7, NULL)",
        )
        .bind(id)
        .bind(&t.parameter)
        .bind(&q.period_start)
        .bind(&q.period_end)
        .bind(dose.hours_over_threshold as i64)
        .bind(i32::try_from(dose.sample_count).unwrap_or(i32::MAX))
        .bind(&t.averaging_period)
        .fetch_one(&state.pg)
        .await?;
    }

    let results = read_results(&state.pg, id, &q.period_start, &q.period_end).await?;
    Ok(Json(results))
}

/// Doses en cache d'une association (tous calculs confondus), de la plus récente période
/// à la plus ancienne.
#[utoipa::path(
    get,
    path = "/api/tracked-location-profiles/{id}/results",
    tag = "exposure-profiles",
    params(("id" = i64, Path, description = "Identifiant de l'association")),
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Doses en cache de l'association", body = [ExposureResultDto]),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 404, description = "Association inconnue ou d'une autre org (`tracked_location_profile_not_found`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_results(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Vec<ExposureResultDto>>, AppError> {
    let owns: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM tracked_location_profiles tlp \
         WHERE tlp.id = $1 AND EXISTS(SELECT 1 FROM tracked_locations tl \
                WHERE tl.id = tlp.tracked_location_id AND tl.org_id = $2))",
    )
    .bind(id)
    .bind(user.org_id)
    .fetch_one(&state.pg)
    .await?;
    if !owns {
        return Err(AppError::NotFound("tracked_location_profile_not_found"));
    }

    let results = sqlx::query_as::<_, ExposureResultDto>(&format!(
        "SELECT {RESULT_COLUMNS} FROM exposure_results \
         WHERE tracked_location_profile_id = $1 ORDER BY period_end DESC, parameter, averaging_period"
    ))
    .bind(id)
    .fetch_all(&state.pg)
    .await?;
    Ok(Json(results))
}
