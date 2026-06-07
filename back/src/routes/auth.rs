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
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::error::AppError;
use crate::openapi::{ErrorBody, LogoutResponse, MeResponse};
use crate::security::{self, AuthUser};
use crate::state::AppState;
use crate::validation::ValidatedJson;

/// Fenêtre des compteurs de rate-limit (les limites configurées sont « par minute »).
const RATE_LIMIT_WINDOW_SECS: i64 = 60;

#[derive(Deserialize, ToSchema, Validate)]
pub struct LoginRequest {
    /// 1 à 254 caractères (longueur maximale RFC d'une adresse), non composé
    /// uniquement d'espaces.
    #[validate(
        length(min = 1, max = 254, message = "longueur attendue 1..=254"),
        custom(function = "crate::validation::validate_not_blank")
    )]
    #[schema(example = "sophie@agglo-riviera.fr")]
    pub email: String,
    /// 1 à 512 caractères — borne anti-DoS : sans plafond, un corps arbitrairement
    /// long part dans une vérification Argon2 coûteuse.
    #[validate(length(min = 1, max = 512, message = "longueur attendue 1..=512"))]
    #[schema(example = "Quarity2026!")]
    pub password: String,
}

#[derive(Serialize, ToSchema)]
pub struct TokenResponse {
    /// JWT d'accès (HS256), à passer en `Authorization: Bearer <token>`.
    pub access_token: String,
    /// Jeton opaque de rafraîchissement (rotation à chaque usage).
    pub refresh_token: String,
    #[schema(example = "Bearer")]
    pub token_type: String,
    /// Durée de vie de l'access token, en secondes.
    #[schema(example = 900)]
    pub expires_in: i64,
}

/// Login : émet un couple access/refresh token.
#[utoipa::path(
    post,
    path = "/api/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Authentification réussie", body = TokenResponse),
        (status = 400, description = "Email ou mot de passe vide ou hors bornes (validation — corps `ErrorBody`) — ou JSON malformé (rejet de l'extracteur, corps texte)", body = ErrorBody),
        (status = 401, description = "Identifiants invalides (`invalid_credentials`)", body = ErrorBody),
        (status = 415, description = "`Content-Type` non JSON (rejet de l'extracteur, corps texte)"),
        (status = 422, description = "Champs manquants ou mal typés (rejet de l'extracteur, corps texte)"),
        (status = 429, description = "Rate-limit dépassé — par email ou par IP (`rate_limited`)", body = ErrorBody),
    )
)]
pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    ValidatedJson(req): ValidatedJson<LoginRequest>,
) -> Result<Json<TokenResponse>, AppError> {
    // La présence/longueur/non-blancheur est garantie par `ValidatedJson<LoginRequest>`
    // (A5) — il ne reste ici que la NORMALISATION de l'identifiant.
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

#[derive(Deserialize, ToSchema, Validate)]
pub struct RefreshRequest {
    /// 1 à 128 caractères (les jetons émis sont des UUID v4 — 36 caractères).
    #[validate(length(min = 1, max = 128, message = "longueur attendue 1..=128"))]
    pub refresh_token: String,
}

/// Refresh : rotation du refresh token + nouvel access token.
#[utoipa::path(
    post,
    path = "/api/auth/refresh",
    tag = "auth",
    request_body = RefreshRequest,
    responses(
        (status = 200, description = "Nouveau couple de jetons (l'ancien refresh est révoqué)", body = TokenResponse),
        (status = 400, description = "`refresh_token` vide ou hors bornes (validation — corps `ErrorBody`) — ou JSON malformé (rejet de l'extracteur, corps texte)", body = ErrorBody),
        (status = 401, description = "Refresh token inconnu, expiré, ou compte désactivé (`invalid_refresh`)", body = ErrorBody),
        (status = 415, description = "`Content-Type` non JSON (rejet de l'extracteur, corps texte)"),
        (status = 422, description = "Champ `refresh_token` manquant ou mal typé (rejet de l'extracteur, corps texte)"),
        (status = 429, description = "Rate-limit par IP dépassé (`rate_limited`)", body = ErrorBody),
    )
)]
pub async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    ValidatedJson(req): ValidatedJson<RefreshRequest>,
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

#[derive(Deserialize, ToSchema, Validate)]
pub struct LogoutRequest {
    /// 1 à 128 caractères (les jetons émis sont des UUID v4 — 36 caractères).
    #[validate(length(min = 1, max = 128, message = "longueur attendue 1..=128"))]
    pub refresh_token: String,
}

/// Logout : ne supprime le refresh token QUE s'il appartient à l'appelant authentifié.
///
/// Choix « refus silencieux » (plutôt que 403) : la réponse est identique que le token
/// soit valide, inconnu ou appartienne à autrui — sinon le logout deviendrait un oracle
/// permettant de tester la validité des refresh tokens d'autres utilisateurs.
/// La tentative sur le token d'autrui est tracée (warn) côté serveur.
#[utoipa::path(
    post,
    path = "/api/auth/logout",
    tag = "auth",
    request_body = LogoutRequest,
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Réponse identique que le token soit valide, inconnu ou à autrui (anti-oracle)", body = LogoutResponse),
        (status = 400, description = "`refresh_token` vide ou hors bornes (validation — corps `ErrorBody`) — ou JSON malformé (rejet de l'extracteur, corps texte)", body = ErrorBody),
        (status = 401, description = "Bearer manquant ou invalide", body = ErrorBody),
        (status = 415, description = "`Content-Type` non JSON (rejet de l'extracteur, corps texte)"),
        (status = 422, description = "Champ `refresh_token` manquant ou mal typé (rejet de l'extracteur, corps texte)"),
    )
)]
pub async fn logout(
    State(state): State<AppState>,
    user: AuthUser,
    ValidatedJson(req): ValidatedJson<LogoutRequest>,
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

/// Identité de l'appelant (claims JWT + profil Postgres).
#[utoipa::path(
    get,
    path = "/api/auth/me",
    tag = "auth",
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Identité authentifiée", body = MeResponse),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = ErrorBody),
        (status = 404, description = "Utilisateur du JWT introuvable (`user_not_found`)", body = ErrorBody),
    )
)]
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
