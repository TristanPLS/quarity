//! Endpoint WebSocket des alertes temps réel (B8) : `GET /api/ws`.
//!
//! ## Authentification
//!
//! Le jeton d'accès JWT passe en **query string** (`/api/ws?token=...`) : un
//! navigateur ne peut PAS poser d'en-tête `Authorization` sur une WebSocket native
//! (limitation de l'API HTML5, pas un choix). Le jeton est validé UNE fois au
//! handshake (mêmes signature/expiration que l'extracteur Bearer, via
//! [`crate::security::decode_access_token`]). L'`org_id` provient des claims
//! SIGNÉS — jamais d'un paramètre client : un client ne peut pas écouter une autre
//! org.
//!
//! Limites documentées (back MVP B8) : le jeton de la WebSocket ne se renouvelle
//! pas en vol — si l'access token expire pendant une session longue, la connexion
//! reste ouverte jusqu'à sa fermeture ; le front la rouvrira après un refresh (B11).
//! En prod, WSS est requis (le TLS chiffre la query string).
//!
//! ## Cycle de vie
//!
//! À l'upgrade, le client est enregistré dans l'[`crate::alerts::AlertHub`] sous
//! son `org_id` ; une boucle `select!` pousse les messages reçus du hub, détecte la
//! fermeture et émet un ping périodique (keep-alive proxy). À la sortie (fermeture,
//! erreur d'envoi), le client est retiré du hub (tous chemins).

use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::Response;
use serde::Deserialize;
use tokio::time::interval;

use crate::alerts::AlertHub;
use crate::error::AppError;
use crate::security::decode_access_token;
use crate::state::AppState;

/// Query string du handshake : le jeton d'accès JWT (faute d'en-tête Authorization
/// sur une WebSocket navigateur).
#[derive(Debug, Deserialize)]
pub struct WsAuthQuery {
    pub token: String,
}

/// `GET /api/ws?token=<jwt>` — upgrade WebSocket après validation du jeton.
/// 401 (`token_expired` / `invalid_token`) si le jeton est invalide/expiré ;
/// 400 si le paramètre `token` est absent (rejet de l'extracteur `Query`).
pub async fn alerts_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(q): Query<WsAuthQuery>,
) -> Result<Response, AppError> {
    // org_id dérivé des claims SIGNÉS : infalsifiable, jamais d'un paramètre client.
    let claims = decode_access_token(&state.cfg.jwt_secret, &q.token)?;
    let org_id = claims.org_id;
    let hub = state.alerts.clone();
    let ping_every = Duration::from_secs(state.cfg.ws_ping_interval_secs.max(1));
    Ok(ws.on_upgrade(move |socket| handle_socket(socket, org_id, hub, ping_every)))
}

/// Boucle de service d'UNE connexion : pousse les alertes de l'org, détecte la
/// fermeture, ping périodique. Retire le client du hub en sortant (tous chemins).
async fn handle_socket(mut socket: WebSocket, org_id: i64, hub: AlertHub, ping_every: Duration) {
    let (conn_id, mut rx) = hub.register(org_id);

    let mut ping = interval(ping_every);
    ping.tick().await; // 1er tick immédiat consommé : pas de ping dès la connexion.

    loop {
        tokio::select! {
            // Message d'alerte à pousser (fan-out du hub).
            maybe = rx.recv() => match maybe {
                Some(payload) => {
                    if socket.send(Message::Text(payload.into())).await.is_err() {
                        break; // socket fermée côté client
                    }
                }
                None => break, // hub fermé — ne devrait pas arriver en service
            },
            // Trafic entrant : on n'attend AUCUNE commande client (lecture seule) ;
            // seule la fermeture (ou une erreur) nous intéresse.
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {} // ping/pong/texte client : ignorés
                Some(Err(_)) => break,
            },
            // Keep-alive : ping périodique (traverse les proxies à timeout court).
            _ = ping.tick() => {
                if socket.send(Message::Ping(Default::default())).await.is_err() {
                    break;
                }
            }
        }
    }

    hub.unregister(org_id, conn_id);
}
