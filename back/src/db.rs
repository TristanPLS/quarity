//! Accès Postgres (sqlx, requêtes RUNTIME). CITEXT casté en text au SELECT.

use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

pub async fn make_pg_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await
}

#[derive(Debug, sqlx::FromRow)]
pub struct AuthContextRow {
    pub user_id: i64,
    pub email: String,
    pub password_hash: String,
    pub is_active: bool,
    pub org_id: i64,
    pub role_code: String,
    pub can_write: bool,
}

/// Login : user + (première) org + rôle, en un seul aller-retour.
pub async fn fetch_auth_context_by_email(
    pool: &PgPool,
    email: &str,
) -> Result<Option<AuthContextRow>, sqlx::Error> {
    sqlx::query_as::<_, AuthContextRow>(
        r#"
        SELECT
            u.id            AS user_id,
            u.email::text   AS email,
            u.password_hash AS password_hash,
            u.is_active     AS is_active,
            m.org_id        AS org_id,
            r.code          AS role_code,
            r.can_write     AS can_write
        FROM users u
        JOIN memberships m ON m.user_id = u.id
        JOIN roles       r ON r.id      = m.role_id
        WHERE lower(u.email::text) = lower($1)
          AND u.is_active = TRUE
        ORDER BY m.org_id
        LIMIT 1
        "#,
    )
    .bind(email)
    .fetch_optional(pool)
    .await
}

#[derive(Debug, sqlx::FromRow)]
pub struct UserRow {
    pub id: i64,
    pub email: String,
    pub full_name: Option<String>,
}

pub async fn fetch_user_by_id(pool: &PgPool, user_id: i64) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        r#"SELECT id, email::text AS email, full_name FROM users WHERE id = $1 AND is_active = TRUE"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

/// Recharge rôle + droits pour un couple (user, org) — utilisé au refresh.
pub async fn fetch_role(
    pool: &PgPool,
    user_id: i64,
    org_id: i64,
) -> Result<Option<(String, bool)>, sqlx::Error> {
    sqlx::query_as::<_, (String, bool)>(
        r#"
        SELECT r.code, r.can_write
        FROM memberships m
        JOIN roles r ON r.id = m.role_id
        WHERE m.user_id = $1 AND m.org_id = $2
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(org_id)
    .fetch_optional(pool)
    .await
}

/// Isolation multi-tenant : l'org suit-elle réellement cette station OpenAQ ?
pub async fn org_owns_location(
    pool: &PgPool,
    org_id: i64,
    openaq_location_id: i64,
) -> Result<bool, sqlx::Error> {
    let (owns,): (bool,) = sqlx::query_as::<_, (bool,)>(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM tracked_locations tl
            JOIN tracked_location_stations tls ON tls.tracked_location_id = tl.id
            JOIN ref_locations rl             ON rl.id = tls.ref_location_id
            WHERE tl.org_id = $1 AND rl.openaq_location_id = $2
        )
        "#,
    )
    .bind(org_id)
    .bind(openaq_location_id)
    .fetch_one(pool)
    .await?;
    Ok(owns)
}

pub async fn ping(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(pool).await?;
    Ok(())
}
