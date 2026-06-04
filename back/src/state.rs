//! État applicatif partagé (clonable) injecté dans tous les handlers.

use std::sync::Arc;

use redis::aio::ConnectionManager;
use sqlx::PgPool;

use crate::ch::ClickhouseClient;
use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub pg: PgPool,
    pub redis: ConnectionManager,
    pub ch: ClickhouseClient,
}

impl AppState {
    pub async fn connect(cfg: Arc<Config>) -> anyhow::Result<Self> {
        let pg = crate::db::make_pg_pool(&cfg.database_url).await?;

        let client = redis::Client::open(cfg.redis_url.clone())?;
        let redis = ConnectionManager::new(client).await?;

        let ch = ClickhouseClient::new(
            cfg.clickhouse_url.clone(),
            cfg.clickhouse_user.clone(),
            cfg.clickhouse_password.clone(),
            cfg.clickhouse_db.clone(),
        );

        Ok(Self { cfg, pg, redis, ch })
    }
}
