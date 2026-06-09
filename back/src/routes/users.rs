//! CRUD `/api/users` (B6) — membres de l'organisation du JWT.
//!
//! La ressource exposée n'est PAS « la table `users` » mais « les MEMBRES de l'org
//! de l'appelant » : un compte est GLOBAL (multi-org via `memberships`), seule la
//! membership est tenant-scopée. Conventions du gabarit (`tracked_locations`),
//! plus les choix propres à la ressource :
//! - **Isolation multi-tenant par construction** : CHAQUE requête SQL est filtrée
//!   par `EXISTS (membership dans l'org du JWT)` — un compte d'une autre org est
//!   invisible et répond **404** `user_not_found` (anti-énumération : un id
//!   étranger est indistinguable d'un id inexistant ; même code stable que
//!   `/api/auth/me`). Le 403 est réservé aux refus de RÔLE.
//! - **Mutations réservées au rôle `admin`** ([`RequireAdmin`], 403 `admin_required`,
//!   dans la signature des handlers) : gérer les membres relève de l'administration
//!   de l'org, pas du simple `can_write` — le `gestionnaire` édite lieux et règles,
//!   pas les comptes.
//! - **DELETE retire la MEMBERSHIP, jamais le compte** : le compte peut appartenir
//!   à d'autres orgs — le supprimer (ou le désactiver via `is_active`) depuis une
//!   org serait un débordement cross-tenant. Un compte sans plus aucune membership
//!   ne peut simplement plus se connecter : le login exige une membership.
//! - **Garde-fous anti-verrouillage (400)** : un admin ne modifie pas SON propre
//!   rôle et ne SE retire pas lui-même — une org ne doit pas perdre son admin
//!   par mégarde.
//! - **Unicité d'email GLOBALE** (`users_email_key`) : créer un email déjà pris —
//!   y compris par un compte d'une AUTRE org — répond 409. « Inviter » un compte
//!   existant dans une org supplémentaire (membership sans création de compte)
//!   est une évolution future, hors périmètre B6.
//! - **Mot de passe à la création : 12..=512** — plus strict que la borne du login
//!   (1..=512) : ici on CRÉE un secret, là-bas on en VÉRIFIE un d'existant.
//! - Pas de GUC d'audit ici : T5 n'audite que `alert_rules`, qu'aucune mutation
//!   de ce module ne touche.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use validator::Validate;

use crate::error::AppError;
use crate::listing::ListParams;
use crate::security::{AuthUser, RequireAdmin};
use crate::state::AppState;
use crate::validation::{ValidatedJson, ValidatedQuery};

// ─────────────────────────────────────────────────────────────────────────────
// DTOs
// ─────────────────────────────────────────────────────────────────────────────

/// Membre de l'org du JWT : compte global + rôle/entrée dans CETTE org.
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct UserDto {
    pub id: i64,
    #[schema(example = "claire@agglo-riviera.fr")]
    pub email: String,
    #[schema(example = "Claire Dubois")]
    pub full_name: String,
    /// `false` = compte désactivé GLOBALEMENT (plus de login). État du COMPTE,
    /// pas de la membership — non éditable ici (cf. doc de module).
    pub is_active: bool,
    /// Code du rôle dans l'org du JWT (`admin`, `gestionnaire`, `lecteur`).
    #[schema(example = "lecteur")]
    pub role: String,
    /// Droit d'écriture du rôle (`roles.can_write` — `false` ⇒ 403 sur mutation).
    pub can_write: bool,
    /// Date d'entrée dans l'org (`memberships.created_at`).
    pub joined_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Page de membres (`{ page, page_size, count, total, data }`).
#[derive(Debug, Serialize, ToSchema)]
pub struct UsersPage {
    pub page: u32,
    pub page_size: u32,
    /// Taille de `data`.
    pub count: usize,
    /// Total filtré (toutes pages confondues).
    pub total: i64,
    pub data: Vec<UserDto>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Requêtes entrantes
// ─────────────────────────────────────────────────────────────────────────────

/// Filtres propres au listing des membres (s'ajoutent à [`ListParams`]).
#[derive(Debug, Deserialize, IntoParams, Validate)]
#[into_params(parameter_in = Query)]
pub struct UserFilters {
    /// Ne renvoyer que les membres portant ce rôle dans l'org
    /// (allowlist : `admin`, `gestionnaire`, `lecteur`).
    #[validate(custom(function = "crate::validation::validate_role_code"))]
    pub role: Option<String>,
    /// Ne renvoyer que les comptes actifs (`true`) ou désactivés (`false`).
    pub is_active: Option<bool>,
}

/// Corps de `POST /api/users`.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct CreateUserRequest {
    /// Adresse de connexion, unique GLOBALEMENT (254 max — longueur RFC d'une
    /// adresse). Normalisée trim + minuscules avant insertion (CITEXT en base).
    #[validate(
        email(message = "format d'email invalide"),
        length(max = 254, message = "254 caractères maximum")
    )]
    #[schema(example = "claire@agglo-riviera.fr")]
    pub email: String,
    /// Nom affiché (1..=200, non blanc).
    #[validate(
        length(min = 1, max = 200, message = "longueur attendue 1..=200"),
        custom(function = "crate::validation::validate_not_blank")
    )]
    #[schema(example = "Claire Dubois")]
    pub full_name: String,
    /// Mot de passe initial (12..=512) — politique de CRÉATION d'un secret,
    /// volontairement plus stricte que la borne de vérification du login
    /// (cf. doc de module). Plafond anti-DoS identique au login (Argon2 coûteux).
    #[validate(length(min = 12, max = 512, message = "longueur attendue 12..=512"))]
    #[schema(example = "Quarity2026!")]
    pub password: String,
    /// Rôle dans l'org du JWT (allowlist : `admin`, `gestionnaire`, `lecteur`).
    #[validate(custom(function = "crate::validation::validate_role_code"))]
    #[schema(example = "lecteur")]
    pub role: String,
}

/// Corps de `PATCH /api/users/{id}` — champs absents = inchangés. Aucun champ
/// TEXT optionnel ici : pas de sémantique d'effacement par chaîne vide
/// (`full_name` est NOT NULL — une chaîne vide est refusée au boundary).
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct UpdateUserRequest {
    #[validate(
        length(min = 1, max = 200, message = "longueur attendue 1..=200"),
        custom(function = "crate::validation::validate_not_blank")
    )]
    pub full_name: Option<String>,
    /// Nouveau rôle dans l'org du JWT — refusé sur SOI-MÊME (400, cf. doc de module).
    #[validate(custom(function = "crate::validation::validate_role_code"))]
    pub role: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// SQL (scopé membership de l'org du JWT — JAMAIS de requête sans ce filtre)
// ─────────────────────────────────────────────────────────────────────────────

/// Colonnes du DTO — sous-requêtes corrélées (pas de JOIN dans le FROM du SELECT
/// paginé) : `id` reste non ambigu pour le tri. Les trois sous-requêtes référencent
/// **`$1` = org du JWT** : toute requête qui interpole `DTO_COLUMNS` doit binder
/// l'org en PREMIER paramètre. `UNIQUE (org_id, user_id)` garantit au plus une
/// ligne par sous-requête, et le filtre `EXISTS (membership)` des appelants
/// garantit qu'elles n'en renvoient jamais zéro (donc jamais de NULL à décoder).
const DTO_COLUMNS: &str = r#"
    id, email::text AS email, full_name, is_active,
    (SELECT r.code FROM memberships m JOIN roles r ON r.id = m.role_id
      WHERE m.user_id = users.id AND m.org_id = $1)                        AS role,
    (SELECT r.can_write FROM memberships m JOIN roles r ON r.id = m.role_id
      WHERE m.user_id = users.id AND m.org_id = $1)                        AS can_write,
    (SELECT m.created_at FROM memberships m
      WHERE m.user_id = users.id AND m.org_id = $1)                        AS joined_at,
    created_at, updated_at
"#;

/// Allowlist de tri : `nom_api → colonne SQL`. `joined_at` référence l'ALIAS de la
/// sous-requête de `DTO_COLUMNS` (Postgres résout les alias de SORTIE dans un
/// ORDER BY, mais seulement en référence nue — pas dans une expression) : il n'est
/// utilisable que parce que la requête paginée sélectionne `DTO_COLUMNS`.
const SORT_ALLOW: &[(&str, &str)] = &[
    ("email", "lower(email::text)"),
    ("full_name", "lower(full_name)"),
    ("created_at", "created_at"),
    ("joined_at", "joined_at"),
];

/// Charge un MEMBRE de l'org du JWT — le `EXISTS` est LE filtre d'isolation
/// (compte inexistant et compte d'une autre org : même `None`, donc même 404).
async fn fetch_member(
    conn: &mut sqlx::PgConnection,
    org_id: i64,
    user_id: i64,
) -> Result<Option<UserDto>, sqlx::Error> {
    let sql = format!(
        "SELECT {DTO_COLUMNS} FROM users WHERE id = $2 \
         AND EXISTS (SELECT 1 FROM memberships m \
                     WHERE m.user_id = users.id AND m.org_id = $1)"
    );
    sqlx::query_as::<_, UserDto>(&sql)
        .bind(org_id)
        .bind(user_id)
        .fetch_optional(&mut *conn)
        .await
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// Liste paginée des membres de l'org de l'appelant.
#[utoipa::path(
    get,
    path = "/api/users",
    tag = "users",
    params(ListParams, UserFilters),
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Page de membres (tri par défaut : `full_name` croissant)", body = UsersPage),
        (status = 400, description = "Pagination/tri/filtre invalide (corps `ErrorBody`, ou rejet `Query` en corps texte)", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
    ValidatedQuery(filters): ValidatedQuery<UserFilters>,
) -> Result<Json<UsersPage>, AppError> {
    let p = params.pagination();
    let order_by = params.order_by(SORT_ALLOW, "full_name")?;
    let like = params.like_pattern();

    // `$n::type IS NULL OR …` : un filtre absent est bindé NULL et neutralisé —
    // une seule requête préparée quel que soit le jeu de filtres. Le filtre `role`
    // re-scope son EXISTS sur l'org du JWT : le rôle d'un compte dans une AUTRE
    // org ne doit jamais influencer le listing de celle-ci.
    let where_clause = r#"
        WHERE EXISTS (SELECT 1 FROM memberships m
                      WHERE m.user_id = users.id AND m.org_id = $1)
          AND ($2::text IS NULL OR email::text ILIKE $2 OR full_name ILIKE $2)
          AND ($3::text IS NULL OR EXISTS (
                SELECT 1 FROM memberships m JOIN roles r ON r.id = m.role_id
                WHERE m.user_id = users.id AND m.org_id = $1 AND r.code = $3))
          AND ($4::boolean IS NULL OR is_active = $4)
    "#;

    let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM users {where_clause}"))
        .bind(user.org_id)
        .bind(like.as_deref())
        .bind(filters.role.as_deref())
        .bind(filters.is_active)
        .fetch_one(&state.pg)
        .await?;

    let data = sqlx::query_as::<_, UserDto>(&format!(
        "SELECT {DTO_COLUMNS} FROM users {where_clause} {order_by} LIMIT $5 OFFSET $6"
    ))
    .bind(user.org_id)
    .bind(like.as_deref())
    .bind(filters.role.as_deref())
    .bind(filters.is_active)
    .bind(p.limit)
    .bind(p.offset)
    .fetch_all(&state.pg)
    .await?;

    Ok(Json(UsersPage {
        page: p.page,
        page_size: p.page_size,
        count: data.len(),
        total,
        data,
    }))
}

/// Crée un compte et l'attache à l'org de l'appelant (membership + rôle), atomiquement.
#[utoipa::path(
    post,
    path = "/api/users",
    tag = "users",
    request_body = CreateUserRequest,
    security(("bearer_jwt" = [])),
    responses(
        (status = 201, description = "Membre créé (compte + membership dans l'org du JWT)", body = UserDto),
        (status = 400, description = "Validation du corps échouée (corps `ErrorBody`) — ou JSON malformé (rejet de l'extracteur, corps texte)", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle non admin (`admin_required`)", body = crate::openapi::ErrorBody),
        (status = 409, description = "Un compte existe déjà pour cet email — y compris dans une autre org (`conflict`)", body = crate::openapi::ErrorBody),
        (status = 413, description = "Corps dépassant la borne des routes CRUD (64 Kio) — rejeté avant tout travail (corps texte)"),
        (status = 415, description = "`Content-Type` non JSON (rejet de l'extracteur, corps texte)"),
        (status = 422, description = "Champs manquants/mal typés (rejet de l'extracteur, corps texte) — ou email passé au boundary mais refusé par le CHECK du schéma", body = crate::openapi::ErrorBody),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    ValidatedJson(req): ValidatedJson<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserDto>), AppError> {
    // Normalisation de l'identifiant — même convention que le login (qui compare
    // en `lower(...)`) : un email est insensible à la casse dans tout le système.
    let email = req.email.trim().to_lowercase();

    // Hash Argon2id SYNCHRONE (~100 ms) : même compromis assumé que la vérification
    // au login — pas de `spawn_blocking` tant que la création de comptes reste un
    // événement rare d'administration. Calculé AVANT d'ouvrir la transaction pour
    // ne pas immobiliser une connexion du pool pendant le hash.
    let password_hash = crate::security::hash_password(&req.password)?;

    let mut tx = state.pg.begin().await?;

    // org_id et invited_by viennent du JWT, JAMAIS du corps (isolation). Email déjà
    // pris — dans N'IMPORTE QUELLE org, l'unicité est globale — → 23505
    // `users_email_key` → 409 (mapping automatique de `error.rs`).
    let new_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, full_name) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(&email)
    .bind(&password_hash)
    .bind(req.full_name.trim())
    .fetch_one(tx.as_mut())
    .await?;

    // Membership dans l'org du JWT, rôle résolu par code (l'allowlist du boundary
    // est le miroir du CHECK `roles.code` : la sous-sélection trouve toujours).
    let inserted = sqlx::query(
        r#"
        INSERT INTO memberships (org_id, user_id, role_id, invited_by)
        SELECT $1, $2, r.id, $3 FROM roles r WHERE r.code = $4
        "#,
    )
    .bind(admin.org_id)
    .bind(new_id)
    .bind(admin.user_id)
    .bind(&req.role)
    .execute(tx.as_mut())
    .await?;
    if inserted.rows_affected() == 0 {
        // Inatteignable tant que `validate_role_code` reflète la table `roles`.
        return Err(AppError::Internal(anyhow::anyhow!(
            "rôle « {} » absent du référentiel malgré l'allowlist",
            req.role
        )));
    }

    let dto = fetch_member(tx.as_mut(), admin.org_id, new_id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("membre créé introuvable")))?;
    tx.commit().await?;

    Ok((StatusCode::CREATED, Json(dto)))
}

/// Détail d'un membre de l'org de l'appelant.
#[utoipa::path(
    get,
    path = "/api/users/{id}",
    tag = "users",
    params(("id" = i64, Path, description = "Identifiant du membre")),
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Détail du membre (rôle et droits dans l'org du JWT)", body = UserDto),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu — ou compte non membre de l'org du JWT (anti-énumération : indistinguables, `user_not_found`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<UserDto>, AppError> {
    let mut conn = state.pg.acquire().await?;
    let dto = fetch_member(&mut conn, user.org_id, id)
        .await?
        .ok_or(AppError::NotFound("user_not_found"))?;
    Ok(Json(dto))
}

/// Modifie le nom affiché et/ou le rôle d'un membre de l'org.
#[utoipa::path(
    patch,
    path = "/api/users/{id}",
    tag = "users",
    params(("id" = i64, Path, description = "Identifiant du membre")),
    request_body = UpdateUserRequest,
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Membre modifié (état après mutation)", body = UserDto),
        (status = 400, description = "Validation échouée, aucun champ fourni, ou modification de SON PROPRE rôle (corps `ErrorBody`) — ou JSON malformé (corps texte)", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle non admin (`admin_required`)", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu ou compte non membre de l'org (`user_not_found`)", body = crate::openapi::ErrorBody),
        (status = 413, description = "Corps dépassant la borne des routes CRUD (64 Kio) — rejeté avant tout travail (corps texte)"),
        (status = 415, description = "`Content-Type` non JSON (rejet de l'extracteur, corps texte)"),
        (status = 422, description = "Champs mal typés (rejet de l'extracteur, corps texte)"),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    Path(id): Path<i64>,
    ValidatedJson(req): ValidatedJson<UpdateUserRequest>,
) -> Result<Json<UserDto>, AppError> {
    if req.full_name.is_none() && req.role.is_none() {
        return Err(AppError::BadRequest(
            "aucun champ à modifier (attendus : full_name, role)".into(),
        ));
    }
    // Garde-fou : modifier SON propre rôle est refusé — un admin qui se rétrograde
    // par mégarde peut laisser l'org sans aucun admin (l'API n'a pas de console de
    // secours). La cible étant soi-même, le 400 prime sur tout le reste.
    if req.role.is_some() && id == admin.user_id {
        return Err(AppError::BadRequest(
            "impossible de modifier son propre rôle (l'organisation ne doit pas perdre son admin)"
                .into(),
        ));
    }

    let mut tx = state.pg.begin().await?;

    if let Some(full_name) = req.full_name.as_deref() {
        // Le compte est global mais la mutation reste conditionnée à la membership
        // dans l'org du JWT : cible inexistante OU d'une autre org ⇒ 0 ligne ⇒ 404.
        let updated = sqlx::query(
            r#"
            UPDATE users SET full_name = $3, updated_at = now()
            WHERE id = $2
              AND EXISTS (SELECT 1 FROM memberships m
                          WHERE m.user_id = users.id AND m.org_id = $1)
            "#,
        )
        .bind(admin.org_id)
        .bind(id)
        .bind(full_name.trim())
        .execute(tx.as_mut())
        .await?;
        if updated.rows_affected() == 0 {
            return Err(AppError::NotFound("user_not_found"));
        }
    }

    if let Some(role) = req.role.as_deref() {
        // Le rôle vit sur la MEMBERSHIP (scopée org + user) — jamais sur le compte.
        // Rôle résolu par code (allowlist miroir du CHECK : toujours trouvé).
        let updated = sqlx::query(
            r#"
            UPDATE memberships SET role_id = (SELECT id FROM roles WHERE code = $3)
            WHERE org_id = $1 AND user_id = $2
            "#,
        )
        .bind(admin.org_id)
        .bind(id)
        .bind(role)
        .execute(tx.as_mut())
        .await?;
        if updated.rows_affected() == 0 {
            return Err(AppError::NotFound("user_not_found"));
        }
    }

    let dto = fetch_member(tx.as_mut(), admin.org_id, id)
        .await?
        .ok_or(AppError::NotFound("user_not_found"))?;
    tx.commit().await?;
    Ok(Json(dto))
}

/// Retire un membre de l'org (supprime la MEMBERSHIP — jamais le compte).
#[utoipa::path(
    delete,
    path = "/api/users/{id}",
    tag = "users",
    params(("id" = i64, Path, description = "Identifiant du membre")),
    security(("bearer_jwt" = [])),
    responses(
        (status = 204, description = "Membership supprimée. Le compte global subsiste ; sans plus aucune membership, il ne peut plus se connecter (le login exige une membership)"),
        (status = 400, description = "Tentative de SE retirer soi-même (corps `ErrorBody`)", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle non admin (`admin_required`)", body = crate::openapi::ErrorBody),
        (status = 404, description = "Id inconnu ou compte non membre de l'org (`user_not_found`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    // Garde-fou symétrique du PATCH de rôle : se retirer soi-même laisserait
    // potentiellement l'org sans admin (et l'appelant sans accès pour réparer).
    if id == admin.user_id {
        return Err(AppError::BadRequest(
            "impossible de se retirer soi-même de l'organisation (l'organisation ne doit pas perdre son admin)"
                .into(),
        ));
    }

    // On supprime la MEMBERSHIP, pas le compte (ni `is_active` : désactiver
    // globalement un compte depuis UNE org serait un débordement cross-tenant —
    // cf. doc de module). Une seule requête : pas besoin de transaction.
    let deleted = sqlx::query("DELETE FROM memberships WHERE org_id = $1 AND user_id = $2")
        .bind(admin.org_id)
        .bind(id)
        .execute(&state.pg)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound("user_not_found"));
    }

    Ok(StatusCode::NO_CONTENT)
}
