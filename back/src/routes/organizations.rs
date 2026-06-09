//! CRUD `/api/organizations` (B6) — les organisations de L'APPELANT.
//!
//! Mêmes conventions que le gabarit (`tracked_locations`) : 404 anti-énumération,
//! codes d'erreur stables, listing par allowlist, requêtes mono-table +
//! sous-requêtes. Avec une différence STRUCTURELLE de périmètre :
//! - le scope n'est PAS l'`org_id` du JWT mais « les orgs dont l'appelant est
//!   MEMBRE » (`EXISTS memberships`) — un user peut être multi-org ;
//! - le rôle des claims ne vaut que pour l'org du JWT : pour `/{id}`, le rôle est
//!   RE-RÉSOLU en base via [`crate::db::fetch_role_in_org`]. Les gardes
//!   d'extracteur (`CanWrite`, `RequireAdmin`) lisent les claims : elles sont
//!   donc INADAPTÉES ici — les handlers prennent [`AuthUser`] et tranchent après
//!   résolution. Ordre des refus : non-membre / org inconnue / org supprimée
//!   ⇒ **404** `organization_not_found` D'ABORD ; puis membre non admin ⇒
//!   **403** `admin_required`. Le 403 ne concerne ainsi qu'un MEMBRE de l'org :
//!   il ne révèle jamais l'existence d'une org étrangère.
//!
//! Choix assumés du module :
//! - **POST ouvert à tout authentifié** : fonder un NOUVEAU tenant n'est pas une
//!   mutation du tenant courant — même un `lecteur` d'une autre org peut créer
//!   la sienne (il en devient `admin`, membership posée dans la même
//!   transaction). LIMITE B6 : le JWT de l'appelant reste sur son org d'origine —
//!   les ressources scopées JWT (lieux, règles, membres) de la nouvelle org
//!   attendent le sélecteur multi-org au login (post-B6) ; GET/PATCH/DELETE
//!   `/{id}` fonctionnent eux dès maintenant (rôle re-résolu en base).
//! - **slug IMMUABLE en B6** : identifiant public stable (URLs, intégrations) —
//!   pas de renommage tant qu'un mécanisme de redirection n'existe pas. Le PATCH
//!   ne connaît tout simplement pas le champ.
//! - **DELETE = SOFT-delete** (`deleted_at = now()`) : les `alert_events`
//!   référencent `org_id` en FK RESTRICT — jamais de suppression physique. Une
//!   org supprimée disparaît de tous les listings/lookups (filtre
//!   `deleted_at IS NULL` partout, `fetch_role_in_org` compris) et ses
//!   logins/refresh meurent (`db.rs` filtre déjà `deleted_at`).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use validator::Validate;

use crate::error::AppError;
use crate::listing::ListParams;
use crate::security::AuthUser;
use crate::state::AppState;
use crate::validation::{ValidatedJson, ValidatedQuery};

// ─────────────────────────────────────────────────────────────────────────────
// DTOs
// ─────────────────────────────────────────────────────────────────────────────

/// Organisation vue par UN appelant (le rôle `my_role` dépend de qui demande).
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct OrgDto {
    pub id: i64,
    #[schema(example = "Agglo Riviera")]
    pub name: String,
    /// Identifiant public stable (CITEXT, `^[a-z0-9-]{2,64}$`) — IMMUABLE en B6.
    #[schema(example = "agglo-riviera")]
    pub slug: String,
    /// Segment commercial (`B2G`, `B2B`, `B2B2C`) — optionnel.
    #[schema(example = "B2G")]
    pub segment: Option<String>,
    /// Alertes non lues de l'org (compteur maintenu par trigger T6).
    #[schema(example = 0)]
    pub unread_alert_count: i32,
    /// Rôle de L'APPELANT dans CETTE org (sous-requête corrélée sur SES
    /// memberships) — un user multi-org peut être `admin` ici et `lecteur` là.
    #[schema(example = "admin")]
    pub my_role: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Page d'organisations (`{ page, page_size, count, total, data }`).
#[derive(Debug, Serialize, ToSchema)]
pub struct OrganizationsPage {
    pub page: u32,
    pub page_size: u32,
    /// Taille de `data`.
    pub count: usize,
    /// Total filtré (toutes pages confondues).
    pub total: i64,
    pub data: Vec<OrgDto>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Requêtes entrantes
// ─────────────────────────────────────────────────────────────────────────────

/// Filtres propres au listing des organisations (s'ajoutent à [`ListParams`]).
#[derive(Debug, Deserialize, IntoParams, Validate)]
#[into_params(parameter_in = Query)]
pub struct OrganizationFilters {
    /// Ne renvoyer que les orgs du segment donné (`B2G`, `B2B`, `B2B2C`).
    /// Un segment NULL en base ne matche jamais un filtre.
    #[validate(custom(function = "crate::validation::validate_segment"))]
    pub segment: Option<String>,
}

/// Corps de `POST /api/organizations`.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct CreateOrganizationRequest {
    /// Nom de l'organisation (1..=200, non blanc) — non unique : c'est le slug
    /// qui identifie publiquement.
    #[validate(
        length(min = 1, max = 200, message = "longueur attendue 1..=200"),
        custom(function = "crate::validation::validate_not_blank")
    )]
    #[schema(example = "Mairie de Bormes")]
    pub name: String,
    /// Slug public, UNIQUE GLOBALEMENT (`^[a-z0-9-]{2,64}$`). Pris → 409.
    #[validate(custom(function = "crate::validation::validate_org_slug"))]
    #[schema(example = "mairie-de-bormes")]
    pub slug: String,
    /// Segment commercial optionnel (`B2G`, `B2B`, `B2B2C`).
    #[validate(custom(function = "crate::validation::validate_segment"))]
    pub segment: Option<String>,
}

/// `segment` en PATCH : une valeur de l'allowlist OU `""` (= effacement —
/// convention PATCH des champs TEXT optionnels). Le POST, lui, reste strict :
/// l'absence du champ y suffit pour « pas de segment ».
fn validate_segment_or_clear(s: &str) -> Result<(), validator::ValidationError> {
    if s.is_empty() {
        return Ok(());
    }
    crate::validation::validate_segment(s)
}

/// Corps de `PATCH /api/organizations/{id}` — champs absents = inchangés ;
/// `segment: ""` efface le segment. Le slug est IMMUABLE (absent du contrat).
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct UpdateOrganizationRequest {
    #[validate(
        length(min = 1, max = 200, message = "longueur attendue 1..=200"),
        custom(function = "crate::validation::validate_not_blank")
    )]
    pub name: Option<String>,
    /// `B2G`, `B2B`, `B2B2C` — ou `""` pour effacer.
    #[validate(custom(function = "validate_segment_or_clear"))]
    pub segment: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// SQL (scopé MEMBERSHIP de l'appelant — JAMAIS de requête sans ce filtre,
// ni sans `deleted_at IS NULL`)
// ─────────────────────────────────────────────────────────────────────────────

/// Colonnes du DTO — sous-requête corrélée (pas de JOIN) : `my_role` est le rôle
/// de L'APPELANT et `id` reste non ambigu pour le tri secondaire.
///
/// CONVENTION DE BIND : `$1` est TOUJOURS l'id de l'appelant (`user_id`) dans
/// toute requête qui embarque ces colonnes (le `WHERE` le réutilise pour le
/// périmètre membership).
const DTO_COLUMNS: &str = r#"
    id, name, slug::text AS slug, segment, unread_alert_count,
    (SELECT r.code FROM memberships m
       JOIN roles r ON r.id = m.role_id
      WHERE m.org_id = organizations.id AND m.user_id = $1)               AS my_role,
    created_at, updated_at
"#;

/// Allowlist de tri : `nom_api → colonne SQL`.
const SORT_ALLOW: &[(&str, &str)] = &[
    ("name", "lower(name)"),
    ("created_at", "created_at"),
    ("updated_at", "updated_at"),
];

/// Une org visible par l'appelant (membre + non soft-supprimée), sinon `None`.
async fn fetch_one(
    conn: &mut sqlx::PgConnection,
    user_id: i64,
    id: i64,
) -> Result<Option<OrgDto>, sqlx::Error> {
    let sql = format!(
        r#"
        SELECT {DTO_COLUMNS} FROM organizations
        WHERE id = $2
          AND deleted_at IS NULL
          AND EXISTS (SELECT 1 FROM memberships m
                       WHERE m.org_id = organizations.id AND m.user_id = $1)
        "#
    );
    sqlx::query_as::<_, OrgDto>(&sql)
        .bind(user_id)
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// Liste paginée des organisations dont l'appelant est membre.
#[utoipa::path(
    get,
    path = "/api/organizations",
    tag = "organizations",
    params(ListParams, OrganizationFilters),
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Page des organisations de l'appelant (tri par défaut : `name` croissant) — les orgs soft-supprimées n'apparaissent jamais", body = OrganizationsPage),
        (status = 400, description = "Pagination/tri/filtre invalide (corps `ErrorBody`, ou rejet `Query` en corps texte)", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
    ValidatedQuery(filters): ValidatedQuery<OrganizationFilters>,
) -> Result<Json<OrganizationsPage>, AppError> {
    let p = params.pagination();
    let order_by = params.order_by(SORT_ALLOW, "name")?;
    let like = params.like_pattern();

    // Périmètre = memberships de l'appelant (PAS l'org du JWT : multi-org).
    // `$n::type IS NULL OR …` : un filtre absent est bindé NULL et neutralisé —
    // une seule requête préparée quel que soit le jeu de filtres.
    let where_clause = r#"
        WHERE deleted_at IS NULL
          AND EXISTS (SELECT 1 FROM memberships m
                       WHERE m.org_id = organizations.id AND m.user_id = $1)
          AND ($2::text IS NULL OR name ILIKE $2 OR slug::text ILIKE $2)
          AND ($3::text IS NULL OR segment = $3)
    "#;

    let total: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM organizations {where_clause}"
    ))
    .bind(user.user_id)
    .bind(like.as_deref())
    .bind(filters.segment.as_deref())
    .fetch_one(&state.pg)
    .await?;

    let data = sqlx::query_as::<_, OrgDto>(&format!(
        "SELECT {DTO_COLUMNS} FROM organizations {where_clause} {order_by} LIMIT $4 OFFSET $5"
    ))
    .bind(user.user_id)
    .bind(like.as_deref())
    .bind(filters.segment.as_deref())
    .bind(p.limit)
    .bind(p.offset)
    .fetch_all(&state.pg)
    .await?;

    Ok(Json(OrganizationsPage {
        page: p.page,
        page_size: p.page_size,
        count: data.len(),
        total,
        data,
    }))
}

/// Crée une organisation — l'appelant en devient `admin` (atomique).
///
/// `AuthUser` simple, PAS `CanWrite` : créer un NOUVEAU tenant n'est pas une
/// mutation du tenant courant — même un `lecteur` d'une autre org peut fonder
/// la sienne. LIMITE B6 : le JWT de l'appelant reste sur son org d'origine
/// (sélecteur multi-org au login : post-B6) ; `GET/PATCH/DELETE /{id}` sur la
/// nouvelle org fonctionnent eux dès maintenant (rôle re-résolu en base).
#[utoipa::path(
    post,
    path = "/api/organizations",
    tag = "organizations",
    request_body = CreateOrganizationRequest,
    security(("bearer_jwt" = [])),
    responses(
        (status = 201, description = "Organisation créée — l'appelant en est `admin` (`my_role`). Les ressources scopées JWT de la nouvelle org attendent le sélecteur multi-org au login (post-B6)", body = OrgDto),
        (status = 400, description = "Validation du corps échouée : nom blanc, slug hors `^[a-z0-9-]{2,64}$`, segment hors allowlist (corps `ErrorBody`) — ou JSON malformé (rejet de l'extracteur, corps texte)", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 409, description = "Slug déjà pris — l'unicité est GLOBALE (`conflict`)", body = crate::openapi::ErrorBody),
        (status = 413, description = "Corps dépassant la borne des routes CRUD (64 Kio) — rejeté avant tout travail (corps texte)"),
        (status = 415, description = "`Content-Type` non JSON (rejet de l'extracteur, corps texte)"),
        (status = 422, description = "Champs manquants ou mal typés (rejet de l'extracteur, corps texte)"),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    ValidatedJson(req): ValidatedJson<CreateOrganizationRequest>,
) -> Result<(StatusCode, Json<OrgDto>), AppError> {
    // Org + membership admin dans UNE transaction : pas d'org orpheline (sans
    // admin) possible, même en cas d'échec à mi-chemin.
    let mut tx = state.pg.begin().await?;

    // Slug déjà pris → 23505 sur organizations_slug_key → 409 (mapping error.rs).
    let org_id: i64 = sqlx::query_scalar(
        "INSERT INTO organizations (name, slug, segment) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(req.name.trim())
    .bind(&req.slug)
    .bind(req.segment.as_deref())
    .fetch_one(tx.as_mut())
    .await?;

    // Le fondateur devient admin — l'id du rôle est résolu par code (référentiel
    // RBAC seedé). created_by/membership viennent du JWT, JAMAIS du corps.
    let membership = sqlx::query(
        "INSERT INTO memberships (org_id, user_id, role_id)
         SELECT $1, $2, id FROM roles WHERE code = 'admin'",
    )
    .bind(org_id)
    .bind(user.user_id)
    .execute(tx.as_mut())
    .await?;
    if membership.rows_affected() == 0 {
        return Err(AppError::Internal(anyhow::anyhow!(
            "rôle admin absent du référentiel roles (seed incomplète ?)"
        )));
    }

    let org = fetch_one(tx.as_mut(), user.user_id, org_id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("org créée introuvable")))?;
    tx.commit().await?;

    Ok((StatusCode::CREATED, Json(org)))
}

/// Détail d'une organisation dont l'appelant est membre.
#[utoipa::path(
    get,
    path = "/api/organizations/{id}",
    tag = "organizations",
    params(("id" = i64, Path, description = "Identifiant de l'organisation")),
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Détail de l'org (`my_role` = rôle de l'appelant dans CETTE org)", body = OrgDto),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu, org soft-supprimée — ou org dont l'appelant n'est PAS membre (anti-énumération : indistinguables, `organization_not_found`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<OrgDto>, AppError> {
    let mut conn = state.pg.acquire().await?;
    let org = fetch_one(&mut conn, user.user_id, id)
        .await?
        .ok_or(AppError::NotFound("organization_not_found"))?;
    Ok(Json(org))
}

/// Modifie nom / segment d'une organisation (admin de CETTE org requis).
#[utoipa::path(
    patch,
    path = "/api/organizations/{id}",
    tag = "organizations",
    params(("id" = i64, Path, description = "Identifiant de l'organisation")),
    request_body = UpdateOrganizationRequest,
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Organisation modifiée (état après mutation)", body = OrgDto),
        (status = 400, description = "Validation échouée ou aucun champ fourni (corps `ErrorBody`) — ou JSON malformé (corps texte)", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "MEMBRE de l'org sans rôle admin (`admin_required`) — un non-membre reçoit 404, jamais 403", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu, org soft-supprimée ou appelant non membre (`organization_not_found`)", body = crate::openapi::ErrorBody),
        (status = 413, description = "Corps dépassant la borne des routes CRUD (64 Kio) — rejeté avant tout travail (corps texte)"),
        (status = 415, description = "`Content-Type` non JSON (rejet de l'extracteur, corps texte)"),
        (status = 422, description = "Champs mal typés (rejet de l'extracteur, corps texte)"),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    ValidatedJson(req): ValidatedJson<UpdateOrganizationRequest>,
) -> Result<Json<OrgDto>, AppError> {
    if req.name.is_none() && req.segment.is_none() {
        return Err(AppError::BadRequest(
            "aucun champ à modifier (attendus : name, segment — le slug est immuable)".into(),
        ));
    }

    // Rôle RE-RÉSOLU en base pour CETTE org (les claims ne valent que pour l'org
    // du JWT). Ordre des refus : non-membre / org inconnue / org soft-supprimée
    // ⇒ 404 D'ABORD — le 403 ne s'adresse qu'à un MEMBRE non admin, il ne peut
    // donc pas servir d'oracle d'existence sur une org étrangère.
    let role = crate::db::fetch_role_in_org(&state.pg, user.user_id, id)
        .await?
        .ok_or(AppError::NotFound("organization_not_found"))?;
    if role.role_code != "admin" {
        return Err(AppError::Forbidden("admin_required"));
    }

    let mut tx = state.pg.begin().await?;
    let updated = sqlx::query(
        r#"
        UPDATE organizations SET
            name       = COALESCE($2, name),
            segment    = CASE WHEN $3::text IS NULL THEN segment
                              ELSE NULLIF(trim($3), '') END,
            updated_at = now()
        WHERE id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(id)
    .bind(req.name.as_deref().map(str::trim))
    .bind(req.segment.as_deref())
    .execute(tx.as_mut())
    .await?;
    if updated.rows_affected() == 0 {
        // Course : org soft-supprimée entre la résolution du rôle et l'UPDATE.
        return Err(AppError::NotFound("organization_not_found"));
    }

    let org = fetch_one(tx.as_mut(), user.user_id, id)
        .await?
        .ok_or(AppError::NotFound("organization_not_found"))?;
    tx.commit().await?;
    Ok(Json(org))
}

/// Soft-supprime une organisation (admin de CETTE org requis).
///
/// Conséquences (testées e2e) : l'org disparaît de tous les listings/lookups,
/// les logins/refresh de ses membres meurent (`db.rs` filtre `deleted_at`), et
/// les `alert_events` sont PRÉSERVÉS — aucune suppression physique (FK RESTRICT).
#[utoipa::path(
    delete,
    path = "/api/organizations/{id}",
    tag = "organizations",
    params(("id" = i64, Path, description = "Identifiant de l'organisation")),
    security(("bearer_jwt" = [])),
    responses(
        (status = 204, description = "Org soft-supprimée : logins/refresh de ses membres révoqués de fait, alert_events préservés (aucune suppression physique)"),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "MEMBRE de l'org sans rôle admin (`admin_required`)", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu, org DÉJÀ soft-supprimée ou appelant non membre (`organization_not_found`) — rejouer un DELETE rend 404", body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    // Même garde que PATCH : 404 (invisible) avant 403 (membre non admin).
    // Une org déjà soft-supprimée est exclue par fetch_role_in_org ⇒ 404 direct.
    let role = crate::db::fetch_role_in_org(&state.pg, user.user_id, id)
        .await?
        .ok_or(AppError::NotFound("organization_not_found"))?;
    if role.role_code != "admin" {
        return Err(AppError::Forbidden("admin_required"));
    }

    // SOFT-delete : `deleted_at IS NULL` dans le WHERE rend l'opération sûre
    // face aux courses (deux DELETE concurrents : un seul gagne, l'autre 404).
    let deleted = sqlx::query(
        "UPDATE organizations SET deleted_at = now(), updated_at = now()
         WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .execute(&state.pg)
    .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound("organization_not_found"));
    }

    Ok(StatusCode::NO_CONTENT)
}
