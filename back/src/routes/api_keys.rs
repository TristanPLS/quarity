//! CRUD `/api/api-keys` (B9b-1) — clés API d'une organisation (US-11).
//!
//! Une clé API permet à terme un accès **lecture seule** à l'API publique (B9b-2),
//! authentifié par la clé (pas un JWT), avec quota par abonnement.
//!
//! Sécurité :
//! - Le **secret** (`qrt_<48 hex>`) n'est renvoyé qu'**UNE fois**, à la création — seul
//!   son **hash SHA-256** est stocké (`api_tokens.token_hash`), jamais le secret en clair
//!   (US-11 c2). Le listing n'expose que des **métadonnées** (dont `token_prefix`, 8
//!   premiers caractères en clair pour reconnaître la clé).
//! - **Émission/listing/révocation réservés au rôle admin** (`RequireAdmin` ⇒ 403
//!   `admin_required`) : ce sont des credentials d'organisation.
//! - **Isolation** : toute requête est scopée `org_id = <org du JWT>` — une clé d'une
//!   autre org est invisible ⇒ **404**.
//! - **Révocation** = `revoked_at = now()` (soft) : immédiate (la clé ne s'authentifiera
//!   plus en B9b-2), tout en gardant la trace (US-11 c3).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use crate::error::AppError;
use crate::security::{generate_api_key, RequireAdmin};
use crate::state::AppState;
use crate::validation::ValidatedJson;

/// Scopes acceptés — miroir du CHECK `api_tokens.scope`. `read_write` reste un point
/// d'extension (US-11 émet du lecture seule).
fn validate_api_scope(s: &str) -> Result<(), validator::ValidationError> {
    if matches!(s, "read" | "read_write") {
        return Ok(());
    }
    Err(validator::ValidationError::new("scope")
        .with_message("valeurs acceptées : read, read_write".into()))
}

// ─────────────────────────────────────────────────────────────────────────────
// DTOs
// ─────────────────────────────────────────────────────────────────────────────

/// Métadonnées d'une clé (JAMAIS le secret ni son hash).
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct ApiKeyDto {
    pub id: i64,
    #[schema(example = "Intégration tableau de bord ville")]
    pub name: String,
    /// 8 premiers caractères du secret, en clair (pour reconnaître la clé).
    #[schema(example = "qrt_a1b2")]
    pub token_prefix: String,
    #[schema(example = "read")]
    pub scope: String,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    /// Non nul ⇒ clé révoquée (ne s'authentifie plus).
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Réponse de création : la clé + le **secret en clair, à copier maintenant** (non
/// récupérable ensuite).
#[derive(Debug, Serialize, ToSchema)]
pub struct CreatedApiKey {
    #[serde(flatten)]
    pub api_key: ApiKeyDto,
    /// Secret COMPLET — affiché UNE seule fois. Stocké uniquement hashé côté serveur.
    #[schema(example = "qrt_a1b2c3d4e5f6...")]
    pub secret: String,
}

/// Corps de `POST /api/api-keys`.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct CreateApiKeyRequest {
    /// Libellé (1..=100, non blanc).
    #[validate(
        length(min = 1, max = 100, message = "longueur attendue 1..=100"),
        custom(function = "crate::validation::validate_not_blank")
    )]
    pub name: String,
    /// Scope (défaut `read`).
    #[validate(custom(function = "validate_api_scope"))]
    pub scope: Option<String>,
    /// Expiration en jours (1..=3650) ; absent ⇒ pas d'expiration.
    #[validate(range(min = 1, max = 3650, message = "expires_in_days attendu 1..=3650"))]
    pub expires_in_days: Option<i64>,
}

const DTO_COLUMNS: &str =
    "id, name, token_prefix, scope, last_used_at, expires_at, revoked_at, created_at";

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// Émet une nouvelle clé API pour l'org de l'appelant. Le secret n'est renvoyé qu'ICI.
#[utoipa::path(
    post,
    path = "/api/api-keys",
    tag = "api-keys",
    request_body = CreateApiKeyRequest,
    security(("bearer_jwt" = [])),
    responses(
        (status = 201, description = "Clé créée — `secret` à copier MAINTENANT (non récupérable)", body = CreatedApiKey),
        (status = 400, description = "Validation du corps échouée", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle non admin (`admin_required`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    RequireAdmin(user): RequireAdmin,
    ValidatedJson(req): ValidatedJson<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreatedApiKey>), AppError> {
    let (secret, prefix, hash) = generate_api_key();
    let expires_at = req.expires_in_days.map(|d| Utc::now() + Duration::days(d));
    let scope = req.scope.as_deref().unwrap_or("read");

    // org_id et created_by viennent du JWT, jamais du corps. token_hash UNIQUE — une
    // collision SHA-256 (astronomiquement improbable) remonterait en 409 (mapping auto).
    let new_id: i64 = sqlx::query_scalar(
        "INSERT INTO api_tokens (org_id, created_by, name, token_prefix, token_hash, scope, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(user.org_id)
    .bind(user.user_id)
    .bind(req.name.trim())
    .bind(&prefix)
    .bind(&hash)
    .bind(scope)
    .bind(expires_at)
    .fetch_one(&state.pg)
    .await?;

    let api_key = sqlx::query_as::<_, ApiKeyDto>(&format!(
        "SELECT {DTO_COLUMNS} FROM api_tokens WHERE id = $1"
    ))
    .bind(new_id)
    .fetch_one(&state.pg)
    .await?;

    Ok((StatusCode::CREATED, Json(CreatedApiKey { api_key, secret })))
}

/// Liste les clés API de l'org (métadonnées seulement — jamais de secret/hash).
#[utoipa::path(
    get,
    path = "/api/api-keys",
    tag = "api-keys",
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Clés de l'org (vives et révoquées), récentes d'abord", body = [ApiKeyDto]),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle non admin (`admin_required`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    RequireAdmin(user): RequireAdmin,
) -> Result<Json<Vec<ApiKeyDto>>, AppError> {
    let keys = sqlx::query_as::<_, ApiKeyDto>(&format!(
        "SELECT {DTO_COLUMNS} FROM api_tokens WHERE org_id = $1 ORDER BY created_at DESC, id DESC"
    ))
    .bind(user.org_id)
    .fetch_all(&state.pg)
    .await?;
    Ok(Json(keys))
}

/// Révoque une clé API de l'org (immédiat : elle ne s'authentifie plus). Idempotent côté
/// effet, mais une clé déjà révoquée ou inconnue renvoie 404.
#[utoipa::path(
    delete,
    path = "/api/api-keys/{id}",
    tag = "api-keys",
    params(("id" = i64, Path, description = "Identifiant de la clé")),
    security(("bearer_jwt" = [])),
    responses(
        (status = 204, description = "Clé révoquée"),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
        (status = 403, description = "Rôle non admin (`admin_required`)", body = crate::openapi::ErrorBody),
        (status = 404, description = "Clé inconnue, déjà révoquée ou d'une autre org (`api_key_not_found`)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn revoke(
    State(state): State<AppState>,
    RequireAdmin(user): RequireAdmin,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let revoked = sqlx::query(
        "UPDATE api_tokens SET revoked_at = now() \
         WHERE id = $1 AND org_id = $2 AND revoked_at IS NULL",
    )
    .bind(id)
    .bind(user.org_id)
    .execute(&state.pg)
    .await?;
    if revoked.rows_affected() == 0 {
        return Err(AppError::NotFound("api_key_not_found"));
    }
    Ok(StatusCode::NO_CONTENT)
}
