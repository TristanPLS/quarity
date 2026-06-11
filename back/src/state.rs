//! État applicatif partagé (clonable) injecté dans tous les handlers.

use std::sync::Arc;

use redis::aio::ConnectionManager;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;

use crate::alerts::AlertHub;
use crate::ch::ClickhouseClient;
use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub pg: PgPool,
    pub redis: ConnectionManager,
    pub ch: ClickhouseClient,
    /// Registre des clients WebSocket d'alerte (B8), par organisation.
    pub alerts: AlertHub,
    /// Signal d'arrêt gracieux (B8b) : annulé par `main` après l'arrêt du serveur HTTP.
    /// Partagé par la boucle de matching, l'abonné Redis et chaque session WebSocket —
    /// toutes s'arrêtent proprement au lieu d'être tuées à la volée. Les tests ne
    /// l'annulent jamais (le runtime du test abandonne les tâches en fin de test).
    pub shutdown: CancellationToken,
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

        // Alertes temps réel (B8) : registre in-process + UNE tâche d'abonnement
        // Redis par instance (PSUBSCRIBE quarity:alerts:org:*). Spawnée ICI — donc
        // active pour le binaire ET pour chaque instance de test (`spawn_app`) : un
        // client WebSocket reçoit les events publiés sur SON instance. L'abonné est
        // en LECTURE seule sur Redis (aucune écriture base), inoffensif en e2e —
        // contrairement à la boucle de matching, qui ne tourne que dans `main.rs`.
        let alerts = AlertHub::new(cfg.ws_client_buffer, cfg.ws_max_connections_per_org);
        let shutdown = CancellationToken::new();
        // `.await` : le PREMIER PSUBSCRIBE est synchrone — `connect` ne rend la main
        // qu'une fois l'abonné prêt (Redis up), donc aucun « publié avant abonnement ».
        // On DÉTACHE la tâche (JoinHandle ignoré) : l'abonné (lecture seule sur Redis)
        // s'arrête sur annulation du `shutdown` partagé — pas besoin de l'attendre.
        crate::alerts::start_alert_subscriber(
            cfg.redis_url.clone(),
            alerts.clone(),
            shutdown.clone(),
        )
        .await;

        Ok(Self {
            cfg,
            pg,
            redis,
            ch,
            alerts,
            shutdown,
        })
    }
}
