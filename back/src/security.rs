//! Sécurité : hash/vérif Argon2id, JWT HS256, extractor d'authentification.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::HeaderMap;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
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

/// Hash factice constant (calculé une seule fois, mêmes paramètres que les vrais hashes).
/// Sert UNIQUEMENT à la vérification factice anti-énumération ci-dessous.
static DUMMY_PHC_HASH: LazyLock<String> = LazyLock::new(|| {
    hash_password("quarity-dummy-anti-enumeration")
        .expect("hash argon2 factice (paramètres par défaut — ne peut pas échouer)")
});

/// Vérification Argon2 factice : égalise le temps de réponse du login quand l'email est
/// INCONNU (ou le compte inactif), pour qu'un attaquant ne puisse pas distinguer
/// « email inexistant » de « mauvais mot de passe » au chronomètre. Résultat ignoré.
pub fn dummy_verify_password(password: &str) {
    let _ = verify_password(password, &DUMMY_PHC_HASH);
}

/// IP cliente best-effort pour le rate-limit, SANS `ConnectInfo` (le routeur est servi via
/// `axum::serve(listener, app)` dans `main.rs` — hors périmètre de ce lot — donc pas de
/// `into_make_service_with_connect_info`, l'adresse du peer est inaccessible ici).
///
/// Priorité :
/// 1. `X-Real-IP` — écrasé inconditionnellement par notre nginx (`$remote_addr`),
///    donc NON falsifiable à travers le proxy ;
/// 2. premier élément de `X-Forwarded-For` (repli, falsifiable car nginx APPEND
///    via `$proxy_add_x_forwarded_for`) ;
/// 3. « unknown » sinon (accès direct au back sans proxy).
///
/// Limite documentée : en accès direct au port du back (sans nginx devant), ces en-têtes
/// sont forgeables → le rate-limit par IP est best-effort ; celui par email n'en dépend pas.
pub fn client_ip(headers: &HeaderMap) -> String {
    if let Some(ip) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        let ip = ip.trim();
        if !ip.is_empty() {
            return ip.to_string();
        }
    }
    if let Some(first) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|xff| xff.split(',').next())
    {
        let first = first.trim();
        if !first.is_empty() {
            return first.to_string();
        }
    }
    "unknown".into()
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

/// Garde d'écriture (B6) : `AuthUser` + refus 403 si `can_write = false` (rôle `lecteur`).
///
/// À utiliser comme TYPE D'ARGUMENT de tout handler de mutation (POST/PATCH/DELETE) :
/// l'isolation ne dépend plus d'un `if` que chaque handler pourrait oublier — un
/// handler de mutation qui prend `CanWrite` ne compile pas sans la vérification.
/// Code d'erreur stable : `read_only_role`.
#[derive(Debug, Clone)]
pub struct CanWrite(pub AuthUser);

impl FromRequestParts<AppState> for CanWrite {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        if !user.can_write {
            return Err(AppError::Forbidden("read_only_role"));
        }
        Ok(CanWrite(user))
    }
}

/// Garde d'administration (B6) : réservé au rôle `admin` de l'org du JWT
/// (gestion des membres, de l'organisation). Code d'erreur stable : `admin_required`.
///
/// Le rôle provient des claims (signés) — pour une opération sur une AUTRE org que
/// celle du JWT, le rôle doit être re-résolu en base via `db::fetch_role_in_org`.
#[derive(Debug, Clone)]
pub struct RequireAdmin(pub AuthUser);

impl FromRequestParts<AppState> for RequireAdmin {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        if user.role != "admin" {
            return Err(AppError::Forbidden("admin_required"));
        }
        Ok(RequireAdmin(user))
    }
}

#[cfg(test)]
mod tests {
    use super::{client_ip, dummy_verify_password};
    use axum::http::HeaderMap;

    #[test]
    fn client_ip_prefers_x_real_ip() {
        let mut h = HeaderMap::new();
        h.insert("x-real-ip", "203.0.113.7".parse().unwrap());
        h.insert("x-forwarded-for", "198.51.100.1, 10.0.0.2".parse().unwrap());
        assert_eq!(client_ip(&h), "203.0.113.7");
    }

    #[test]
    fn client_ip_falls_back_to_first_xff_element() {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", "198.51.100.1, 10.0.0.2".parse().unwrap());
        assert_eq!(client_ip(&h), "198.51.100.1");
    }

    #[test]
    fn client_ip_unknown_without_headers() {
        assert_eq!(client_ip(&HeaderMap::new()), "unknown");
    }

    #[test]
    fn client_ip_ignores_empty_headers() {
        let mut h = HeaderMap::new();
        h.insert("x-real-ip", "  ".parse().unwrap());
        h.insert("x-forwarded-for", "".parse().unwrap());
        assert_eq!(client_ip(&h), "unknown");
    }

    #[test]
    fn dummy_verify_never_panics() {
        // Vérification factice : doit s'exécuter sans panic, quel que soit le mot de passe.
        dummy_verify_password("n'importe quoi");
        dummy_verify_password("");
    }
}
