//! CRUD `/api/alert-rules` (B6) — règles de seuil d'une organisation.
//!
//! Mêmes conventions que le gabarit `tracked_locations` :
//! - **Isolation multi-tenant par construction** : CHAQUE requête SQL est filtrée
//!   `org_id = <org du JWT>` (colonne dénormalisée sur `alert_rules`, cohérence
//!   `org_id = tracked_locations.org_id` garantie par le trigger **T2**) — une
//!   règle d'une autre org est invisible et les réponses renvoient **404**
//!   (anti-énumération : un id étranger est indistinguable d'un id inexistant).
//!   Le 403 est réservé aux refus de RÔLE.
//! - **Mutations** : extracteur [`CanWrite`] (rôle `lecteur` ⇒ 403 `read_only_role`),
//!   la vérification ne peut pas être oubliée — elle est dans la signature.
//! - **Listing** : pagination/tri/recherche via [`crate::listing`] — tri STRICTEMENT
//!   par allowlist, `q` ILIKE échappé et bindé, requêtes **mono-table +
//!   sous-requêtes** (pas de JOIN dans le SELECT paginé : la clé de tri
//!   secondaire `id` reste non ambiguë).
//!
//! Spécificités du module :
//! - **Audit T5** : tout INSERT/UPDATE/DELETE sur `alert_rules` est journalisé
//!   dans `audit_log` (actions `create`/`update`/`activate`/`deactivate`/`delete`),
//!   l'acteur étant lu dans le GUC `quarity.actor_user_id` ⇒ TOUTE mutation se
//!   fait dans une transaction ouverte par [`crate::db::set_audit_actor`].
//! - `tracked_location_id` et `parameter` sont **IMMUABLES** après création :
//!   les `alert_events` adossés à la règle figent son contexte historique —
//!   « déplacer » la règle sur un autre lieu ou polluant rendrait cet historique
//!   mensonger. Pour changer de cible, on supprime et on re-crée une règle.
//! - Le POST vérifie D'ABORD que le lieu appartient à l'org du JWT (404 sinon :
//!   un lieu étranger est invisible — ni 403 ni 422, qui confirmeraient son
//!   existence), puis insère via `INSERT … SELECT FROM parameters` : le
//!   RÉFÉRENTIEL des polluants fait foi (l'allowlist du boundary n'en est qu'un
//!   miroir de confort) — 0 ligne insérée ⇒ 400.
//! - À la suppression, les `alert_events` survivent (FK `ON DELETE SET NULL`,
//!   snapshot immuable T4) ; T5 trace le `delete`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
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

/// Règle de seuil (élément de listing et de détail).
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct AlertRuleDto {
    pub id: i64,
    /// Org propriétaire — toujours celle du JWT (les autres sont invisibles).
    pub org_id: i64,
    /// Lieu suivi porteur de la règle (IMMUABLE après création).
    pub tracked_location_id: i64,
    /// Nom du lieu porteur (sous-requête de confort pour l'affichage).
    #[schema(example = "Écoles du centre-ville")]
    pub tracked_location_name: String,
    /// Code du polluant surveillé (IMMUABLE après création).
    #[schema(example = "pm25")]
    pub parameter: String,
    /// `>` ou `>=` (seuls les dépassements existent — US-02).
    #[schema(example = ">")]
    pub comparator: String,
    /// Seuil ≥ 0, dans l'unité canonique du polluant (`parameters.unit`).
    /// SELECTé `::float8` : NUMERIC ne se mappe pas sur `f64` sans cast.
    #[schema(example = 15.0)]
    pub threshold_value: f64,
    /// `info` / `warning` / `critical` (défaut à la création : `warning`).
    #[schema(example = "warning")]
    pub severity: String,
    /// `active` / `inactive` — seules les règles actives déclenchent des alertes.
    #[schema(example = "active")]
    pub status: String,
    /// Libellé optionnel (`""` au PATCH = effacement).
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Page de règles de seuil (`{ page, page_size, count, total, data }`).
#[derive(Debug, Serialize, ToSchema)]
pub struct AlertRulesPage {
    pub page: u32,
    pub page_size: u32,
    /// Taille de `data`.
    pub count: usize,
    /// Total filtré (toutes pages confondues).
    pub total: i64,
    pub data: Vec<AlertRuleDto>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Requêtes entrantes
// ─────────────────────────────────────────────────────────────────────────────

/// Statuts de règle — miroir du CHECK `alert_rules.status`. Validateur LOCAL au
/// module : `validation.rs` n'expose pas d'équivalent pour ce CHECK (seuls le
/// listing et le PATCH des règles l'utilisent — inutile d'élargir la surface
/// partagée pour deux usages).
fn validate_status(s: &str) -> Result<(), validator::ValidationError> {
    if matches!(s, "active" | "inactive") {
        return Ok(());
    }
    Err(validator::ValidationError::new("status")
        .with_message("valeurs acceptées : active, inactive".into()))
}

/// Filtres propres au listing des règles (s'ajoutent à [`ListParams`]).
#[derive(Debug, Deserialize, IntoParams, Validate)]
#[into_params(parameter_in = Query)]
pub struct AlertRuleFilters {
    /// Ne renvoyer que les règles d'un lieu suivi donné (de l'org du JWT).
    pub tracked_location_id: Option<i64>,
    /// `active` ou `inactive`.
    #[validate(custom(function = "validate_status"))]
    #[param(example = "active")]
    pub status: Option<String>,
    /// `info`, `warning` ou `critical`.
    #[validate(custom(function = "crate::validation::validate_severity"))]
    pub severity: Option<String>,
    /// Code polluant (allowlist : `pm25`, `pm10`, `no2`, `o3`, `so2`, `co`).
    #[validate(custom(function = "crate::validation::validate_parameter_code"))]
    pub parameter: Option<String>,
}

/// Corps de `POST /api/alert-rules`.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct CreateAlertRuleRequest {
    /// Lieu suivi porteur — DOIT appartenir à l'org du JWT (404 sinon : un lieu
    /// étranger est invisible).
    #[schema(example = 1)]
    pub tracked_location_id: i64,
    /// Polluant (allowlist : `pm25`, `pm10`, `no2`, `o3`, `so2`, `co`).
    #[validate(custom(function = "crate::validation::validate_parameter_code"))]
    #[schema(example = "pm25")]
    pub parameter: String,
    /// `>` ou `>=` (seuls les dépassements existent — US-02).
    #[validate(custom(function = "crate::validation::validate_comparator"))]
    #[schema(example = ">")]
    pub comparator: String,
    /// Seuil ≥ 0, dans l'unité canonique du polluant (`parameters.unit`).
    /// Stocké en `NUMERIC(12,4)` : arrondi SILENCIEUX à 4 décimales — la valeur
    /// effective (arrondie) est celle renvoyée dans la réponse.
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

/// Corps de `PATCH /api/alert-rules/{id}` — champs absents = inchangés ;
/// `name: ""` efface le libellé. `tracked_location_id` et `parameter` sont
/// IMMUABLES (absents de cette struct : serde les ignore, ils ne sont même pas
/// désérialisés — re-créer une règle pour changer de cible, cf. doc de module).
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct UpdateAlertRuleRequest {
    #[validate(custom(function = "crate::validation::validate_comparator"))]
    pub comparator: Option<String>,
    /// Arrondi silencieux à 4 décimales (`NUMERIC(12,4)`) — cf. création.
    #[validate(range(min = 0.0, max = 99999999.0, message = "seuil attendu 0..=99999999"))]
    pub threshold_value: Option<f64>,
    #[validate(custom(function = "crate::validation::validate_severity"))]
    pub severity: Option<String>,
    /// `active` / `inactive` — la bascule est auditée `activate`/`deactivate` (T5).
    #[validate(custom(function = "validate_status"))]
    pub status: Option<String>,
    /// Pas de `min` ici : `""` est une valeur LÉGITIME (effacement du libellé).
    #[validate(length(max = 200, message = "200 caractères maximum"))]
    pub name: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// SQL (scopé org_id — JAMAIS de requête sans le filtre)
// ─────────────────────────────────────────────────────────────────────────────

/// Colonnes du DTO — sous-requêtes (pas de JOIN) : `id` reste non ambigu pour le
/// tri, et `threshold_value` est casté `::float8` (NUMERIC → f64).
const DTO_COLUMNS: &str = r#"
    id, org_id, tracked_location_id,
    (SELECT tl.name FROM tracked_locations tl
      WHERE tl.id = alert_rules.tracked_location_id)                    AS tracked_location_name,
    (SELECT p.code FROM parameters p
      WHERE p.id = alert_rules.parameter_id)                            AS parameter,
    comparator, threshold_value::float8 AS threshold_value,
    severity, status, name, created_at, updated_at
"#;

/// Allowlist de tri : `nom_api → colonne SQL`.
const SORT_ALLOW: &[(&str, &str)] = &[
    ("created_at", "created_at"),
    ("updated_at", "updated_at"),
    ("threshold_value", "threshold_value"),
    ("severity", "severity"),
    ("status", "status"),
];

/// Une règle de l'org, par id — `None` = inexistante OU d'une autre org (404).
async fn fetch_one_dto(
    conn: &mut sqlx::PgConnection,
    org_id: i64,
    id: i64,
) -> Result<Option<AlertRuleDto>, sqlx::Error> {
    let sql = format!("SELECT {DTO_COLUMNS} FROM alert_rules WHERE id = $1 AND org_id = $2");
    sqlx::query_as::<_, AlertRuleDto>(&sql)
        .bind(id)
        .bind(org_id)
        .fetch_optional(conn)
        .await
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// Liste paginée des règles de seuil de l'org de l'appelant.
#[utoipa::path(
    get,
    path = "/api/alert-rules",
    tag = "alert-rules",
    params(ListParams, AlertRuleFilters),
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Page de règles (tri par défaut : `created_at` décroissant)", body = AlertRulesPage),
        (status = 400, description = "Pagination/tri/filtre invalide (corps `ErrorBody`, ou rejet `Query` en corps texte)", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
    ValidatedQuery(filters): ValidatedQuery<AlertRuleFilters>,
) -> Result<Json<AlertRulesPage>, AppError> {
    let p = params.pagination();
    let order_by = params.order_by(SORT_ALLOW, "-created_at")?;
    let like = params.like_pattern();

    // `$n::type IS NULL OR …` : un filtre absent est bindé NULL et neutralisé —
    // une seule requête préparée quel que soit le jeu de filtres.
    // `q` cherche dans le nom de la règle ET celui du lieu porteur (sous-requête,
    // pas de JOIN). `name` est nullable : `NULL ILIKE $2` vaut NULL (≅ false dans
    // le OR) — une règle sans nom n'est trouvée que via le nom de son lieu,
    // comportement voulu. Le filtre `parameter` se résout par sous-requête sur le
    // référentiel (un code absent du référentiel ⇒ simplement 0 ligne).
    let where_clause = r#"
        WHERE org_id = $1
          AND ($2::text IS NULL OR name ILIKE $2
               OR (SELECT tl.name FROM tracked_locations tl
                    WHERE tl.id = alert_rules.tracked_location_id) ILIKE $2)
          AND ($3::bigint IS NULL OR tracked_location_id = $3)
          AND ($4::text IS NULL OR status = $4)
          AND ($5::text IS NULL OR severity = $5)
          AND ($6::text IS NULL OR parameter_id =
               (SELECT p.id FROM parameters p WHERE p.code = $6))
    "#;

    let total: i64 =
        sqlx::query_scalar(&format!("SELECT count(*) FROM alert_rules {where_clause}"))
            .bind(user.org_id)
            .bind(like.as_deref())
            .bind(filters.tracked_location_id)
            .bind(filters.status.as_deref())
            .bind(filters.severity.as_deref())
            .bind(filters.parameter.as_deref())
            .fetch_one(&state.pg)
            .await?;

    let data = sqlx::query_as::<_, AlertRuleDto>(&format!(
        "SELECT {DTO_COLUMNS} FROM alert_rules {where_clause} {order_by} LIMIT $7 OFFSET $8"
    ))
    .bind(user.org_id)
    .bind(like.as_deref())
    .bind(filters.tracked_location_id)
    .bind(filters.status.as_deref())
    .bind(filters.severity.as_deref())
    .bind(filters.parameter.as_deref())
    .bind(p.limit)
    .bind(p.offset)
    .fetch_all(&state.pg)
    .await?;

    Ok(Json(AlertRulesPage {
        page: p.page,
        page_size: p.page_size,
        count: data.len(),
        total,
        data,
    }))
}

/// Crée une règle de seuil sur un lieu suivi de l'org de l'appelant.
#[utoipa::path(
    post,
    path = "/api/alert-rules",
    tag = "alert-rules",
    request_body = CreateAlertRuleRequest,
    security(("bearer_jwt" = [])),
    responses(
        (status = 201, description = "Règle créée (statut `active`, sévérité `warning` par défaut)", body = AlertRuleDto),
        (status = 400, description = "Validation du corps échouée ou polluant hors référentiel (corps `ErrorBody`) — ou JSON malformé (rejet de l'extracteur, corps texte)", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle sans droit d'écriture (`read_only_role`)", body = crate::openapi::ErrorBody),
        (status = 404, description = "Lieu inconnu — ou lieu d'une autre org (anti-énumération : indistinguables, `tracked_location_not_found`)", body = crate::openapi::ErrorBody),
        (status = 409, description = "Règle identique (lieu, polluant, comparateur, seuil) déjà existante (`conflict`)", body = crate::openapi::ErrorBody),
        (status = 413, description = "Corps dépassant la borne des routes CRUD (64 Kio) — rejeté avant tout travail (corps texte)"),
        (status = 415, description = "`Content-Type` non JSON (rejet de l'extracteur, corps texte)"),
        (status = 422, description = "Champs mal typés (rejet de l'extracteur, corps texte) — ou CHECK du schéma ayant échappé au boundary", body = crate::openapi::ErrorBody),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    CanWrite(user): CanWrite,
    ValidatedJson(req): ValidatedJson<CreateAlertRuleRequest>,
) -> Result<(StatusCode, Json<AlertRuleDto>), AppError> {
    // Le lieu doit appartenir à l'org du JWT — sinon 404 (un lieu étranger est
    // INVISIBLE : ni 403 ni 422, qui confirmeraient son existence à un attaquant).
    let owns: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM tracked_locations WHERE id = $1 AND org_id = $2)",
    )
    .bind(req.tracked_location_id)
    .bind(user.org_id)
    .fetch_one(&state.pg)
    .await?;
    if !owns {
        return Err(AppError::NotFound("tracked_location_not_found"));
    }

    let mut tx = state.pg.begin().await?;
    // T5 audite l'INSERT → l'acteur doit être posé AVANT la mutation.
    crate::db::set_audit_actor(tx.as_mut(), user.user_id).await?;

    // org_id et created_by viennent du JWT, JAMAIS du corps (isolation — T2
    // re-vérifie la cohérence org/lieu côté base). `INSERT … SELECT` sur
    // `parameters` : le RÉFÉRENTIEL fait foi, pas seulement l'allowlist du
    // boundary — un code valide en surface mais absent du référentiel n'insère
    // rien ⇒ 400. `status` est omis : le défaut `active` du schéma s'applique.
    let new_id: Option<i64> = sqlx::query_scalar(
        r#"
        INSERT INTO alert_rules
            (org_id, tracked_location_id, parameter_id, comparator,
             threshold_value, severity, name, created_by)
        SELECT $1, $2, p.id, $3, $4, COALESCE($5, 'warning'), $6, $7
        FROM parameters p
        WHERE p.code = $8
        RETURNING id
        "#,
    )
    .bind(user.org_id)
    .bind(req.tracked_location_id)
    .bind(&req.comparator)
    .bind(req.threshold_value)
    .bind(req.severity.as_deref())
    .bind(req.name.as_deref().map(str::trim))
    .bind(user.user_id)
    .bind(&req.parameter)
    .fetch_optional(tx.as_mut())
    .await?;
    let Some(new_id) = new_id else {
        return Err(AppError::BadRequest(format!(
            "parameter invalide « {} » (hors référentiel des polluants)",
            req.parameter
        )));
    };

    let dto = fetch_one_dto(tx.as_mut(), user.org_id, new_id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("règle créée introuvable")))?;
    tx.commit().await?;

    Ok((StatusCode::CREATED, Json(dto)))
}

/// Détail d'une règle de seuil de l'org de l'appelant.
#[utoipa::path(
    get,
    path = "/api/alert-rules/{id}",
    tag = "alert-rules",
    params(("id" = i64, Path, description = "Identifiant de la règle de seuil")),
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Détail de la règle", body = AlertRuleDto),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu — ou règle d'une autre org (anti-énumération : indistinguables, `alert_rule_not_found`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<AlertRuleDto>, AppError> {
    let mut conn = state.pg.acquire().await?;
    let dto = fetch_one_dto(&mut conn, user.org_id, id)
        .await?
        .ok_or(AppError::NotFound("alert_rule_not_found"))?;
    Ok(Json(dto))
}

/// Modifie comparateur / seuil / sévérité / statut / libellé d'une règle.
#[utoipa::path(
    patch,
    path = "/api/alert-rules/{id}",
    tag = "alert-rules",
    params(("id" = i64, Path, description = "Identifiant de la règle de seuil")),
    request_body = UpdateAlertRuleRequest,
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Règle modifiée (état après mutation)", body = AlertRuleDto),
        (status = 400, description = "Validation échouée ou aucun champ fourni (corps `ErrorBody`) — ou JSON malformé (corps texte)", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle sans droit d'écriture (`read_only_role`)", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu ou règle d'une autre org (`alert_rule_not_found`)", body = crate::openapi::ErrorBody),
        (status = 409, description = "La modification recouperait une règle existante (lieu, polluant, comparateur, seuil — `conflict`)", body = crate::openapi::ErrorBody),
        (status = 413, description = "Corps dépassant la borne des routes CRUD (64 Kio) — rejeté avant tout travail (corps texte)"),
        (status = 415, description = "`Content-Type` non JSON (rejet de l'extracteur, corps texte)"),
        (status = 422, description = "Champs mal typés (rejet de l'extracteur, corps texte)"),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    CanWrite(user): CanWrite,
    Path(id): Path<i64>,
    ValidatedJson(req): ValidatedJson<UpdateAlertRuleRequest>,
) -> Result<Json<AlertRuleDto>, AppError> {
    if req.comparator.is_none()
        && req.threshold_value.is_none()
        && req.severity.is_none()
        && req.status.is_none()
        && req.name.is_none()
    {
        return Err(AppError::BadRequest(
            "aucun champ à modifier (attendus : comparator, threshold_value, severity, status, \
             name — tracked_location_id et parameter sont immuables : re-créer une règle)"
                .into(),
        ));
    }

    let mut tx = state.pg.begin().await?;
    // T5 audite l'UPDATE (action `update`, ou `activate`/`deactivate` si le
    // statut bascule) → l'acteur doit être posé AVANT la mutation.
    crate::db::set_audit_actor(tx.as_mut(), user.user_id).await?;

    // `$4::numeric` : le paramètre arrive en float8 (f64) — cast explicite pour
    // que le COALESCE reste en NUMERIC (pas de détour de la valeur existante par
    // float8). Un seuil identique au doublon près ⇒ 409 (uq_alert_rule, mapping auto).
    let updated = sqlx::query(
        r#"
        UPDATE alert_rules SET
            comparator      = COALESCE($3, comparator),
            threshold_value = COALESCE($4::numeric, threshold_value),
            severity        = COALESCE($5, severity),
            status          = COALESCE($6, status),
            name            = CASE WHEN $7::text IS NULL THEN name
                                   ELSE NULLIF(trim($7), '') END,
            updated_at      = now()
        WHERE id = $1 AND org_id = $2
        "#,
    )
    .bind(id)
    .bind(user.org_id)
    .bind(req.comparator.as_deref())
    .bind(req.threshold_value)
    .bind(req.severity.as_deref())
    .bind(req.status.as_deref())
    .bind(req.name.as_deref())
    .execute(tx.as_mut())
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound("alert_rule_not_found"));
    }

    let dto = fetch_one_dto(tx.as_mut(), user.org_id, id)
        .await?
        .ok_or(AppError::NotFound("alert_rule_not_found"))?;
    tx.commit().await?;
    Ok(Json(dto))
}

/// Paramètres du force-check (`POST /api/alert-rules/{id}/run`).
#[derive(Debug, Deserialize, IntoParams, Validate)]
#[into_params(parameter_in = Query)]
pub struct RunQuery {
    /// Fenêtre réévaluée, en heures d'ARRIVÉE des mesures (`ingested_at`) —
    /// défaut 24, bornée 1..=168 (la dédup 0006 rend le rejouage sans risque).
    #[validate(range(min = 1, max = 168, message = "lookback_hours attendu 1..=168"))]
    pub lookback_hours: Option<u32>,
}

/// Bilan d'un force-check.
#[derive(Debug, Serialize, ToSchema)]
pub struct RunOutcome {
    pub rule_id: i64,
    /// Mesures (dédupliquées) relues sur la fenêtre.
    pub evaluated: usize,
    /// Dépassements constatés (avant déduplication en base).
    pub breaches: usize,
    /// Événements RÉELLEMENT créés (un re-run renvoie 0 : idempotence 0006).
    pub events_created: u64,
}

/// Force-check (B7) : réévalue UNE règle immédiatement sur les mesures
/// récemment ingérées — même moteur que la boucle périodique (hot path Moka),
/// même idempotence (un dépassement déjà alerté ne re-crée rien).
#[utoipa::path(
    post,
    path = "/api/alert-rules/{id}/run",
    tag = "alert-rules",
    params(
        ("id" = i64, Path, description = "Identifiant de la règle de seuil"),
        RunQuery
    ),
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Bilan d'évaluation (les événements créés apparaissent dans alert_events / le compteur non-lus)", body = RunOutcome),
        (status = 400, description = "`lookback_hours` hors borne 1..=168 (corps `ErrorBody`, ou rejet `Query` en corps texte)", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle sans droit d'écriture (`read_only_role`)", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu ou règle d'une autre org (`alert_rule_not_found`)", body = crate::openapi::ErrorBody),
        (status = 422, description = "Règle inactive, lieu en pause ou org supprimée — rien à évaluer (`unprocessable_entity`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn run(
    State(state): State<AppState>,
    CanWrite(user): CanWrite,
    Path(id): Path<i64>,
    ValidatedQuery(q): ValidatedQuery<RunQuery>,
) -> Result<Json<RunOutcome>, AppError> {
    // 404 d'abord (existence scopée org — anti-énumération), 422 ensuite
    // (la règle existe mais n'est pas évaluable : statut/lieu/org).
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM alert_rules WHERE id = $1 AND org_id = $2)",
    )
    .bind(id)
    .bind(user.org_id)
    .fetch_one(&state.pg)
    .await?;
    if !exists {
        return Err(AppError::NotFound("alert_rule_not_found"));
    }

    let index = crate::matching::load_rule_index_for_rule(&state.pg, user.org_id, id).await?;
    if index.is_empty() {
        return Err(AppError::Unprocessable(
            "règle inactive, lieu suivi en pause ou organisation supprimée — rien à évaluer".into(),
        ));
    }

    let lookback = i64::from(q.lookback_hours.unwrap_or(24));
    let since = Utc::now() - Duration::hours(lookback);
    let outcome = crate::matching::run_once(
        &state.pg,
        &state.ch,
        &index,
        since,
        state.cfg.matching_batch_limit,
    )
    .await
    .map_err(AppError::Internal)?;

    // Push temps réel B8 : un force-check qui CRÉE des événements les publie aussi
    // (même contrat que la boucle) — uniquement les RÉELLEMENT insérés. Best-effort.
    if !outcome.inserted_events.is_empty() {
        let redis = state.redis.clone();
        crate::alerts::publish_alert_events(&redis, &outcome.inserted_events).await;
    }

    Ok(Json(RunOutcome {
        rule_id: id,
        evaluated: outcome.evaluated,
        breaches: outcome.breaches,
        events_created: outcome.inserted,
    }))
}

/// Supprime une règle de seuil (les `alert_events` adossés survivent — FK
/// `ON DELETE SET NULL`, snapshot immuable T4 ; T5 trace le `delete`).
#[utoipa::path(
    delete,
    path = "/api/alert-rules/{id}",
    tag = "alert-rules",
    params(("id" = i64, Path, description = "Identifiant de la règle de seuil")),
    security(("bearer_jwt" = [])),
    responses(
        (status = 204, description = "Règle supprimée (auditée T5 ; events conservés)"),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle sans droit d'écriture (`read_only_role`)", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu ou règle d'une autre org (`alert_rule_not_found`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete(
    State(state): State<AppState>,
    CanWrite(user): CanWrite,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let mut tx = state.pg.begin().await?;
    // T5 trace le `delete` avec cet acteur.
    crate::db::set_audit_actor(tx.as_mut(), user.user_id).await?;

    let deleted = sqlx::query("DELETE FROM alert_rules WHERE id = $1 AND org_id = $2")
        .bind(id)
        .bind(user.org_id)
        .execute(tx.as_mut())
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound("alert_rule_not_found"));
    }
    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}
