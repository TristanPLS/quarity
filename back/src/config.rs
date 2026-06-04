//! Configuration typée depuis l'environnement.

use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: String,
    pub database_url: String,
    pub redis_url: String,
    pub clickhouse_url: String,
    pub clickhouse_db: String,
    pub clickhouse_user: String,
    pub clickhouse_password: String,
    pub jwt_secret: String,
    pub access_ttl_secs: i64,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Arc<Self>> {
        fn req(k: &str) -> anyhow::Result<String> {
            std::env::var(k).map_err(|_| anyhow::anyhow!("variable d'environnement manquante : {k}"))
        }

        let jwt_secret = req("JWT_SECRET")?;
        if jwt_secret.len() < 32 {
            anyhow::bail!("JWT_SECRET doit faire au moins 32 caractères");
        }

        Ok(Arc::new(Self {
            bind_addr: std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into()),
            database_url: req("DATABASE_URL")?,
            redis_url: req("REDIS_URL")?,
            clickhouse_url: req("CLICKHOUSE_URL")?,
            clickhouse_db: std::env::var("CLICKHOUSE_DB").unwrap_or_else(|_| "quarity".into()),
            clickhouse_user: req("CLICKHOUSE_USER")?,
            clickhouse_password: req("CLICKHOUSE_PASSWORD")?,
            jwt_secret,
            access_ttl_secs: std::env::var("JWT_ACCESS_TTL_SECONDS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(900),
        }))
    }
}
