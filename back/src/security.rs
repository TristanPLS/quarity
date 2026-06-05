//! Sécurité : hash/vérif Argon2id, JWT HS256, extractor d'authentification.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::AppError;
use crate::state::AppState;

/// Vérifie un mot de passe contre un hash PHC `$argon2id$...`. Jamais de panic.
pub fn verify_password(password: &str, phc_hash: &str) -> bool {
    match PasswordHash::new(phc_hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Génère un hash PHC argon2id (params OWASP par défaut). Utilisé par le helper CLI `hash`.
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("argon2 hash: {e}"))?;
    Ok(hash.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,        // user_id
    pub org_id: i64,     // isolation multi-tenant (jamais d'un paramètre client)
    pub role: String,    // roles.code
    pub can_write: bool, // roles.can_write
    pub jti: String,     // identifiant du token (lié au refresh Redis)
    pub iat: i64,
    pub exp: i64,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn issue_access_token(
    secret: &[u8],
    user_id: i64,
    org_id: i64,
    role: &str,
    can_write: bool,
    jti: &str,
    ttl_secs: i64,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = now_secs();
    let claims = Claims {
        sub: user_id,
        org_id,
        role: role.to_owned(),
        can_write,
        jti: jti.to_owned(),
        iat: now,
        exp: now + ttl_secs,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret),
    )
}

fn validation() -> Validation {
    let mut v = Validation::new(Algorithm::HS256);
    v.leeway = 5;
    v
}

/// Identité authentifiée, dérivée UNIQUEMENT des claims JWT.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: i64,
    pub org_id: i64,
    pub role: String,
    pub can_write: bool,
    pub jti: String,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .ok_or(AppError::Unauthorized("missing_bearer"))?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or(AppError::Unauthorized("missing_bearer"))?;

        let key = DecodingKey::from_secret(state.cfg.jwt_secret.as_bytes());
        let data = decode::<Claims>(token, &key, &validation()).map_err(|e| {
            use jsonwebtoken::errors::ErrorKind::ExpiredSignature;
            match e.kind() {
                ExpiredSignature => AppError::Unauthorized("token_expired"),
                _ => AppError::Unauthorized("invalid_token"),
            }
        })?;
        let c = data.claims;

        Ok(AuthUser {
            user_id: c.sub,
            org_id: c.org_id,
            role: c.role,
            can_write: c.can_write,
            jti: c.jti,
        })
    }
}
