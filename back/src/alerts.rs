//! Alertes temps réel (B8) : Redis pub/sub par organisation → push WebSocket.
//!
//! ## Pourquoi Redis pub/sub ET un registre in-process
//!
//! Quand la boucle de matching (B7) écrit RÉELLEMENT un `alert_event` (idempotence
//! 0006 : un dépassement déjà vu ne ré-insère rien), elle **publie** l'événement
//! sur le canal Redis `quarity:alerts:org:{org_id}`. À terme, N réplicas du back
//! tournent : Redis fait le **fan-out** à tous, et chaque instance ne pousse qu'à
//! SES clients WebSocket connectés. C'est exactement ce que Moka (L1, in-process)
//! ne peut pas faire — d'où Redis (L2, distribué), cf. roadmap « Pourquoi Moka ET
//! Redis ».
//!
//! ## Topologie
//!
//! - **Producteur** ([`publish_alert_events`]) : appelé par `matching::run_loop` et
//!   par le force-check API APRÈS l'insertion, sur les events RÉELLEMENT créés
//!   (jamais sur les `Breach` bruts : un rejouage en republierait). Best-effort :
//!   Redis indisponible ⇒ trace `warn`, JAMAIS d'échec (le fait persistant est la
//!   ligne en base ; le push live n'est qu'un bonus).
//! - **Abonné (1 par instance)** ([`spawn_alert_subscriber`]) : ouvre UNE connexion
//!   Redis dédiée (mode souscription — incompatible avec la `ConnectionManager`
//!   partagée des commandes) et `PSUBSCRIBE quarity:alerts:org:*`. Chaque message
//!   est routé EN MÉMOIRE vers les clients de l'org via [`AlertHub`] — surtout pas
//!   un abonnement Redis par client (1000 clients ⇒ 1000 connexions).
//! - **Registre** ([`AlertHub`]) : `org_id → {clients}`. Un client = un canal mpsc
//!   BORNÉ ; un client lent voit ses messages déposés (`try_send`) plutôt que de
//!   faire gonfler la mémoire du back.
//!
//! ## Isolation multi-tenant
//!
//! `org_id` vient TOUJOURS du canal (dérivé des claims JWT SIGNÉS à l'abonnement
//! WebSocket — cf. `routes::ws`), jamais d'une entrée client. Un client de l'org A
//! n'est enregistré QUE sous la clé A : un message pour l'org B ne le trouve pas.
//! Et grâce à l'idempotence 0006, en multi-instance une SEULE instance insère donc
//! publie — pas de doublon côté client.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use redis::aio::ConnectionManager;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::matching::InsertedEvent;

/// Préfixe des canaux Redis (namespacé « quarity » : un Redis partagé avec un
/// autre service ne collisionne pas).
const CHANNEL_PREFIX: &str = "quarity:alerts:org:";

/// Canal d'une org.
fn channel_for(org_id: i64) -> String {
    format!("{CHANNEL_PREFIX}{org_id}")
}

/// Extrait l'`org_id` d'un nom de canal (`quarity:alerts:org:42` → 42). `None` si
/// le suffixe n'est pas un entier (canal étranger au motif — ignoré sans bruit).
fn org_from_channel(channel: &str) -> Option<i64> {
    channel.strip_prefix(CHANNEL_PREFIX)?.parse::<i64>().ok()
}

/// Message poussé au client WebSocket. `type: "alert"` discrimine d'éventuels
/// futurs types (heartbeat applicatif, ack…). `event` = le snapshot inséré.
#[derive(Serialize)]
struct AlertMessage<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    event: &'a InsertedEvent,
}

/// Clients WebSocket d'UNE org : id de connexion → émetteur vers son socket.
type OrgClients = HashMap<u64, mpsc::Sender<String>>;

/// Registre in-process des clients WebSocket, par organisation. Clonable (tout est
/// derrière `Arc`) — vit dans `AppState`.
#[derive(Clone)]
pub struct AlertHub {
    subscribers: Arc<Mutex<HashMap<i64, OrgClients>>>,
    next_id: Arc<AtomicU64>,
    /// Profondeur de file par client (backpressure bornée).
    buffer: usize,
}

impl AlertHub {
    pub fn new(buffer: usize) -> Self {
        Self {
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(0)),
            buffer: buffer.max(1),
        }
    }

    /// Enregistre un client de l'org `org_id` ; renvoie son id (pour le
    /// désenregistrer) et le récepteur d'où le handler WebSocket lira les messages.
    pub fn register(&self, org_id: i64) -> (u64, mpsc::Receiver<String>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(self.buffer);
        self.subscribers
            .lock()
            .expect("verrou AlertHub")
            .entry(org_id)
            .or_default()
            .insert(id, tx);
        (id, rx)
    }

    /// Retire un client (à la fermeture de sa WebSocket) ; vide l'entrée d'org
    /// devenue sans client.
    pub fn unregister(&self, org_id: i64, id: u64) {
        let mut guard = self.subscribers.lock().expect("verrou AlertHub");
        if let Some(clients) = guard.get_mut(&org_id) {
            clients.remove(&id);
            if clients.is_empty() {
                guard.remove(&org_id);
            }
        }
    }

    /// Distribue un message (déjà sérialisé) à tous les clients de l'org. `try_send`
    /// NON bloquant : un client plein (lent) ou fermé voit le message DÉPOSÉ — jamais
    /// d'attente, jamais de mémoire qui gonfle (le verrou n'est tenu qu'un instant,
    /// aucun `await` dessous).
    fn dispatch(&self, org_id: i64, payload: &str) {
        let guard = self.subscribers.lock().expect("verrou AlertHub");
        if let Some(clients) = guard.get(&org_id) {
            for tx in clients.values() {
                if tx.try_send(payload.to_string()).is_err() {
                    tracing::debug!(org_id, "client WS lent ou fermé — message d'alerte déposé");
                }
            }
        }
    }
}

/// Publie les événements RÉELLEMENT insérés sur leur canal d'org (un PUBLISH par
/// event). Best-effort : une erreur Redis est tracée puis ignorée — l'événement
/// reste en base (fait persistant), seul le push live est perdu. Utilise la
/// `ConnectionManager` partagée (PUBLISH est une commande ordinaire, pas une
/// souscription).
pub async fn publish_alert_events(redis: &mut ConnectionManager, events: &[InsertedEvent]) {
    for event in events {
        let payload = match serde_json::to_string(&AlertMessage {
            kind: "alert",
            event,
        }) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "sérialisation d'une alerte échouée — non publiée");
                continue;
            }
        };
        let channel = channel_for(event.org_id);
        let published: redis::RedisResult<()> = redis::cmd("PUBLISH")
            .arg(&channel)
            .arg(&payload)
            .query_async(redis)
            .await;
        if let Err(e) = published {
            tracing::warn!(channel = %channel, error = %e,
                "publication d'alerte Redis échouée (événement en base, push live perdu)");
        }
    }
}

/// Démarre l'abonnement aux alertes (UNE souscription par instance). Le PREMIER
/// abonnement est **synchrone** : en cas nominal (Redis up — garanti par le
/// healthcheck du compose), cette fonction ne rend la main qu'une fois le
/// `PSUBSCRIBE` ACTIF, si bien que l'appelant ([`crate::state::AppState::connect`])
/// sait l'abonné prêt — pas de course « publié avant abonnement » (déterminisme
/// des tests e2e compris). Si Redis est indisponible au boot, on bascule SANS
/// bloquer le démarrage en mode best-effort (la tâche de fond reconnecte).
///
/// La tâche pompe les messages et RECONNECTE sur coupure, TOUJOURS avec un délai
/// entre deux tentatives (jamais de boucle serrée, même si le flux se ferme
/// proprement). Détachée : son arrêt gracieux est un reliquat assumé (B8b —
/// CancellationToken partagé avec la boucle de matching).
pub async fn start_alert_subscriber(redis_url: String, hub: AlertHub) {
    let first = match subscribe(&redis_url).await {
        Ok(pubsub) => Some(pubsub),
        Err(e) => {
            tracing::warn!(error = %e,
                "premier abonnement aux alertes Redis échoué — best-effort en tâche de fond");
            None
        }
    };

    tokio::spawn(async move {
        let mut pending = first;
        loop {
            // Premier tour : réutilise l'abonnement déjà ouvert ; ensuite, en rouvre un.
            let pubsub = match pending.take() {
                Some(pubsub) => pubsub,
                None => match subscribe(&redis_url).await {
                    Ok(pubsub) => pubsub,
                    Err(e) => {
                        tracing::warn!(error = %e,
                            "réabonnement aux alertes Redis échoué — nouvelle tentative dans 2 s");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                },
            };
            pump_messages(pubsub, &hub).await;
            // Flux terminé/coupé : la connexion est fermée (drop ci-dessus au retour
            // de pump_messages) — on TEMPORISE avant de rouvrir, jamais de boucle serrée.
            tracing::warn!("flux d'alertes Redis interrompu — réabonnement dans 2 s");
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}

/// Ouvre une connexion Redis DÉDIÉE (mode souscription — incompatible avec la
/// `ConnectionManager` des commandes) et `PSUBSCRIBE` le motif des canaux d'alerte.
async fn subscribe(redis_url: &str) -> redis::RedisResult<redis::aio::PubSub> {
    let client = redis::Client::open(redis_url)?;
    let mut pubsub = client.get_async_pubsub().await?;
    pubsub.psubscribe(format!("{CHANNEL_PREFIX}*")).await?;
    tracing::info!("abonnement aux alertes Redis actif (PSUBSCRIBE {CHANNEL_PREFIX}*)");
    Ok(pubsub)
}

/// Pompe les messages jusqu'à fin/coupure du flux ; route chacun vers les clients
/// de l'org via le hub. La connexion `pubsub` est fermée (drop) au retour.
async fn pump_messages(mut pubsub: redis::aio::PubSub, hub: &AlertHub) {
    let mut stream = pubsub.on_message();
    while let Some(msg) = stream.next().await {
        let channel = msg.get_channel_name().to_string();
        let Ok(payload) = msg.get_payload::<String>() else {
            tracing::warn!(channel = %channel, "charge utile d'alerte illisible — ignorée");
            continue;
        };
        if let Some(org_id) = org_from_channel(&channel) {
            hub.dispatch(org_id, &payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::org_from_channel;

    #[test]
    fn parses_org_from_channel() {
        assert_eq!(org_from_channel("quarity:alerts:org:42"), Some(42));
        assert_eq!(org_from_channel("quarity:alerts:org:0"), Some(0));
        assert_eq!(org_from_channel("quarity:alerts:org:abc"), None);
        assert_eq!(org_from_channel("autre:canal"), None);
    }
}
