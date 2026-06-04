//! Endpoints d'authentification : login / refresh / logout / me.

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::error::AppError;
use crate::security::{self, AuthUser};
use crate::state::AppState;

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
    Json(req): Json<LoginRequest>,
) -> Result<Json<TokenResponse>, AppError> {
    if req.email.trim().is_empty() || req.password.is_empty() {
        return Err(AppError::BadRequest("email et mot de passe requis".into()));
    }

    let ctx = crate::db::fetch_auth_context_by_email(&state.pg, req.email.trim())
        .await?
        .ok_or(AppError::Unauthorized("invalid_credentials"))?;

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

    let mut conn = state.redis.clone();
    crate::redis_store::store_refresh(&mut conn, &jti, ctx.user_id, ctx.org_id).await?;

    Ok(Json(TokenResponse {
        access_token: access,
        refresh_token: jti,
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
    Json(req): Json<RefreshRequest>,
) -> Result<Json<TokenResponse>, AppError> {
    let mut conn = state.redis.clone();

    let (uid, oid) = crate::redis_store::read_refresh(&mut conn, &req.refresh_token)
        .await?
        .ok_or(AppError::Unauthorized("invalid_refresh"))?;

    let (role, can_write) = crate::db::fetch_role(&state.pg, uid, oid)
        .await?
        .ok_or(AppError::Unauthorized("invalid_refresh"))?;

    // Rotation : on révoque l'ancien refresh et on en émet un nouveau.
    crate::redis_store::delete_refresh(&mut conn, &req.refresh_token).await?;
    let new_jti = Uuid::new_v4().to_string();
    let ttl = state.cfg.access_ttl_secs;
    let access = security::issue_access_token(
        state.cfg.jwt_secret.as_bytes(),
        uid,
        oid,
        &role,
        can_write,
        &new_jti,
        ttl,
    )
    .map_err(|e| AppError::Internal(anyhow::anyhow!("jwt encode: {e}")))?;

    crate::redis_store::store_refresh(&mut conn, &new_jti, uid, oid).await?;

    Ok(Json(TokenResponse {
        access_token: access,
        refresh_token: new_jti,
        token_type: "Bearer".into(),
        expires_in: ttl,
    }))
}

#[derive(Deserialize)]
pub struct LogoutRequest {
    pub refresh_token: String,
}

pub async fn logout(
    State(state): State<AppState>,
    _user: AuthUser,
    Json(req): Json<LogoutRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut conn = state.redis.clone();
    crate::redis_store::delete_refresh(&mut conn, &req.refresh_token).await?;
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
