//! Sécurité : hash/vérif Argon2id, JWT HS256, extractor d'authentification.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::HeaderMap;
use chrono::Utc;
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

/// Encode des octets en hexadécimal minuscule.
fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Génère une clé API (B9b) : renvoie `(secret_en_clair, token_prefix, token_hash)`.
/// Le secret (`qrt_<48 hex>`, 192 bits) n'est montré qu'UNE fois à la création ; seul le
/// **hash** est stocké. Haute entropie ⇒ **SHA-256** (déterministe, lookup O(1) via
/// `api_tokens.token_hash`) — argon2 (salé/lent) est réservé aux mots de passe.
pub fn generate_api_key() -> (String, String, String) {
    use argon2::password_hash::rand_core::RngCore;
    let mut bytes = [0u8; 24];
    OsRng.fill_bytes(&mut bytes);
    let secret = format!("qrt_{}", to_hex(&bytes));
    let prefix = secret[..8].to_string(); // "qrt_" + 4 hex — en clair pour l'affichage
    let hash = hash_api_key(&secret);
    (secret, prefix, hash)
}

/// Hash SHA-256 (hex) d'une clé API présentée — sert au lookup `api_tokens.token_hash`.
pub fn hash_api_key(secret: &str) -> String {
    use sha2::{Digest, Sha256};
    to_hex(&Sha256::digest(secret.as_bytes()))
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

/// Décode et valide un access token (HS256, expiration + leeway). Erreurs stables :
/// `token_expired` / `invalid_token`. Partagé par l'extracteur `AuthUser` (en-tête
/// Bearer) ET l'endpoint WebSocket B8 (jeton en query string — un navigateur ne
/// peut pas poser d'en-tête Authorization sur une WebSocket native).
pub fn decode_access_token(secret: &str, token: &str) -> Result<Claims, AppError> {
    use jsonwebtoken::errors::ErrorKind::ExpiredSignature;
    let key = DecodingKey::from_secret(secret.as_bytes());
    decode::<Claims>(token, &key, &validation())
        .map(|data| data.claims)
        .map_err(|e| match e.kind() {
            ExpiredSignature => AppError::Unauthorized("token_expired"),
            _ => AppError::Unauthorized("invalid_token"),
        })
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

        let c = decode_access_token(&state.cfg.jwt_secret, token)?;

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

/// Identité dérivée d'une **clé API** (B9b-2), pour l'API publique. Résolue depuis
/// l'en-tête `X-API-Key: qrt_…` (distinct du Bearer JWT) :
/// 1. hash SHA-256 → lookup d'une clé NON révoquée (index partiel) ; rejet expirée → **401** ;
/// 2. **rate-limit/minute** + **quota mensuel** (compteurs Redis, fenêtre fixe) selon le plan
///    de l'abonnement ACTIF de l'org (défaut « free » si aucun) — `0 = illimité` ; dépassement → **429** ;
/// 3. `last_used_at` mis à jour best-effort.
///
/// L'`org_id` vient de la CLÉ, jamais d'un paramètre client (isolation multi-tenant).
pub struct ApiKeyAuth {
    pub org_id: i64,
    pub scope: String,
    pub token_id: i64,
}

impl FromRequestParts<AppState> for ApiKeyAuth {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let presented = parts
            .headers
            .get("x-api-key")
            .and_then(|h| h.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or(AppError::Unauthorized("missing_api_key"))?;

        // Clé VIVE (non révoquée) par hash — sert l'index partiel unique sur token_hash.
        let token_hash = hash_api_key(presented);
        let row: Option<(i64, i64, String, Option<chrono::DateTime<Utc>>)> = sqlx::query_as(
            "SELECT id, org_id, scope, expires_at FROM api_tokens \
             WHERE token_hash = $1 AND revoked_at IS NULL",
        )
        .bind(&token_hash)
        .fetch_optional(&state.pg)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
        let (token_id, org_id, scope, expires_at) =
            row.ok_or(AppError::Unauthorized("invalid_api_key"))?;

        if matches!(expires_at, Some(exp) if exp < Utc::now()) {
            return Err(AppError::Unauthorized("api_key_expired"));
        }

        // Limites du plan de l'abonnement ACTIF (défaut free si aucun). 0 = illimité.
        let (monthly_quota, rate_per_min): (i32, i32) = sqlx::query_as(
            "SELECT sp.monthly_request_quota, sp.rate_limit_per_min \
             FROM organization_subscriptions os \
             JOIN subscription_plans sp ON sp.id = os.plan_id \
             WHERE os.org_id = $1 AND os.status = 'active'",
        )
        .bind(org_id)
        .fetch_optional(&state.pg)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .unwrap_or((1000, 30));

        // Compteurs Redis « fenêtre fixe » par clé : rate-limit/min, puis quota du mois
        // courant (clé suffixée du mois → repart de zéro au changement de mois).
        // POLITIQUE assumée : TOUTE requête AUTHENTIFIÉE compte — y compris celles qui
        // finiront en 4xx côté handler (ex. station étrangère → 403). Conservateur ANTI-ABUS
        // (un attaquant ne peut pas sonder l'API sans consommer son propre quota) ; le coût
        // d'un quota « gâché » par une requête mal formée d'un client légitime est mineur.
        let mut redis = state.redis.clone();
        if rate_per_min > 0 {
            let n = crate::redis_store::rate_limit_hit(
                &mut redis,
                &format!("apikey:rl:{token_id}"),
                60,
            )
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
            if n > rate_per_min as u64 {
                return Err(AppError::TooManyRequests("rate-limit par minute depasse"));
            }
        }
        if monthly_quota > 0 {
            let month = Utc::now().format("%Y%m").to_string();
            let key = format!("apikey:quota:{token_id}:{month}");
            let n = crate::redis_store::rate_limit_hit(&mut redis, &key, 35 * 86_400)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
            if n > monthly_quota as u64 {
                return Err(AppError::TooManyRequests("quota mensuel depasse"));
            }
        }

        // Trace d'usage best-effort (échec non bloquant).
        let _ = sqlx::query("UPDATE api_tokens SET last_used_at = now() WHERE id = $1")
            .bind(token_id)
            .execute(&state.pg)
            .await;

        Ok(ApiKeyAuth {
            org_id,
            scope,
            token_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{client_ip, dummy_verify_password, generate_api_key, hash_api_key};
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

    #[test]
    fn api_key_generation_and_hash() {
        let (secret, prefix, hash) = generate_api_key();
        assert!(secret.starts_with("qrt_"), "préfixe : {secret}");
        assert_eq!(secret.len(), 52, "qrt_ + 48 hex");
        assert_eq!(prefix, secret[..8], "prefix = 8 premiers caractères");
        assert_eq!(hash.len(), 64, "SHA-256 en hex");
        assert_ne!(hash, secret, "le hash n'est pas le secret en clair");
        // Déterministe : re-hasher le secret redonne le même hash (lookup possible).
        assert_eq!(hash_api_key(&secret), hash);
        // Deux clés générées diffèrent (secret ET hash).
        let (secret2, _, hash2) = generate_api_key();
        assert_ne!(secret, secret2);
        assert_ne!(hash, hash2);
    }
}
