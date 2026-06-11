//! Quarity — back (walking skeleton).
//! Bootstrap : tracing, config (env), connexions (Postgres/Redis/ClickHouse), routeur, serve + arrêt gracieux.
//! Les modules vivent dans la lib `quarity_back` (cf. `src/lib.rs`) pour rester testables depuis `tests/`.

use anyhow::Context;
use quarity_back::{config, routes, security, state};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn,hyper=warn")),
        )
        .init();

    // Helper hors-ligne : `quarity-back hash <password>` imprime un hash argon2id (pour le seed/démo).
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "hash" {
        println!("{}", security::hash_password(&args[2])?);
        return Ok(());
    }

    let cfg = config::Config::from_env()?;
    tracing::info!(bind = %cfg.bind_addr, "démarrage quarity-back");

    let state = state::AppState::connect(cfg.clone())
        .await
        .context("connexion aux dépendances (Postgres/Redis/ClickHouse)")?;

    // Migrations Postgres (back/migrations/, embarquées à la compilation) : le schéma
    // DOIT être à jour AVANT de servir. Échec = erreur fatale (on ne démarre jamais
    // sur un schéma incomplet ou divergent).
    sqlx::migrate!("./migrations")
        .run(&state.pg)
        .await
        .context("application des migrations Postgres (back/migrations) — démarrage refusé")?;
    tracing::info!("migrations Postgres appliquées (_sqlx_migrations à jour)");

    // Jeton d'arrêt gracieux (B8b), conservé pour APRÈS l'arrêt du serveur (state est
    // ensuite déplacé dans build_router). Partagé par la boucle de matching, l'abonné
    // Redis et chaque session WebSocket.
    let shutdown = state.shutdown.clone();

    // Boucle de matching B7 : tâche de fond périodique (mesures ingérées ×
    // règles actives → alert_events). Désactivable (MATCHING_INTERVAL_SECS=0) ;
    // un tick en échec est tracé et retenté, jamais fatal (cf. matching.rs).
    // Spawnée ICI (pas dans build_router) : les tests `spawn_app` instancient
    // des routeurs sans boucle — pas de matching parasite pendant les e2e.
    let matching = if cfg.matching_interval_secs > 0 {
        let handle = tokio::spawn(quarity_back::matching::run_loop(state.clone()));
        tracing::info!(
            interval_secs = cfg.matching_interval_secs,
            rules_ttl_secs = cfg.matching_rules_ttl_secs,
            "boucle de matching démarrée"
        );
        Some(handle)
    } else {
        tracing::info!("boucle de matching désactivée (MATCHING_INTERVAL_SECS=0)");
        None
    };

    let app = routes::build_router(state);

    let listener = tokio::net::TcpListener::bind(&cfg.bind_addr)
        .await
        .with_context(|| format!("bind {}", cfg.bind_addr))?;
    tracing::info!("à l'écoute sur http://{}", cfg.bind_addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serveur HTTP")?;

    // Arrêt gracieux des tâches de fond (B8b) : on signale l'annulation (l'abonné
    // Redis et les sessions WebSocket s'arrêtent sur ce même jeton partagé), puis on
    // laisse la boucle de matching finir son tick en cours sous un budget borné.
    tracing::info!("arrêt du serveur — annulation des tâches de fond");
    shutdown.cancel();
    if let Some(handle) = matching {
        if tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .is_err()
        {
            tracing::warn!("boucle de matching pas arrêtée sous 5 s — abandon");
        }
    }

    Ok(())
}

/// Attend Ctrl-C ou SIGTERM (arrêt propre en conteneur).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("signal d'arrêt reçu — arrêt gracieux");
}
