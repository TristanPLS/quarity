//! Endpoints d'authentification : login / refresh / logout / me.
//!
//! Durcissements (revue sécurité 2026-06-06) :
//! - S4 : le refresh token est un secret INDÉPENDANT du `jti` du JWT d'accès
//!   (le payload JWT étant du base64 lisible, exposer le jti comme refresh
//!   transformait toute fuite d'access token 15 min en session 7 j).
//! - A4 : rate-limit fenêtre fixe en Redis sur login (par email + par IP) et refresh (par IP).
//! - `is_active` revérifié à chaque refresh (compte désactivé ⇒ refresh révoqué + 401).
//! - Logout : contrôle de propriété du refresh token avant suppression.
//! - Anti-énumération temporelle : vérification Argon2 factice quand l'email est inconnu.

use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::error::AppError;
use crate::security::{self, AuthUser};
use crate::state::AppState;

/// Fenêtre des compteurs de rate-limit (les limites configurées sont « par minute »).
const RATE_LIMIT_WINDOW_SECS: i64 = 60;

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: i64,
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Result<Json<TokenResponse>, AppError> {
    if req.email.trim().is_empty() || req.password.is_empty() {
        return Err(AppError::BadRequest("email et mot de passe requis".into()));
    }

    let email = req.email.trim().to_lowercase();
    let mut conn = state.redis.clone();

    // A4 — rate-limit AVANT tout travail coûteux (Argon2/SQL) : par email puis par IP.
    let email_hits = crate::redis_store::rate_limit_hit(
        &mut conn,
        &format!("ratelimit:login:email:{email}"),
        RATE_LIMIT_WINDOW_SECS,
    )
    .await?;
    if email_hits > state.cfg.rate_limit_login_email_per_min {
        return Err(AppError::TooManyRequests(
            "trop de tentatives de connexion pour cet email — réessayez dans une minute",
        ));
    }
    let ip = security::client_ip(&headers);
    let ip_hits = crate::redis_store::rate_limit_hit(
        &mut conn,
        &format!("ratelimit:login:ip:{ip}"),
        RATE_LIMIT_WINDOW_SECS,
    )
    .await?;
    if ip_hits > state.cfg.rate_limit_login_ip_per_min {
        return Err(AppError::TooManyRequests(
            "trop de tentatives de connexion depuis cette adresse — réessayez dans une minute",
        ));
    }

    let Some(ctx) = crate::db::fetch_auth_context_by_email(&state.pg, &email).await? else {
        // Anti-énumération temporelle : même coût Argon2 que pour un email connu,
        // sinon le temps de réponse trahit l'existence du compte.
        security::dummy_verify_password(&req.password);
        return Err(AppError::Unauthorized("invalid_credentials"));
    };

    if !security::verify_password(&req.password, &ctx.password_hash) {
        return Err(AppError::Unauthorized("invalid_credentials"));
    }

    let jti = Uuid::new_v4().to_string();
    let ttl = state.cfg.access_ttl_secs;
    let access = security::issue_access_token(
        state.cfg.jwt_secret.as_bytes(),
        ctx.user_id,
        ctx.org_id,
        &ctx.role_code,
        ctx.can_write,
        &jti,
        ttl,
    )
    .map_err(|e| AppError::Internal(anyhow::anyhow!("jwt encode: {e}")))?;

    // S4 — refresh token indépendant du jti (jamais dérivable de l'access token).
    let refresh_token = Uuid::new_v4().to_string();
    crate::redis_store::store_refresh(
        &mut conn,
        &refresh_token,
        ctx.user_id,
        ctx.org_id,
        state.cfg.refresh_ttl_secs,
    )
    .await?;

    Ok(Json(TokenResponse {
        access_token: access,
        refresh_token,
        token_type: "Bearer".into(),
        expires_in: ttl,
    }))
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

pub async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RefreshRequest>,
) -> Result<Json<TokenResponse>, AppError> {
    let mut conn = state.redis.clone();

    // A4 — rate-limit par IP (un refresh token volé ne doit pas se brute-forcer).
    let ip = security::client_ip(&headers);
    let hits = crate::redis_store::rate_limit_hit(
        &mut conn,
        &format!("ratelimit:refresh:ip:{ip}"),
        RATE_LIMIT_WINDOW_SECS,
    )
    .await?;
    if hits > state.cfg.rate_limit_refresh_ip_per_min {
        return Err(AppError::TooManyRequests(
            "trop de rafraîchissements depuis cette adresse — réessayez dans une minute",
        ));
    }

    let (uid, oid) = crate::redis_store::read_refresh(&mut conn, &req.refresh_token)
        .await?
        .ok_or(AppError::Unauthorized("invalid_refresh"))?;

    // Revalidation Postgres : membership existant ET compte actif. Un compte
    // désactivé (ou sorti de l'org) perd sa session : refresh révoqué + 401.
    let ctx = crate::db::fetch_refresh_context(&state.pg, uid, oid).await?;
    let ctx = match ctx {
        Some(c) if c.is_active => c,
        _ => {
            crate::redis_store::delete_refresh(&mut conn, &req.refresh_token).await?;
            return Err(AppError::Unauthorized("invalid_refresh"));
        }
    };

    // Rotation : on révoque l'ancien refresh et on en émet un nouveau.
    crate::redis_store::delete_refresh(&mut conn, &req.refresh_token).await?;
    let new_jti = Uuid::new_v4().to_string();
    let ttl = state.cfg.access_ttl_secs;
    let access = security::issue_access_token(
        state.cfg.jwt_secret.as_bytes(),
        uid,
        oid,
        &ctx.role_code,
        ctx.can_write,
        &new_jti,
        ttl,
    )
    .map_err(|e| AppError::Internal(anyhow::anyhow!("jwt encode: {e}")))?;

    // S4 — nouveau refresh token lui aussi indépendant du nouveau jti.
    let new_refresh = Uuid::new_v4().to_string();
    crate::redis_store::store_refresh(
        &mut conn,
        &new_refresh,
        uid,
        oid,
        state.cfg.refresh_ttl_secs,
    )
    .await?;

    Ok(Json(TokenResponse {
        access_token: access,
        refresh_token: new_refresh,
        token_type: "Bearer".into(),
        expires_in: ttl,
    }))
}

#[derive(Deserialize)]
pub struct LogoutRequest {
    pub refresh_token: String,
}

/// Logout : ne supprime le refresh token QUE s'il appartient à l'appelant authentifié.
///
/// Choix « refus silencieux » (plutôt que 403) : la réponse est identique que le token
/// soit valide, inconnu ou appartienne à autrui — sinon le logout deviendrait un oracle
/// permettant de tester la validité des refresh tokens d'autres utilisateurs.
/// La tentative sur le token d'autrui est tracée (warn) côté serveur.
pub async fn logout(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<LogoutRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut conn = state.redis.clone();
    match crate::redis_store::read_refresh(&mut conn, &req.refresh_token).await? {
        Some((uid, _)) if uid == user.user_id => {
            crate::redis_store::delete_refresh(&mut conn, &req.refresh_token).await?;
        }
        Some(_) => {
            tracing::warn!(
                user_id = user.user_id,
                "logout refusé : refresh token appartenant à un autre utilisateur"
            );
        }
        None => {} // token inconnu/expiré : rien à faire, réponse identique.
    }
    Ok(Json(json!({ "status": "logged_out" })))
}

pub async fn me(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let u = crate::db::fetch_user_by_id(&state.pg, user.user_id)
        .await?
        .ok_or(AppError::NotFound("user_not_found"))?;
    Ok(Json(json!({
        "user_id": u.id,
        "email": u.email,
        "full_name": u.full_name,
        "org_id": user.org_id,
        "role": user.role,
        "can_write": user.can_write,
    })))
}
