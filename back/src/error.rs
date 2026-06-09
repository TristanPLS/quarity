//! Erreur applicative unique → réponse HTTP JSON structurée.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    Unauthorized(&'static str),
    Forbidden(&'static str),
    NotFound(&'static str),
    /// Conflit d'unicité (doublon métier) → 409, code stable `conflict`.
    Conflict(String),
    /// Règle métier violée (triggers `QRT_*`, procédures, CHECK) → 422,
    /// code stable `unprocessable_entity`.
    Unprocessable(String),
    /// Rate-limit dépassé → 429, code stable `rate_limited`, message humain en payload.
    TooManyRequests(&'static str),
    Internal(anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, "bad_request".to_string(), m),
            AppError::Unauthorized(c) => (StatusCode::UNAUTHORIZED, c.to_string(), c.to_string()),
            AppError::Forbidden(c) => (StatusCode::FORBIDDEN, c.to_string(), c.to_string()),
            AppError::NotFound(c) => (StatusCode::NOT_FOUND, c.to_string(), c.to_string()),
            AppError::Conflict(m) => (StatusCode::CONFLICT, "conflict".to_string(), m),
            AppError::Unprocessable(m) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "unprocessable_entity".to_string(),
                m,
            ),
            AppError::TooManyRequests(m) => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited".to_string(),
                m.to_string(),
            ),
            AppError::Internal(e) => {
                tracing::error!(error = ?e, "erreur interne");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error".to_string(),
                    "internal error".to_string(),
                )
            }
        };
        (status, Json(json!({ "error": code, "message": message }))).into_response()
    }
}

/// Mapping des erreurs Postgres vers la sémantique HTTP du CRUD (B6).
///
/// - `23505` (unique_violation) → 409 : un doublon métier (slug d'org, email,
///   nom de lieu par org, règle identique) n'est PAS une erreur serveur.
/// - `23514` (check_violation) → 422 : couvre les triggers métier `QRT_T*`/`QRT_P*`
///   (messages volontairement explicites, écrits pour être montrés) ET les CHECK
///   du schéma qui auraient échappé à la validation au boundary.
/// - `23503` (foreign_key_violation) → 422 : référence inexistante qui a passé
///   les contrôles applicatifs (course concurrente comprise).
///
/// Les requêtes d'auth existantes (SELECT purs) ne peuvent pas produire ces codes :
/// leur comportement (500) est inchangé.
impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        if let Some(db) = e.as_database_error() {
            let msg = db.message().to_string();
            match db.code().as_deref() {
                Some("23505") => return AppError::Conflict(conflict_message(&msg)),
                Some("23514") | Some("23503") => return AppError::Unprocessable(msg),
                _ => {}
            }
        }
        AppError::Internal(e.into())
    }
}

/// Message 409 stable et non bavard : on ne renvoie pas le détail brut de Postgres
/// (noms de colonnes/valeurs), seulement la contrainte en cause si identifiable.
fn conflict_message(raw: &str) -> String {
    for (needle, msg) in [
        (
            "uq_tracked_location_org_name",
            "un lieu suivi porte déjà ce nom dans votre organisation",
        ),
        (
            "uq_alert_rule",
            "une règle identique (lieu, polluant, comparateur, seuil) existe déjà",
        ),
        ("users_email_key", "un compte existe déjà pour cet email"),
        (
            "organizations_slug_key",
            "ce slug d'organisation est déjà pris",
        ),
        (
            "uq_membership_user_org",
            "cet utilisateur est déjà membre de l'organisation",
        ),
    ] {
        if raw.contains(needle) {
            return msg.to_string();
        }
    }
    "la ressource entre en conflit avec une ressource existante".to_string()
}
impl From<redis::RedisError> for AppError {
    fn from(e: redis::RedisError) -> Self {
        AppError::Internal(e.into())
    }
}
impl From<crate::ch::ClickhouseError> for AppError {
    fn from(e: crate::ch::ClickhouseError) -> Self {
        AppError::Internal(e.into())
    }
}
impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(e)
    }
}
