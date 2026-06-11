//! CRUD `/api/tracked-location-profiles` (B9a-2) — association n-aire **lieu suivi ×
//! profil d'exposition** + plage horaire (US-04).
//!
//! Une association applique un profil (système ou de l'org) à un lieu suivi de l'org,
//! sur une **plage horaire récurrente** (`start_time`–`end_time` locale, `days_mask`
//! = bitmask bit0=Lundi … bit6=Dimanche, `timezone`). C'est elle que le calcul de
//! dose (B9a-3) interrogera.
//!
//! Conventions (gabarit `alert_rules`) :
//! - **Isolation multi-tenant** : un `tracked_location_profile` appartient à l'org du
//!   **lieu** qu'il porte ; CHAQUE requête est filtrée « le lieu appartient à l'org du
//!   JWT » (sous-requête `EXISTS`) — une association d'un lieu étranger est invisible
//!   ⇒ **404** (anti-énumération ; 403 réservé au RÔLE).
//! - **Mutations** : extracteur [`CanWrite`].
//! - `tracked_location_id` et `exposure_profile_id` sont **IMMUABLES** après création
//!   (re-créer pour changer d'association) ; le PATCH ne touche que la plage / l'état.
//! - **Une seule plage** par couple (lieu, profil) — limite Jalon 1 assumée
//!   (`uq_tlp`) ⇒ **409**. Plage invalide (`end_time <= start_time`, `days_mask` hors
//!   1..=127) refusée par les CHECK du schéma ⇒ **422**.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use validator::Validate;

use crate::error::AppError;
use crate::listing::ListParams;
use crate::security::{AuthUser, CanWrite};
use crate::state::AppState;
use crate::validation::{ValidatedJson, ValidatedQuery};

// ─────────────────────────────────────────────────────────────────────────────
// Validateurs locaux
// ─────────────────────────────────────────────────────────────────────────────

/// `HH:MM` ou `HH:MM:SS` (00–23 / 00–59) — la borne `end > start` est vérifiée en base.
fn validate_time_hm(s: &str) -> Result<(), validator::ValidationError> {
    let err = || {
        validator::ValidationError::new("time")
            .with_message("heure attendue au format HH:MM ou HH:MM:SS".into())
    };
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 && parts.len() != 3 {
        return Err(err());
    }
    let h = parts[0].parse::<u32>().map_err(|_| err())?;
    let m = parts[1].parse::<u32>().map_err(|_| err())?;
    let sec = match parts.get(2) {
        Some(s) => s.parse::<u32>().map_err(|_| err())?,
        None => 0,
    };
    if h <= 23 && m <= 59 && sec <= 59 {
        Ok(())
    } else {
        Err(err())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DTOs
// ─────────────────────────────────────────────────────────────────────────────

/// Association lieu × profil (élément de listing et de détail).
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct TrackedLocationProfileDto {
    pub id: i64,
    pub tracked_location_id: i64,
    /// Nom du lieu porteur (sous-requête de confort).
    #[schema(example = "École Jules-Ferry")]
    pub tracked_location_name: String,
    pub exposure_profile_id: i64,
    /// Code du profil appliqué.
    #[schema(example = "enfants")]
    pub exposure_profile_code: String,
    #[schema(example = "Enfants")]
    pub exposure_profile_name: String,
    /// Début de la plage locale (`HH:MM:SS`).
    #[schema(example = "08:00:00")]
    pub start_time: String,
    /// Fin de la plage locale (`HH:MM:SS`, strictement > `start_time`).
    #[schema(example = "17:00:00")]
    pub end_time: String,
    /// Bitmask des jours : bit0=Lundi … bit6=Dimanche (1..=127). Ex. 31 = Lun–Ven.
    #[schema(example = 31)]
    pub days_mask: i16,
    #[schema(example = "Europe/Paris")]
    pub timezone: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Page d'associations (`{ page, page_size, count, total, data }`).
#[derive(Debug, Serialize, ToSchema)]
pub struct TrackedLocationProfilesPage {
    pub page: u32,
    pub page_size: u32,
    pub count: usize,
    pub total: i64,
    pub data: Vec<TrackedLocationProfileDto>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Requêtes entrantes
// ─────────────────────────────────────────────────────────────────────────────

/// Filtres propres au listing (s'ajoutent à [`ListParams`]).
#[derive(Debug, Deserialize, IntoParams, Validate)]
#[into_params(parameter_in = Query)]
pub struct TlpFilters {
    /// Restreindre à un lieu suivi (de l'org du JWT).
    pub tracked_location_id: Option<i64>,
    /// Restreindre à un profil d'exposition.
    pub exposure_profile_id: Option<i64>,
    /// Restreindre aux associations actives / inactives.
    pub is_active: Option<bool>,
}

/// Corps de `POST /api/tracked-location-profiles`.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct CreateTlpRequest {
    /// Lieu suivi — DOIT appartenir à l'org du JWT (404 sinon).
    pub tracked_location_id: i64,
    /// Profil appliqué — système (partagé) ou de l'org ; doit être VISIBLE (404 sinon).
    pub exposure_profile_id: i64,
    /// Début de plage locale `HH:MM[:SS]`.
    #[validate(custom(function = "validate_time_hm"))]
    #[schema(example = "08:00")]
    pub start_time: String,
    /// Fin de plage locale `HH:MM[:SS]` (strictement > `start_time` — CHECK en base).
    #[validate(custom(function = "validate_time_hm"))]
    #[schema(example = "17:00")]
    pub end_time: String,
    /// Bitmask des jours (1..=127 ; bit0=Lundi … bit6=Dimanche). Ex. 31 = Lun–Ven.
    #[validate(range(min = 1, max = 127, message = "days_mask attendu 1..=127"))]
    #[schema(example = 31)]
    pub days_mask: i16,
    /// Fuseau (défaut `Europe/Paris`).
    #[validate(
        length(min = 1, max = 64, message = "longueur attendue 1..=64"),
        custom(function = "crate::validation::validate_not_blank")
    )]
    pub timezone: Option<String>,
    /// Active à la création (défaut `true`).
    pub is_active: Option<bool>,
}

/// Corps de `PATCH /api/tracked-location-profiles/{id}` — les FK (`tracked_location_id`,
/// `exposure_profile_id`) sont IMMUABLES (absentes : serde ne les désérialise pas).
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct UpdateTlpRequest {
    #[validate(custom(function = "validate_time_hm"))]
    pub start_time: Option<String>,
    #[validate(custom(function = "validate_time_hm"))]
    pub end_time: Option<String>,
    #[validate(range(min = 1, max = 127, message = "days_mask attendu 1..=127"))]
    pub days_mask: Option<i16>,
    #[validate(
        length(min = 1, max = 64, message = "longueur attendue 1..=64"),
        custom(function = "crate::validation::validate_not_blank")
    )]
    pub timezone: Option<String>,
    pub is_active: Option<bool>,
}

// ─────────────────────────────────────────────────────────────────────────────
// SQL (scopé : le lieu porteur appartient à l'org du JWT)
// ─────────────────────────────────────────────────────────────────────────────

const DTO_COLUMNS: &str = r#"
    id, tracked_location_id,
    (SELECT tl.name FROM tracked_locations tl
      WHERE tl.id = tracked_location_profiles.tracked_location_id)         AS tracked_location_name,
    exposure_profile_id,
    (SELECT ep.code FROM exposure_profiles ep
      WHERE ep.id = tracked_location_profiles.exposure_profile_id)         AS exposure_profile_code,
    (SELECT ep.name FROM exposure_profiles ep
      WHERE ep.id = tracked_location_profiles.exposure_profile_id)         AS exposure_profile_name,
    start_time::text AS start_time, end_time::text AS end_time,
    days_mask, timezone, is_active, created_at, updated_at
"#;

/// Prédicat d'isolation : le lieu porteur appartient à l'org du JWT.
const OWNS_LOCATION: &str = "EXISTS(SELECT 1 FROM tracked_locations tl \
     WHERE tl.id = tracked_location_profiles.tracked_location_id AND tl.org_id = $2)";

const SORT_ALLOW: &[(&str, &str)] = &[
    ("created_at", "created_at"),
    ("updated_at", "updated_at"),
    ("start_time", "start_time"),
    ("days_mask", "days_mask"),
];

/// Une association de l'org, par id — `None` = inexistante OU lieu d'une autre org (404).
async fn fetch_one_dto(
    conn: &mut sqlx::PgConnection,
    org_id: i64,
    id: i64,
) -> Result<Option<TrackedLocationProfileDto>, sqlx::Error> {
    let sql = format!(
        "SELECT {DTO_COLUMNS} FROM tracked_location_profiles WHERE id = $1 AND {OWNS_LOCATION}"
    );
    sqlx::query_as::<_, TrackedLocationProfileDto>(&sql)
        .bind(id)
        .bind(org_id)
        .fetch_optional(conn)
        .await
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// Liste paginée des associations lieu × profil de l'org de l'appelant.
#[utoipa::path(
    get,
    path = "/api/tracked-location-profiles",
    tag = "exposure-profiles",
    params(ListParams, TlpFilters),
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Page d'associations (tri défaut `created_at` décroissant)", body = TrackedLocationProfilesPage),
        (status = 400, description = "Pagination/tri/filtre invalide", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
    ValidatedQuery(filters): ValidatedQuery<TlpFilters>,
) -> Result<Json<TrackedLocationProfilesPage>, AppError> {
    let p = params.pagination();
    let order_by = params.order_by(SORT_ALLOW, "-created_at")?;
    let like = params.like_pattern();

    // Isolation inline ($1 = org) ; `q` cherche dans le nom du lieu porteur.
    let where_clause = r#"
        WHERE EXISTS(SELECT 1 FROM tracked_locations tl
                     WHERE tl.id = tracked_location_profiles.tracked_location_id AND tl.org_id = $1)
          AND ($2::text IS NULL OR (SELECT tl.name FROM tracked_locations tl
                WHERE tl.id = tracked_location_profiles.tracked_location_id) ILIKE $2)
          AND ($3::bigint IS NULL OR tracked_location_id = $3)
          AND ($4::bigint IS NULL OR exposure_profile_id = $4)
          AND ($5::boolean IS NULL OR is_active = $5)
    "#;

    let total: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM tracked_location_profiles {where_clause}"
    ))
    .bind(user.org_id)
    .bind(like.as_deref())
    .bind(filters.tracked_location_id)
    .bind(filters.exposure_profile_id)
    .bind(filters.is_active)
    .fetch_one(&state.pg)
    .await?;

    let data = sqlx::query_as::<_, TrackedLocationProfileDto>(&format!(
        "SELECT {DTO_COLUMNS} FROM tracked_location_profiles {where_clause} {order_by} LIMIT $6 OFFSET $7"
    ))
    .bind(user.org_id)
    .bind(like.as_deref())
    .bind(filters.tracked_location_id)
    .bind(filters.exposure_profile_id)
    .bind(filters.is_active)
    .bind(p.limit)
    .bind(p.offset)
    .fetch_all(&state.pg)
    .await?;

    Ok(Json(TrackedLocationProfilesPage {
        page: p.page,
        page_size: p.page_size,
        count: data.len(),
        total,
        data,
    }))
}

/// Applique un profil à un lieu suivi de l'org (avec plage horaire).
#[utoipa::path(
    post,
    path = "/api/tracked-location-profiles",
    tag = "exposure-profiles",
    request_body = CreateTlpRequest,
    security(("bearer_jwt" = [])),
    responses(
        (status = 201, description = "Association créée", body = TrackedLocationProfileDto),
        (status = 400, description = "Validation du corps échouée", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle sans droit d'écriture (`read_only_role`)", body = crate::openapi::ErrorBody),
        (status = 404, description = "Lieu ou profil inconnu / non visible (`tracked_location_not_found` ou `exposure_profile_not_found`)", body = crate::openapi::ErrorBody),
        (status = 409, description = "Ce profil est déjà appliqué à ce lieu (`conflict`)", body = crate::openapi::ErrorBody),
        (status = 422, description = "Plage invalide (`end_time` ≤ `start_time`, ou `days_mask` hors 1..=127)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    CanWrite(user): CanWrite,
    ValidatedJson(req): ValidatedJson<CreateTlpRequest>,
) -> Result<(StatusCode, Json<TrackedLocationProfileDto>), AppError> {
    // Le lieu doit appartenir à l'org (404 sinon : invisible).
    let owns_loc: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM tracked_locations WHERE id = $1 AND org_id = $2)",
    )
    .bind(req.tracked_location_id)
    .bind(user.org_id)
    .fetch_one(&state.pg)
    .await?;
    if !owns_loc {
        return Err(AppError::NotFound("tracked_location_not_found"));
    }

    // Le profil doit être VISIBLE (système ou de l'org) — on n'applique pas un profil
    // d'une autre org (404 sinon).
    let sees_profile: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM exposure_profiles WHERE id = $1 AND (org_id = $2 OR org_id IS NULL))",
    )
    .bind(req.exposure_profile_id)
    .bind(user.org_id)
    .fetch_one(&state.pg)
    .await?;
    if !sees_profile {
        return Err(AppError::NotFound("exposure_profile_not_found"));
    }

    // 409 auto (uq_tlp) ; 422 auto sur les CHECK (end > start, days_mask 1..=127).
    let new_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO tracked_location_profiles
            (tracked_location_id, exposure_profile_id, start_time, end_time,
             days_mask, timezone, is_active)
        VALUES ($1, $2, $3::time, $4::time, $5, COALESCE($6, 'Europe/Paris'), COALESCE($7, true))
        RETURNING id
        "#,
    )
    .bind(req.tracked_location_id)
    .bind(req.exposure_profile_id)
    .bind(&req.start_time)
    .bind(&req.end_time)
    .bind(req.days_mask)
    .bind(req.timezone.as_deref().map(str::trim))
    .bind(req.is_active)
    .fetch_one(&state.pg)
    .await?;

    let mut conn = state.pg.acquire().await?;
    let dto = fetch_one_dto(&mut conn, user.org_id, new_id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("association créée introuvable")))?;
    Ok((StatusCode::CREATED, Json(dto)))
}

/// Détail d'une association de l'org.
#[utoipa::path(
    get,
    path = "/api/tracked-location-profiles/{id}",
    tag = "exposure-profiles",
    params(("id" = i64, Path, description = "Identifiant de l'association")),
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Détail de l'association", body = TrackedLocationProfileDto),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu ou lieu d'une autre org (`tracked_location_profile_not_found`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<TrackedLocationProfileDto>, AppError> {
    let mut conn = state.pg.acquire().await?;
    let dto = fetch_one_dto(&mut conn, user.org_id, id)
        .await?
        .ok_or(AppError::NotFound("tracked_location_profile_not_found"))?;
    Ok(Json(dto))
}

/// Modifie la plage horaire / l'état d'une association (les FK sont immuables).
#[utoipa::path(
    patch,
    path = "/api/tracked-location-profiles/{id}",
    tag = "exposure-profiles",
    params(("id" = i64, Path, description = "Identifiant de l'association")),
    request_body = UpdateTlpRequest,
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Association modifiée", body = TrackedLocationProfileDto),
        (status = 400, description = "Validation échouée ou aucun champ fourni", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle sans droit d'écriture (`read_only_role`)", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu ou lieu d'une autre org (`tracked_location_profile_not_found`)", body = crate::openapi::ErrorBody),
        (status = 422, description = "Plage résultante invalide (`end_time` ≤ `start_time`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    CanWrite(user): CanWrite,
    Path(id): Path<i64>,
    ValidatedJson(req): ValidatedJson<UpdateTlpRequest>,
) -> Result<Json<TrackedLocationProfileDto>, AppError> {
    if req.start_time.is_none()
        && req.end_time.is_none()
        && req.days_mask.is_none()
        && req.timezone.is_none()
        && req.is_active.is_none()
    {
        return Err(AppError::BadRequest(
            "aucun champ à modifier (attendus : start_time, end_time, days_mask, timezone, \
             is_active — le lieu et le profil sont immuables : re-créer une association)"
                .into(),
        ));
    }

    let updated = sqlx::query(&format!(
        r#"
        UPDATE tracked_location_profiles SET
            start_time = COALESCE($3::time, start_time),
            end_time   = COALESCE($4::time, end_time),
            days_mask  = COALESCE($5, days_mask),
            timezone   = COALESCE($6, timezone),
            is_active  = COALESCE($7, is_active),
            updated_at = now()
        WHERE id = $1 AND {OWNS_LOCATION}
        "#
    ))
    .bind(id)
    .bind(user.org_id)
    .bind(req.start_time.as_deref())
    .bind(req.end_time.as_deref())
    .bind(req.days_mask)
    .bind(req.timezone.as_deref().map(str::trim))
    .bind(req.is_active)
    .execute(&state.pg)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound("tracked_location_profile_not_found"));
    }

    let mut conn = state.pg.acquire().await?;
    let dto = fetch_one_dto(&mut conn, user.org_id, id)
        .await?
        .ok_or(AppError::NotFound("tracked_location_profile_not_found"))?;
    Ok(Json(dto))
}

/// Supprime une association (les doses en cache `exposure_results` suivent — FK CASCADE).
#[utoipa::path(
    delete,
    path = "/api/tracked-location-profiles/{id}",
    tag = "exposure-profiles",
    params(("id" = i64, Path, description = "Identifiant de l'association")),
    security(("bearer_jwt" = [])),
    responses(
        (status = 204, description = "Association supprimée"),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle sans droit d'écriture (`read_only_role`)", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu ou lieu d'une autre org (`tracked_location_profile_not_found`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete(
    State(state): State<AppState>,
    CanWrite(user): CanWrite,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let deleted = sqlx::query(&format!(
        "DELETE FROM tracked_location_profiles WHERE id = $1 AND {OWNS_LOCATION}"
    ))
    .bind(id)
    .bind(user.org_id)
    .execute(&state.pg)
    .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound("tracked_location_profile_not_found"));
    }
    Ok(StatusCode::NO_CONTENT)
}
