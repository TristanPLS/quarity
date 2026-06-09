//! CRUD `/api/tracked-locations` (B6) — lieux suivis d'une organisation.
//!
//! GABARIT des modules CRUD B6. Conventions appliquées partout :
//! - **Isolation multi-tenant par construction** : CHAQUE requête SQL est filtrée
//!   `org_id = <org du JWT>` — une ressource d'une autre org est invisible et les
//!   réponses renvoient **404** (anti-énumération : un id étranger est
//!   indistinguable d'un id inexistant). Le 403 est réservé aux refus de RÔLE.
//! - **Mutations** : extracteur [`CanWrite`] (rôle `lecteur` ⇒ 403 `read_only_role`),
//!   la vérification ne peut pas être oubliée — elle est dans la signature.
//! - **Codes** : 201 + corps à la création, 204 à la suppression, 409 doublon
//!   (contrainte unique → `error.rs`), 422 règle métier (triggers `QRT_*`).
//! - **Listing** : pagination/tri/recherche via [`crate::listing`] — tri STRICTEMENT
//!   par allowlist, `q` ILIKE échappé et bindé.
//! - **Requêtes mono-table + sous-requêtes** (pas de JOIN au niveau du SELECT
//!   paginé) : la clé de tri secondaire `id` reste non ambiguë.
//!
//! Spécificités du module :
//! - POST délègue à la procédure **P1** `create_tracked_location_with_rules`
//!   (atomique : lieu + stations [la 1re = primaire] + règles optionnelles) ;
//! - les mutations qui touchent `alert_rules` (P1, DELETE en cascade) posent le
//!   GUC `quarity.actor_user_id` pour l'audit **T5** ;
//! - la composition des stations est figée à la création en B6 (l'édition fine
//!   des stations — protégée par T1 — viendra avec un sous-resource dédié).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use utoipa::{IntoParams, ToSchema};
use validator::Validate;

use crate::error::AppError;
use crate::listing::ListParams;
use crate::security::{AuthUser, CanWrite};
use crate::state::AppState;
use crate::validation::{ValidatedJson, ValidatedQuery};

// ─────────────────────────────────────────────────────────────────────────────
// DTOs
// ─────────────────────────────────────────────────────────────────────────────

/// Lieu suivi (élément de listing et socle du détail).
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct TrackedLocationDto {
    pub id: i64,
    /// Org propriétaire — toujours celle du JWT (les autres sont invisibles).
    pub org_id: i64,
    #[schema(example = "Écoles du centre-ville")]
    pub name: String,
    pub description: Option<String>,
    /// `false` = surveillance en pause (le lieu reste listé, ses règles dormantes).
    pub is_active: bool,
    /// Nombre de stations OpenAQ liées (≥ 1 par construction — P1 puis T1).
    pub station_count: i64,
    /// Nombre de règles d'alerte ACTIVES adossées au lieu.
    pub active_rule_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Station OpenAQ liée à un lieu suivi (détail uniquement).
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct TrackedStationDto {
    /// Clé naturelle OpenAQ (celle de `GET /api/measurements?location_id=`).
    #[schema(example = 1001)]
    pub openaq_location_id: i64,
    pub name: String,
    pub city: Option<String>,
    /// ISO-3166-1 alpha-2.
    #[schema(example = "FR")]
    pub country: String,
    /// Station de référence du lieu (au plus une — index partiel `uq_tls_primary`).
    pub is_primary: bool,
}

/// Détail d'un lieu suivi : le lieu + ses stations.
#[derive(Debug, Serialize, ToSchema)]
pub struct TrackedLocationDetail {
    #[serde(flatten)]
    pub location: TrackedLocationDto,
    pub stations: Vec<TrackedStationDto>,
}

/// Page de lieux suivis (`{ page, page_size, count, total, data }`).
#[derive(Debug, Serialize, ToSchema)]
pub struct TrackedLocationsPage {
    pub page: u32,
    pub page_size: u32,
    /// Taille de `data`.
    pub count: usize,
    /// Total filtré (toutes pages confondues).
    pub total: i64,
    pub data: Vec<TrackedLocationDto>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Requêtes entrantes
// ─────────────────────────────────────────────────────────────────────────────

/// Filtres propres au listing des lieux (s'ajoutent à [`ListParams`]).
#[derive(Debug, Deserialize, IntoParams, Validate)]
#[into_params(parameter_in = Query)]
pub struct TrackedLocationFilters {
    /// Ne renvoyer que les lieux actifs (`true`) ou en pause (`false`).
    pub is_active: Option<bool>,
}

/// Règle d'alerte créée en même temps que le lieu (forme attendue par P1).
/// (`Serialize` : requis par le validateur `length` sur `Vec<InlineRuleRequest>`,
/// qui sérialise la valeur fautive dans son message d'erreur.)
#[derive(Debug, Serialize, Deserialize, ToSchema, Validate)]
pub struct InlineRuleRequest {
    /// Polluant (allowlist : `pm25`, `pm10`, `no2`, `o3`, `so2`, `co`).
    #[validate(custom(function = "crate::validation::validate_parameter_code"))]
    #[schema(example = "pm25")]
    pub parameter: String,
    /// `>` ou `>=` (seuls les dépassements existent — US-02).
    #[validate(custom(function = "crate::validation::validate_comparator"))]
    #[schema(example = ">")]
    pub comparator: String,
    /// Seuil ≥ 0, dans l'unité canonique du polluant (`parameters.unit`).
    /// Stocké en `NUMERIC(12,4)` : arrondi SILENCIEUX à 4 décimales.
    #[validate(range(min = 0.0, max = 99999999.0, message = "seuil attendu 0..=99999999"))]
    #[schema(example = 15.0)]
    pub threshold_value: f64,
    /// Défaut : `warning`.
    #[validate(custom(function = "crate::validation::validate_severity"))]
    pub severity: Option<String>,
    /// Libellé optionnel (1..=200, non blanc).
    #[validate(
        length(min = 1, max = 200, message = "longueur attendue 1..=200"),
        custom(function = "crate::validation::validate_not_blank")
    )]
    pub name: Option<String>,
}

/// Corps de `POST /api/tracked-locations`.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct CreateTrackedLocationRequest {
    /// Nom du lieu, unique dans l'org (1..=200, non blanc).
    #[validate(
        length(min = 1, max = 200, message = "longueur attendue 1..=200"),
        custom(function = "crate::validation::validate_not_blank")
    )]
    #[schema(example = "Écoles du centre-ville")]
    pub name: String,
    #[validate(length(max = 2000, message = "2000 caractères maximum"))]
    pub description: Option<String>,
    /// Stations OpenAQ (clés naturelles), 1 à 50 — la PREMIÈRE devient primaire.
    /// Stations inconnues du référentiel ou en double → 422 (`QRT_P1`).
    #[validate(length(min = 1, max = 50, message = "1 à 50 stations"))]
    #[schema(example = json!([1001, 1002]))]
    pub openaq_location_ids: Vec<i64>,
    /// Règles d'alerte créées atomiquement avec le lieu (50 max).
    #[validate(length(max = 50, message = "50 règles maximum"), nested)]
    pub rules: Option<Vec<InlineRuleRequest>>,
}

/// Corps de `PATCH /api/tracked-locations/{id}` — champs absents = inchangés ;
/// `description: ""` efface la description.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct UpdateTrackedLocationRequest {
    #[validate(
        length(min = 1, max = 200, message = "longueur attendue 1..=200"),
        custom(function = "crate::validation::validate_not_blank")
    )]
    pub name: Option<String>,
    #[validate(length(max = 2000, message = "2000 caractères maximum"))]
    pub description: Option<String>,
    pub is_active: Option<bool>,
}

// ─────────────────────────────────────────────────────────────────────────────
// SQL (scopé org_id — JAMAIS de requête sans le filtre)
// ─────────────────────────────────────────────────────────────────────────────

/// Colonnes du DTO — sous-requêtes (pas de JOIN) : `id` reste non ambigu pour le tri.
const DTO_COLUMNS: &str = r#"
    id, org_id, name, description, is_active,
    (SELECT count(*) FROM tracked_location_stations tls
      WHERE tls.tracked_location_id = tracked_locations.id)               AS station_count,
    (SELECT count(*) FROM alert_rules ar
      WHERE ar.tracked_location_id = tracked_locations.id
        AND ar.status = 'active')                                         AS active_rule_count,
    created_at, updated_at
"#;

/// Allowlist de tri : `nom_api → colonne SQL`.
const SORT_ALLOW: &[(&str, &str)] = &[
    ("name", "lower(name)"),
    ("created_at", "created_at"),
    ("updated_at", "updated_at"),
    ("is_active", "is_active"),
];

async fn fetch_detail(
    conn: &mut sqlx::PgConnection,
    org_id: i64,
    id: i64,
) -> Result<Option<TrackedLocationDetail>, sqlx::Error> {
    let sql = format!("SELECT {DTO_COLUMNS} FROM tracked_locations WHERE id = $1 AND org_id = $2");
    let Some(location) = sqlx::query_as::<_, TrackedLocationDto>(&sql)
        .bind(id)
        .bind(org_id)
        .fetch_optional(&mut *conn)
        .await?
    else {
        return Ok(None);
    };

    let stations = sqlx::query_as::<_, TrackedStationDto>(
        r#"
        SELECT rl.openaq_location_id, rl.name, rl.city, rl.country::text AS country, tls.is_primary
        FROM tracked_location_stations tls
        JOIN ref_locations rl ON rl.id = tls.ref_location_id
        WHERE tls.tracked_location_id = $1
        ORDER BY tls.is_primary DESC, rl.openaq_location_id
        "#,
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;

    Ok(Some(TrackedLocationDetail { location, stations }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// Liste paginée des lieux suivis de l'org de l'appelant.
#[utoipa::path(
    get,
    path = "/api/tracked-locations",
    tag = "tracked-locations",
    params(ListParams, TrackedLocationFilters),
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Page de lieux (tri par défaut : `name` croissant)", body = TrackedLocationsPage),
        (status = 400, description = "Pagination/tri/filtre invalide (corps `ErrorBody`, ou rejet `Query` en corps texte)", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
    ValidatedQuery(filters): ValidatedQuery<TrackedLocationFilters>,
) -> Result<Json<TrackedLocationsPage>, AppError> {
    let p = params.pagination();
    let order_by = params.order_by(SORT_ALLOW, "name")?;
    let like = params.like_pattern();

    // `$n::type IS NULL OR …` : un filtre absent est bindé NULL et neutralisé —
    // une seule requête préparée quel que soit le jeu de filtres.
    let where_clause = r#"
        WHERE org_id = $1
          AND ($2::text IS NULL OR name ILIKE $2 OR description ILIKE $2)
          AND ($3::boolean IS NULL OR is_active = $3)
    "#;

    let total: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM tracked_locations {where_clause}"
    ))
    .bind(user.org_id)
    .bind(like.as_deref())
    .bind(filters.is_active)
    .fetch_one(&state.pg)
    .await?;

    let data = sqlx::query_as::<_, TrackedLocationDto>(&format!(
        "SELECT {DTO_COLUMNS} FROM tracked_locations {where_clause} {order_by} LIMIT $4 OFFSET $5"
    ))
    .bind(user.org_id)
    .bind(like.as_deref())
    .bind(filters.is_active)
    .bind(p.limit)
    .bind(p.offset)
    .fetch_all(&state.pg)
    .await?;

    Ok(Json(TrackedLocationsPage {
        page: p.page,
        page_size: p.page_size,
        count: data.len(),
        total,
        data,
    }))
}

/// Crée un lieu suivi (+ stations + règles optionnelles), atomiquement via P1.
#[utoipa::path(
    post,
    path = "/api/tracked-locations",
    tag = "tracked-locations",
    request_body = CreateTrackedLocationRequest,
    security(("bearer_jwt" = [])),
    responses(
        (status = 201, description = "Lieu créé (stations résolues, 1re = primaire)", body = TrackedLocationDetail),
        (status = 400, description = "Validation du corps échouée (corps `ErrorBody`) — ou JSON malformé (rejet de l'extracteur, corps texte)", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle sans droit d'écriture (`read_only_role`)", body = crate::openapi::ErrorBody),
        (status = 409, description = "Un lieu porte déjà ce nom dans l'org (`conflict`)", body = crate::openapi::ErrorBody),
        (status = 413, description = "Corps dépassant la borne des routes CRUD (64 Kio) — rejeté avant tout travail (corps texte)"),
        (status = 415, description = "`Content-Type` non JSON (rejet de l'extracteur, corps texte)"),
        (status = 422, description = "Règle métier refusée : station inconnue du référentiel, doublon de station, champs mal typés (`QRT_P1` / rejet extracteur)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    CanWrite(user): CanWrite,
    ValidatedJson(req): ValidatedJson<CreateTrackedLocationRequest>,
) -> Result<(StatusCode, Json<TrackedLocationDetail>), AppError> {
    // Forme attendue par P1 : [{"parameter","comparator","threshold","severity","name"}].
    let rules_json = json!(req
        .rules
        .unwrap_or_default()
        .iter()
        .map(|r| {
            json!({
                "parameter": r.parameter,
                "comparator": r.comparator,
                "threshold": r.threshold_value,
                "severity": r.severity.as_deref().unwrap_or("warning"),
                "name": r.name,
            })
        })
        .collect::<Vec<_>>())
    .to_string();

    let mut tx = state.pg.begin().await?;
    // P1 peut créer des règles → T5 audite avec l'acteur de la transaction.
    crate::db::set_audit_actor(tx.as_mut(), user.user_id).await?;

    // org_id et created_by viennent du JWT, JAMAIS du corps (isolation).
    let row =
        sqlx::query("CALL create_tracked_location_with_rules($1, $2, $3, $4, $5, $6::jsonb, NULL)")
            .bind(user.org_id)
            .bind(req.name.trim())
            .bind(req.description.as_deref().filter(|d| !d.trim().is_empty()))
            .bind(user.user_id)
            .bind(&req.openaq_location_ids)
            .bind(rules_json)
            .fetch_one(tx.as_mut())
            .await?;
    let new_id: i64 = row
        .try_get("p_tracked_location_id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("P1 sans id retourné : {e}")))?;

    let detail = fetch_detail(tx.as_mut(), user.org_id, new_id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("lieu créé par P1 introuvable")))?;
    tx.commit().await?;

    Ok((StatusCode::CREATED, Json(detail)))
}

/// Détail d'un lieu suivi de l'org de l'appelant (avec ses stations).
#[utoipa::path(
    get,
    path = "/api/tracked-locations/{id}",
    tag = "tracked-locations",
    params(("id" = i64, Path, description = "Identifiant du lieu suivi")),
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Détail du lieu (stations triées : primaire d'abord)", body = TrackedLocationDetail),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu — ou lieu d'une autre org (anti-énumération : indistinguables, `tracked_location_not_found`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<TrackedLocationDetail>, AppError> {
    let mut conn = state.pg.acquire().await?;
    let detail = fetch_detail(&mut conn, user.org_id, id)
        .await?
        .ok_or(AppError::NotFound("tracked_location_not_found"))?;
    Ok(Json(detail))
}

/// Modifie nom / description / activité d'un lieu suivi.
#[utoipa::path(
    patch,
    path = "/api/tracked-locations/{id}",
    tag = "tracked-locations",
    params(("id" = i64, Path, description = "Identifiant du lieu suivi")),
    request_body = UpdateTrackedLocationRequest,
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Lieu modifié (état après mutation)", body = TrackedLocationDetail),
        (status = 400, description = "Validation échouée ou aucun champ fourni (corps `ErrorBody`) — ou JSON malformé (corps texte)", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle sans droit d'écriture (`read_only_role`)", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu ou lieu d'une autre org (`tracked_location_not_found`)", body = crate::openapi::ErrorBody),
        (status = 409, description = "Un lieu porte déjà ce nom dans l'org (`conflict`)", body = crate::openapi::ErrorBody),
        (status = 413, description = "Corps dépassant la borne des routes CRUD (64 Kio) — rejeté avant tout travail (corps texte)"),
        (status = 415, description = "`Content-Type` non JSON (rejet de l'extracteur, corps texte)"),
        (status = 422, description = "Champs mal typés (rejet de l'extracteur, corps texte)"),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    CanWrite(user): CanWrite,
    Path(id): Path<i64>,
    ValidatedJson(req): ValidatedJson<UpdateTrackedLocationRequest>,
) -> Result<Json<TrackedLocationDetail>, AppError> {
    if req.name.is_none() && req.description.is_none() && req.is_active.is_none() {
        return Err(AppError::BadRequest(
            "aucun champ à modifier (attendus : name, description, is_active)".into(),
        ));
    }

    let mut tx = state.pg.begin().await?;
    let updated = sqlx::query(
        r#"
        UPDATE tracked_locations SET
            name        = COALESCE($3, name),
            description = CASE WHEN $4::text IS NULL THEN description
                               ELSE NULLIF(trim($4), '') END,
            is_active   = COALESCE($5, is_active),
            updated_at  = now()
        WHERE id = $1 AND org_id = $2
        "#,
    )
    .bind(id)
    .bind(user.org_id)
    .bind(req.name.as_deref().map(str::trim))
    .bind(req.description.as_deref())
    .bind(req.is_active)
    .execute(tx.as_mut())
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound("tracked_location_not_found"));
    }

    let detail = fetch_detail(tx.as_mut(), user.org_id, id)
        .await?
        .ok_or(AppError::NotFound("tracked_location_not_found"))?;
    tx.commit().await?;
    Ok(Json(detail))
}

/// Supprime un lieu suivi (stations liées et règles en cascade ; les
/// `alert_events` survivent — FK `ON DELETE SET NULL`, snapshot immuable T4).
#[utoipa::path(
    delete,
    path = "/api/tracked-locations/{id}",
    tag = "tracked-locations",
    params(("id" = i64, Path, description = "Identifiant du lieu suivi")),
    security(("bearer_jwt" = [])),
    responses(
        (status = 204, description = "Lieu supprimé (règles en cascade, auditées T5 ; events conservés)"),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle sans droit d'écriture (`read_only_role`)", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu ou lieu d'une autre org (`tracked_location_not_found`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete(
    State(state): State<AppState>,
    CanWrite(user): CanWrite,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let mut tx = state.pg.begin().await?;
    // La cascade supprime les règles du lieu → T5 trace `delete` avec cet acteur.
    crate::db::set_audit_actor(tx.as_mut(), user.user_id).await?;

    let deleted = sqlx::query("DELETE FROM tracked_locations WHERE id = $1 AND org_id = $2")
        .bind(id)
        .bind(user.org_id)
        .execute(tx.as_mut())
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound("tracked_location_not_found"));
    }
    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}
