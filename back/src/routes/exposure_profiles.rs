//! CRUD `/api/exposure-profiles` + seuils adaptés (B9a) — profils de population
//! sensible (US-04) et leurs seuils par polluant.
//!
//! Mêmes conventions que le gabarit `alert_rules` :
//! - **Isolation multi-tenant** : un profil est soit **système** (`org_id IS NULL`,
//!   `is_system` — partagé, LECTURE seule pour tous), soit **propre à une org**
//!   (`org_id = <org du JWT>`). La LECTURE voit les deux (`org_id = $org OR org_id
//!   IS NULL`) ; toute MUTATION est scopée `org_id = <org du JWT>` — un profil
//!   système ou d'une autre org est donc invisible à l'écriture ⇒ **404**
//!   (anti-énumération ; le 403 reste réservé aux refus de RÔLE).
//! - **Mutations** : extracteur [`CanWrite`] (rôle `lecteur` ⇒ 403 `read_only_role`).
//! - **Listing** : pagination/tri/recherche via [`crate::listing`] (allowlist stricte).
//!
//! Pas d'audit T5 ici (aucun trigger d'audit sur les tables d'exposition). Les seuils
//! sont une **sous-ressource** d'un profil : `…/{id}/thresholds`. L'unité d'un seuil
//! n'est jamais dupliquée — elle est dérivée de `parameters.unit` (sous-requête).

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
// Validateurs locaux (miroirs des CHECK du schéma — surface partagée non élargie)
// ─────────────────────────────────────────────────────────────────────────────

/// Codes de profil — miroir du CHECK `exposure_profiles.code`.
fn validate_exposure_code(s: &str) -> Result<(), validator::ValidationError> {
    if matches!(
        s,
        "enfants" | "asthmatiques" | "personnes_agees" | "sportifs" | "general"
    ) {
        return Ok(());
    }
    Err(validator::ValidationError::new("code").with_message(
        "valeurs acceptées : enfants, asthmatiques, personnes_agees, sportifs, general".into(),
    ))
}

/// Périodes de moyennage — miroir du CHECK `exposure_thresholds.averaging_period`.
fn validate_averaging_period(s: &str) -> Result<(), validator::ValidationError> {
    if matches!(s, "1h" | "8h" | "24h" | "annual") {
        return Ok(());
    }
    Err(validator::ValidationError::new("averaging_period")
        .with_message("valeurs acceptées : 1h, 8h, 24h, annual".into()))
}

// ─────────────────────────────────────────────────────────────────────────────
// DTOs
// ─────────────────────────────────────────────────────────────────────────────

/// Profil d'exposition (élément de listing et de détail).
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct ExposureProfileDto {
    pub id: i64,
    /// Org propriétaire — `null` pour un profil **système** (partagé, lecture seule).
    pub org_id: Option<i64>,
    /// `enfants` / `asthmatiques` / `personnes_agees` / `sportifs` / `general`.
    #[schema(example = "enfants")]
    pub code: String,
    #[schema(example = "Enfants")]
    pub name: String,
    pub description: Option<String>,
    /// `true` = profil système (préfabriqué, non modifiable par une org).
    pub is_system: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Page de profils (`{ page, page_size, count, total, data }`).
#[derive(Debug, Serialize, ToSchema)]
pub struct ExposureProfilesPage {
    pub page: u32,
    pub page_size: u32,
    pub count: usize,
    pub total: i64,
    pub data: Vec<ExposureProfileDto>,
}

/// Seuil adapté d'un profil pour un polluant (unité dérivée de `parameters`).
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct ExposureThresholdDto {
    pub id: i64,
    pub exposure_profile_id: i64,
    /// Code du polluant (`pm25`, `pm10`, `no2`, `o3`, `so2`, `co`).
    #[schema(example = "pm25")]
    pub parameter: String,
    /// Unité canonique du polluant — DÉRIVÉE de `parameters.unit` (jamais dupliquée).
    #[schema(example = "µg/m³")]
    pub unit: String,
    /// Seuil ≥ 0 dans l'unité ci-dessus (`::float8` : NUMERIC → f64).
    #[schema(example = 15.0)]
    pub threshold_value: f64,
    /// `1h` / `8h` / `24h` / `annual` (période de moyennage réglementaire).
    #[schema(example = "1h")]
    pub averaging_period: String,
    pub created_at: DateTime<Utc>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Requêtes entrantes
// ─────────────────────────────────────────────────────────────────────────────

/// Filtres propres au listing des profils (s'ajoutent à [`ListParams`]).
#[derive(Debug, Deserialize, IntoParams, Validate)]
#[into_params(parameter_in = Query)]
pub struct ExposureProfileFilters {
    /// `system` (profils partagés) ou `custom` (profils de l'org).
    #[validate(custom(function = "validate_scope"))]
    #[param(example = "custom")]
    pub scope: Option<String>,
    /// Code de profil (allowlist : enfants, asthmatiques, personnes_agees, sportifs, general).
    #[validate(custom(function = "validate_exposure_code"))]
    pub code: Option<String>,
}

fn validate_scope(s: &str) -> Result<(), validator::ValidationError> {
    if matches!(s, "system" | "custom") {
        return Ok(());
    }
    Err(validator::ValidationError::new("scope")
        .with_message("valeurs acceptées : system, custom".into()))
}

/// Corps de `POST /api/exposure-profiles` — crée un profil CUSTOM de l'org.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct CreateExposureProfileRequest {
    /// Code de profil (allowlist). Un seul profil par (org, code) — 409 sinon.
    #[validate(custom(function = "validate_exposure_code"))]
    #[schema(example = "sportifs")]
    pub code: String,
    /// Libellé (1..=200, non blanc).
    #[validate(
        length(min = 1, max = 200, message = "longueur attendue 1..=200"),
        custom(function = "crate::validation::validate_not_blank")
    )]
    #[schema(example = "Sportifs amateurs")]
    pub name: String,
    /// Description optionnelle (≤ 1000).
    #[validate(length(max = 1000, message = "1000 caractères maximum"))]
    pub description: Option<String>,
}

/// Corps de `PATCH /api/exposure-profiles/{id}` — `code` est IMMUABLE (absent ici).
/// `description: ""` efface la description.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct UpdateExposureProfileRequest {
    #[validate(
        length(min = 1, max = 200, message = "longueur attendue 1..=200"),
        custom(function = "crate::validation::validate_not_blank")
    )]
    pub name: Option<String>,
    /// `""` est légitime (effacement) — pas de `min`.
    #[validate(length(max = 1000, message = "1000 caractères maximum"))]
    pub description: Option<String>,
}

/// Corps de `POST /api/exposure-profiles/{id}/thresholds`.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct CreateExposureThresholdRequest {
    /// Polluant (allowlist : pm25, pm10, no2, o3, so2, co).
    #[validate(custom(function = "crate::validation::validate_parameter_code"))]
    #[schema(example = "pm25")]
    pub parameter: String,
    /// Seuil ≥ 0 (arrondi silencieux à 4 décimales — `NUMERIC(12,4)`).
    #[validate(range(min = 0.0, max = 99999999.0, message = "seuil attendu 0..=99999999"))]
    #[schema(example = 15.0)]
    pub threshold_value: f64,
    /// Période de moyennage — défaut `1h`. Un seul seuil par (profil, polluant, période).
    #[validate(custom(function = "validate_averaging_period"))]
    pub averaging_period: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// SQL
// ─────────────────────────────────────────────────────────────────────────────

const PROFILE_COLUMNS: &str =
    "id, org_id, code, name, description, is_system, created_at, updated_at";

/// Allowlist de tri des profils.
const SORT_ALLOW: &[(&str, &str)] = &[
    ("created_at", "created_at"),
    ("updated_at", "updated_at"),
    ("name", "name"),
    ("code", "code"),
];

const THRESHOLD_COLUMNS: &str = r#"
    id, exposure_profile_id,
    (SELECT p.code FROM parameters p WHERE p.id = exposure_thresholds.parameter_id) AS parameter,
    (SELECT p.unit FROM parameters p WHERE p.id = exposure_thresholds.parameter_id) AS unit,
    threshold_value::float8 AS threshold_value,
    averaging_period, created_at
"#;

/// Un profil VISIBLE par l'org (système OU propre) — `None` ⇒ 404.
async fn fetch_visible_profile(
    conn: &mut sqlx::PgConnection,
    org_id: i64,
    id: i64,
) -> Result<Option<ExposureProfileDto>, sqlx::Error> {
    let sql = format!(
        "SELECT {PROFILE_COLUMNS} FROM exposure_profiles \
         WHERE id = $1 AND (org_id = $2 OR org_id IS NULL)"
    );
    sqlx::query_as::<_, ExposureProfileDto>(&sql)
        .bind(id)
        .bind(org_id)
        .fetch_optional(conn)
        .await
}

/// `true` si le profil `id` est PROPRE à l'org (modifiable). Un profil système ou
/// d'une autre org renvoie `false` ⇒ l'appelant répond 404 (anti-énumération).
async fn owns_profile(pg: &sqlx::PgPool, org_id: i64, id: i64) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM exposure_profiles WHERE id = $1 AND org_id = $2)",
    )
    .bind(id)
    .bind(org_id)
    .fetch_one(pg)
    .await
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers — profils
// ─────────────────────────────────────────────────────────────────────────────

/// Liste paginée des profils d'exposition visibles (système + ceux de l'org).
#[utoipa::path(
    get,
    path = "/api/exposure-profiles",
    tag = "exposure-profiles",
    params(ListParams, ExposureProfileFilters),
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Page de profils (système + org ; tri défaut `created_at` décroissant)", body = ExposureProfilesPage),
        (status = 400, description = "Pagination/tri/filtre invalide", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
    ValidatedQuery(filters): ValidatedQuery<ExposureProfileFilters>,
) -> Result<Json<ExposureProfilesPage>, AppError> {
    let p = params.pagination();
    let order_by = params.order_by(SORT_ALLOW, "-created_at")?;
    let like = params.like_pattern();

    // Visibilité = système (org_id NULL) OU org du JWT. `scope` restreint à l'un
    // des deux ; `code`/`q` filtrent. Une seule requête préparée (filtres NULL neutres).
    let where_clause = r#"
        WHERE (org_id = $1 OR org_id IS NULL)
          AND ($2::text IS NULL OR name ILIKE $2 OR code ILIKE $2)
          AND ($3::text IS NULL OR code = $3)
          AND ($4::text IS NULL
               OR ($4 = 'system' AND org_id IS NULL)
               OR ($4 = 'custom' AND org_id = $1))
    "#;

    let total: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM exposure_profiles {where_clause}"
    ))
    .bind(user.org_id)
    .bind(like.as_deref())
    .bind(filters.code.as_deref())
    .bind(filters.scope.as_deref())
    .fetch_one(&state.pg)
    .await?;

    let data = sqlx::query_as::<_, ExposureProfileDto>(&format!(
        "SELECT {PROFILE_COLUMNS} FROM exposure_profiles {where_clause} {order_by} LIMIT $5 OFFSET $6"
    ))
    .bind(user.org_id)
    .bind(like.as_deref())
    .bind(filters.code.as_deref())
    .bind(filters.scope.as_deref())
    .bind(p.limit)
    .bind(p.offset)
    .fetch_all(&state.pg)
    .await?;

    Ok(Json(ExposureProfilesPage {
        page: p.page,
        page_size: p.page_size,
        count: data.len(),
        total,
        data,
    }))
}

/// Crée un profil d'exposition CUSTOM pour l'org de l'appelant.
#[utoipa::path(
    post,
    path = "/api/exposure-profiles",
    tag = "exposure-profiles",
    request_body = CreateExposureProfileRequest,
    security(("bearer_jwt" = [])),
    responses(
        (status = 201, description = "Profil créé (propre à l'org, `is_system=false`)", body = ExposureProfileDto),
        (status = 400, description = "Validation du corps échouée", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle sans droit d'écriture (`read_only_role`)", body = crate::openapi::ErrorBody),
        (status = 409, description = "Un profil de même code existe déjà pour cette org (`conflict`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    CanWrite(user): CanWrite,
    ValidatedJson(req): ValidatedJson<CreateExposureProfileRequest>,
) -> Result<(StatusCode, Json<ExposureProfileDto>), AppError> {
    // org_id = JWT (custom) ; is_system reste false (défaut). 409 auto sur (org, code).
    let new_id: i64 = sqlx::query_scalar(
        "INSERT INTO exposure_profiles (org_id, code, name, description) \
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(user.org_id)
    .bind(&req.code)
    .bind(req.name.trim())
    .bind(req.description.as_deref().map(str::trim))
    .fetch_one(&state.pg)
    .await?;

    let mut conn = state.pg.acquire().await?;
    let dto = fetch_visible_profile(&mut conn, user.org_id, new_id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("profil créé introuvable")))?;
    Ok((StatusCode::CREATED, Json(dto)))
}

/// Détail d'un profil visible (système ou de l'org).
#[utoipa::path(
    get,
    path = "/api/exposure-profiles/{id}",
    tag = "exposure-profiles",
    params(("id" = i64, Path, description = "Identifiant du profil")),
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Détail du profil", body = ExposureProfileDto),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu ou profil d'une autre org (`exposure_profile_not_found`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<ExposureProfileDto>, AppError> {
    let mut conn = state.pg.acquire().await?;
    let dto = fetch_visible_profile(&mut conn, user.org_id, id)
        .await?
        .ok_or(AppError::NotFound("exposure_profile_not_found"))?;
    Ok(Json(dto))
}

/// Modifie le libellé / la description d'un profil PROPRE à l'org (les profils
/// système sont en lecture seule ⇒ 404 ; `code` est immuable).
#[utoipa::path(
    patch,
    path = "/api/exposure-profiles/{id}",
    tag = "exposure-profiles",
    params(("id" = i64, Path, description = "Identifiant du profil")),
    request_body = UpdateExposureProfileRequest,
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Profil modifié", body = ExposureProfileDto),
        (status = 400, description = "Validation échouée ou aucun champ fourni", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle sans droit d'écriture (`read_only_role`)", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu, profil système ou d'une autre org (`exposure_profile_not_found`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    CanWrite(user): CanWrite,
    Path(id): Path<i64>,
    ValidatedJson(req): ValidatedJson<UpdateExposureProfileRequest>,
) -> Result<Json<ExposureProfileDto>, AppError> {
    if req.name.is_none() && req.description.is_none() {
        return Err(AppError::BadRequest(
            "aucun champ à modifier (attendus : name, description — code est immuable)".into(),
        ));
    }

    // Scopé `org_id = $2` : un profil système (org_id NULL) ou d'une autre org n'est
    // jamais touché ⇒ 0 ligne ⇒ 404.
    let updated = sqlx::query(
        r#"
        UPDATE exposure_profiles SET
            name        = COALESCE($3, name),
            description  = CASE WHEN $4::text IS NULL THEN description
                                ELSE NULLIF(trim($4), '') END,
            updated_at  = now()
        WHERE id = $1 AND org_id = $2
        "#,
    )
    .bind(id)
    .bind(user.org_id)
    .bind(req.name.as_deref().map(str::trim))
    .bind(req.description.as_deref())
    .execute(&state.pg)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound("exposure_profile_not_found"));
    }

    let mut conn = state.pg.acquire().await?;
    let dto = fetch_visible_profile(&mut conn, user.org_id, id)
        .await?
        .ok_or(AppError::NotFound("exposure_profile_not_found"))?;
    Ok(Json(dto))
}

/// Supprime un profil PROPRE à l'org (système ⇒ 404). Les seuils suivent (FK CASCADE) ;
/// un profil encore RÉFÉRENCÉ par une association lieu×profil (FK RESTRICT) ⇒ 422.
#[utoipa::path(
    delete,
    path = "/api/exposure-profiles/{id}",
    tag = "exposure-profiles",
    params(("id" = i64, Path, description = "Identifiant du profil")),
    security(("bearer_jwt" = [])),
    responses(
        (status = 204, description = "Profil supprimé (ses seuils aussi)"),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle sans droit d'écriture (`read_only_role`)", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu, profil système ou d'une autre org (`exposure_profile_not_found`)", body = crate::openapi::ErrorBody),
        (status = 422, description = "Profil encore appliqué à un lieu suivi (`unprocessable_entity`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete(
    State(state): State<AppState>,
    CanWrite(user): CanWrite,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let deleted = sqlx::query("DELETE FROM exposure_profiles WHERE id = $1 AND org_id = $2")
        .bind(id)
        .bind(user.org_id)
        .execute(&state.pg)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound("exposure_profile_not_found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers — seuils (sous-ressource d'un profil)
// ─────────────────────────────────────────────────────────────────────────────

/// Liste les seuils d'un profil VISIBLE (système ou de l'org). Pas de pagination :
/// un profil porte au plus quelques seuils (un par polluant × période).
#[utoipa::path(
    get,
    path = "/api/exposure-profiles/{id}/thresholds",
    tag = "exposure-profiles",
    params(("id" = i64, Path, description = "Identifiant du profil")),
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Seuils du profil (triés polluant puis période)", body = [ExposureThresholdDto]),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 404, description = "Profil inconnu ou d'une autre org (`exposure_profile_not_found`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_thresholds(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Vec<ExposureThresholdDto>>, AppError> {
    let mut conn = state.pg.acquire().await?;
    // Le profil doit être VISIBLE (système ou org) sinon 404.
    if fetch_visible_profile(&mut conn, user.org_id, id)
        .await?
        .is_none()
    {
        return Err(AppError::NotFound("exposure_profile_not_found"));
    }

    let rows = sqlx::query_as::<_, ExposureThresholdDto>(&format!(
        "SELECT {THRESHOLD_COLUMNS} FROM exposure_thresholds \
         WHERE exposure_profile_id = $1 \
         ORDER BY (SELECT p.code FROM parameters p WHERE p.id = parameter_id), averaging_period"
    ))
    .bind(id)
    .fetch_all(&state.pg)
    .await?;
    Ok(Json(rows))
}

/// Ajoute un seuil à un profil PROPRE à l'org (système ⇒ 404).
#[utoipa::path(
    post,
    path = "/api/exposure-profiles/{id}/thresholds",
    tag = "exposure-profiles",
    params(("id" = i64, Path, description = "Identifiant du profil")),
    request_body = CreateExposureThresholdRequest,
    security(("bearer_jwt" = [])),
    responses(
        (status = 201, description = "Seuil créé", body = ExposureThresholdDto),
        (status = 400, description = "Validation échouée ou polluant hors référentiel", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle sans droit d'écriture (`read_only_role`)", body = crate::openapi::ErrorBody),
        (status = 404, description = "Profil inconnu, système ou d'une autre org (`exposure_profile_not_found`)", body = crate::openapi::ErrorBody),
        (status = 409, description = "Seuil déjà défini pour ce (polluant, période) (`conflict`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn create_threshold(
    State(state): State<AppState>,
    CanWrite(user): CanWrite,
    Path(id): Path<i64>,
    ValidatedJson(req): ValidatedJson<CreateExposureThresholdRequest>,
) -> Result<(StatusCode, Json<ExposureThresholdDto>), AppError> {
    // Le profil doit être PROPRE à l'org (pas système, pas d'une autre org) ⇒ 404 sinon.
    if !owns_profile(&state.pg, user.org_id, id).await? {
        return Err(AppError::NotFound("exposure_profile_not_found"));
    }

    // `INSERT … SELECT FROM parameters` : le référentiel fait foi (0 ligne ⇒ 400).
    // 409 auto sur uq (profil, polluant, période).
    let new_id: Option<i64> = sqlx::query_scalar(
        r#"
        INSERT INTO exposure_thresholds (exposure_profile_id, parameter_id, threshold_value, averaging_period)
        SELECT $1, p.id, $2, COALESCE($3, '1h')
        FROM parameters p WHERE p.code = $4
        RETURNING id
        "#,
    )
    .bind(id)
    .bind(req.threshold_value)
    .bind(req.averaging_period.as_deref())
    .bind(&req.parameter)
    .fetch_optional(&state.pg)
    .await?;
    let Some(new_id) = new_id else {
        return Err(AppError::BadRequest(format!(
            "parameter invalide « {} » (hors référentiel des polluants)",
            req.parameter
        )));
    };

    let dto = sqlx::query_as::<_, ExposureThresholdDto>(&format!(
        "SELECT {THRESHOLD_COLUMNS} FROM exposure_thresholds WHERE id = $1"
    ))
    .bind(new_id)
    .fetch_one(&state.pg)
    .await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

/// Supprime un seuil d'un profil PROPRE à l'org.
#[utoipa::path(
    delete,
    path = "/api/exposure-profiles/{id}/thresholds/{threshold_id}",
    tag = "exposure-profiles",
    params(
        ("id" = i64, Path, description = "Identifiant du profil"),
        ("threshold_id" = i64, Path, description = "Identifiant du seuil")
    ),
    security(("bearer_jwt" = [])),
    responses(
        (status = 204, description = "Seuil supprimé"),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle sans droit d'écriture (`read_only_role`)", body = crate::openapi::ErrorBody),
        (status = 404, description = "Profil/seuil inconnu ou d'une autre org (`exposure_threshold_not_found`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete_threshold(
    State(state): State<AppState>,
    CanWrite(user): CanWrite,
    Path((id, threshold_id)): Path<(i64, i64)>,
) -> Result<StatusCode, AppError> {
    if !owns_profile(&state.pg, user.org_id, id).await? {
        return Err(AppError::NotFound("exposure_threshold_not_found"));
    }
    let deleted =
        sqlx::query("DELETE FROM exposure_thresholds WHERE id = $1 AND exposure_profile_id = $2")
            .bind(threshold_id)
            .bind(id)
            .execute(&state.pg)
            .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound("exposure_threshold_not_found"));
    }
    Ok(StatusCode::NO_CONTENT)
}
