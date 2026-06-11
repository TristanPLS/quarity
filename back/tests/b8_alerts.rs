//! Tests e2e des alertes temps réel B8 (Redis pub/sub par org → WebSocket /api/ws).
//!
//! Prérequis : identiques à `tests/b7_matching.rs` (3 bases réelles + schéma + seed,
//! variables d'env du back exportées).
//!
//! ## Isolation des tests (parallélisme + Redis partagé)
//!
//! Chaque test a son org JETABLE et sa STATION synthétique (plage 999e9+). Redis
//! étant PARTAGÉ entre les instances de test, l'abonné de CHAQUE instance reçoit
//! tous les canaux d'org (`PSUBSCRIBE quarity:alerts:org:*`) MAIS ne route que vers
//! SES clients enregistrés, sous leur `org_id` — org unique par test ⇒ aucun message
//! d'un test ne peut atterrir chez un client d'un autre. La preuve cross-tenant
//! (org A vs org B) tient pour la même raison à l'échelle d'UN test.
//!
//! ⚠ Même mise en garde que B7 : si l'image du conteneur back local date d'après
//! B7, suspendre le back (`docker compose stop back`) avant un `cargo test` (sa
//! boucle de matching partage les bases). Côté pub/sub, un back pré-B8 ne s'abonne
//! ni ne publie — aucun parasitage des assertions WebSocket.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use futures_util::StreamExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

mod common;
use common::{access_token, create_test_org, pg_pool, spawn_app, spawn_app_with};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

// ─────────────────────────────────────────────────────────────────────────────
// Helpers (mêmes conventions que b7_matching.rs)
// ─────────────────────────────────────────────────────────────────────────────

static NEXT_STATION: AtomicU64 = AtomicU64::new(0);

fn new_station_id() -> i64 {
    let ts = Utc::now().timestamp() as u64;
    (999_000_000_000 + ts * 1_000 + NEXT_STATION.fetch_add(1, Ordering::Relaxed)) as i64
}

async fn register_station(pool: &PgPool, openaq_location_id: i64) {
    sqlx::query(
        "INSERT INTO ref_locations (openaq_location_id, name, country, city, latitude, longitude, timezone)
         VALUES ($1, $2, 'FR', 'Ville-Test', 43.7, 7.27, 'Europe/Paris')",
    )
    .bind(openaq_location_id)
    .bind(format!("Station e2e {openaq_location_id}"))
    .execute(pool)
    .await
    .expect("insertion station synthétique");
}

fn fmt_ch(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

/// Mesure « récente » : heure pile − 10 min (déterministe, dans la fenêtre).
fn recent_measured_at() -> DateTime<Utc> {
    let secs = Utc::now().timestamp();
    DateTime::<Utc>::from_timestamp(secs - secs.rem_euclid(3_600), 0).expect("heure pile")
        - ChronoDuration::minutes(10)
}

/// Insère une mesure brute dans ClickHouse (`ingested_at` = maintenant).
async fn insert_ch_measurement(location_id: i64, sensor_id: i64, parameter: &str, value: f64) {
    let sql = format!(
        "INSERT INTO quarity.measurements \
         (location_id, sensor_id, parameter, country, unit, measured_at, \
          value, latitude, longitude, ingested_at) \
         VALUES ({location_id}, {sensor_id}, '{parameter}', 'FR', 'µg/m³', '{}', {value}, 43.7, 7.27, '{}')",
        fmt_ch(recent_measured_at()),
        fmt_ch(Utc::now()),
    );
    let resp = reqwest::Client::new()
        .post(std::env::var("CLICKHOUSE_URL").expect("CLICKHOUSE_URL"))
        .basic_auth(
            std::env::var("CLICKHOUSE_USER").expect("CLICKHOUSE_USER"),
            Some(std::env::var("CLICKHOUSE_PASSWORD").expect("CLICKHOUSE_PASSWORD")),
        )
        .query(&[(
            "database",
            std::env::var("CLICKHOUSE_DB").expect("CLICKHOUSE_DB"),
        )])
        .body(sql)
        .send()
        .await
        .expect("requête HTTP ClickHouse");
    assert!(
        resp.status().is_success(),
        "insertion ClickHouse refusée : {}",
        resp.text().await.unwrap_or_default()
    );
}

/// Crée lieu suivi (1 station) + 1 règle `pm25 > 15` via l'API ; renvoie rule_id.
async fn create_location_and_rule(base: &str, token: &str, station: i64) -> i64 {
    let res = reqwest::Client::new()
        .post(format!("{base}/api/tracked-locations"))
        .bearer_auth(token)
        .json(&json!({
            "name": format!("Lieu WS {station}"),
            "openaq_location_ids": [station],
            "rules": [{
                "parameter": "pm25", "comparator": ">",
                "threshold_value": 15.0, "severity": "critical",
                "name": "Règle WS e2e"
            }]
        }))
        .send()
        .await
        .expect("create location");
    assert_eq!(res.status().as_u16(), 201, "création lieu+règle");
    let body: Value = res.json().await.expect("corps 201");
    let location_id = body["id"].as_i64().expect("location id");
    sqlx::query_scalar(
        "SELECT id FROM alert_rules WHERE tracked_location_id = $1 ORDER BY id LIMIT 1",
    )
    .bind(location_id)
    .fetch_one(&pg_pool().await)
    .await
    .expect("id de la règle créée")
}

/// `http://127.0.0.1:PORT` → `ws://127.0.0.1:PORT/api/ws?token=…`.
fn ws_url(base: &str, token: &str) -> String {
    format!(
        "{}/api/ws?token={}",
        base.replacen("http://", "ws://", 1),
        token
    )
}

/// Ouvre une WebSocket d'alertes ; panique si le handshake échoue (non-101).
async fn connect_ws(base: &str, token: &str) -> Ws {
    let (ws, _resp) = tokio_tungstenite::connect_async(ws_url(base, token))
        .await
        .expect("handshake WebSocket");
    ws
}

/// `true` si le serveur ferme la connexion (frame Close, flux terminé ou erreur) sous
/// le délai imparti ; `false` si rien ne vient (la connexion reste donc ouverte). Les
/// ping/pong sont ignorés sans consommer le verdict.
async fn is_closed_soon(ws: &mut Ws, within: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return false;
        }
        match tokio::time::timeout(remaining, ws.next()).await {
            Err(_) => return false,  // échéance : pas fermé
            Ok(None) => return true, // flux terminé
            Ok(Some(Ok(m))) if m.is_close() => return true,
            Ok(Some(Ok(_))) => {}            // ping/pong/texte : on reboucle
            Ok(Some(Err(_))) => return true, // erreur de transport = fermé
        }
    }
}

/// Force-check d'une règle (déclenche la publication si un event est créé).
async fn force_check(base: &str, token: &str, rule_id: i64) -> Value {
    let res = reqwest::Client::new()
        .post(format!("{base}/api/alert-rules/{rule_id}/run"))
        .bearer_auth(token)
        .send()
        .await
        .expect("force-check");
    assert_eq!(res.status().as_u16(), 200, "force-check 200");
    res.json().await.expect("corps force-check")
}

/// Prochain message TEXTE reçu sur la WS, sous un budget de temps global. `None`
/// si rien n'arrive à temps (les ping/pong/binaires sont ignorés sans consommer le
/// budget « message utile »… au sens où l'on reboucle jusqu'à l'échéance).
async fn next_text(ws: &mut Ws, within: Duration) -> Option<Value> {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }
        match tokio::time::timeout(remaining, ws.next()).await {
            Err(_) => return None,   // échéance atteinte
            Ok(None) => return None, // flux fermé
            Ok(Some(Ok(m))) => {
                if m.is_close() {
                    return None;
                }
                if m.is_text() {
                    let txt = m.into_text().expect("texte WS");
                    return Some(serde_json::from_str(txt.as_ref()).expect("message JSON"));
                }
                // ping / pong / binaire : on reboucle.
            }
            Ok(Some(Err(e))) => panic!("erreur WS : {e}"),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn breach_is_pushed_to_its_org_only() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;
    let token_a = access_token(&base, &org_a.admin_email).await;
    let token_b = access_token(&base, &org_b.admin_email).await;

    let station = new_station_id();
    register_station(&pool, station).await;
    let rule_id = create_location_and_rule(&base, &token_a, station).await;

    // Deux clients connectés AVANT le dépassement (org A et org B).
    let mut ws_a = connect_ws(&base, &token_a).await;
    let mut ws_b = connect_ws(&base, &token_b).await;
    // Laisse l'enregistrement des clients + l'abonnement Redis s'établir.
    tokio::time::sleep(Duration::from_millis(400)).await;

    insert_ch_measurement(station, 81, "pm25", 42.0).await;
    let outcome = force_check(&base, &token_a, rule_id).await;
    assert_eq!(outcome["events_created"].as_u64(), Some(1), "{outcome:?}");

    // org A reçoit l'alerte (snapshot complet, BON tenant).
    let msg = next_text(&mut ws_a, Duration::from_secs(5))
        .await
        .expect("org A doit recevoir l'alerte");
    assert_eq!(msg["type"], "alert");
    assert_eq!(msg["event"]["org_id"].as_i64(), Some(org_a.org_id));
    assert_eq!(msg["event"]["parameter_code"], "pm25");
    assert_eq!(msg["event"]["measured_value"].as_f64(), Some(42.0));
    assert_eq!(msg["event"]["openaq_location_id"].as_i64(), Some(station));
    assert_eq!(msg["event"]["severity"], "critical");
    assert!(
        msg["event"]["id"].as_i64().is_some(),
        "id de l'event présent"
    );

    // org B ne reçoit RIEN (isolation cross-tenant).
    assert!(
        next_text(&mut ws_b, Duration::from_secs(1)).await.is_none(),
        "org B ne doit recevoir aucune alerte d'une autre org"
    );
}

#[tokio::test]
async fn replay_does_not_push_twice() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;

    let station = new_station_id();
    register_station(&pool, station).await;
    let rule_id = create_location_and_rule(&base, &token, station).await;

    let mut ws = connect_ws(&base, &token).await;
    tokio::time::sleep(Duration::from_millis(400)).await;

    insert_ch_measurement(station, 82, "pm25", 30.0).await;

    // 1er force-check : 1 event créé → 1 push.
    let first = force_check(&base, &token, rule_id).await;
    assert_eq!(first["events_created"].as_u64(), Some(1));
    assert!(
        next_text(&mut ws, Duration::from_secs(5)).await.is_some(),
        "1er push attendu"
    );

    // Rejouage : 0 event créé (idempotence 0006) → AUCUN push.
    let second = force_check(&base, &token, rule_id).await;
    assert_eq!(second["events_created"].as_u64(), Some(0));
    assert!(
        next_text(&mut ws, Duration::from_secs(1)).await.is_none(),
        "un rejouage ne doit RIEN republier"
    );
}

#[tokio::test]
async fn same_org_two_clients_both_receive() {
    // Fan-out : deux clients de la MÊME org (p. ex. deux onglets) doivent TOUS DEUX
    // recevoir l'alerte — le hub itère sur tous les clients enregistrés sous l'org.
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;

    let station = new_station_id();
    register_station(&pool, station).await;
    let rule_id = create_location_and_rule(&base, &token, station).await;

    let mut ws1 = connect_ws(&base, &token).await;
    let mut ws2 = connect_ws(&base, &token).await;
    tokio::time::sleep(Duration::from_millis(400)).await;

    insert_ch_measurement(station, 83, "pm25", 40.0).await;
    let outcome = force_check(&base, &token, rule_id).await;
    assert_eq!(outcome["events_created"].as_u64(), Some(1), "{outcome:?}");

    let m1 = next_text(&mut ws1, Duration::from_secs(5))
        .await
        .expect("ws1 doit recevoir l'alerte");
    let m2 = next_text(&mut ws2, Duration::from_secs(5))
        .await
        .expect("ws2 doit recevoir l'alerte");
    assert_eq!(m1["event"]["org_id"].as_i64(), Some(org.org_id));
    assert_eq!(m2["event"]["org_id"].as_i64(), Some(org.org_id));
    assert_eq!(m1["event"]["measured_value"].as_f64(), Some(40.0));
    assert_eq!(m2["event"]["measured_value"].as_f64(), Some(40.0));
}

#[tokio::test]
async fn ws_without_valid_token_is_rejected() {
    let base = spawn_app().await;

    // Jeton bidon : le décodage JWT échoue → 401, pas de bascule 101 → handshake KO.
    let bogus = ws_url(&base, "pas-un-jwt");
    assert!(
        tokio_tungstenite::connect_async(bogus).await.is_err(),
        "un jeton invalide ne doit pas établir la WebSocket"
    );
}

#[tokio::test]
async fn connection_cap_rejects_beyond_limit() {
    // Durcissement B8b : au-delà du cap par org, la WebSocket est refusée (Close 1008),
    // ce qui borne le registre in-process (anti-DoS mémoire par un tenant authentifié).
    // Cap volontairement bas (2) pour CE back de test ; rate-limits relevés (Redis +
    // IP 127.0.0.1 partagés entre tests parallèles, cf. spawn_app).
    let base = spawn_app_with(|c| {
        c.rate_limit_login_email_per_min = 10_000;
        c.rate_limit_login_ip_per_min = 10_000;
        c.rate_limit_refresh_ip_per_min = 10_000;
        c.ws_max_connections_per_org = 2;
    })
    .await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;

    // Deux connexions sous le cap : acceptées et MAINTENUES (on garde les handles).
    let mut ws1 = connect_ws(&base, &token).await;
    let mut ws2 = connect_ws(&base, &token).await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    // La 3e : le handshake HTTP réussit (101), mais le serveur ferme AUSSITÔT (cap atteint).
    let mut ws3 = connect_ws(&base, &token).await;
    assert!(
        is_closed_soon(&mut ws3, Duration::from_secs(2)).await,
        "la 3e connexion d'une même org doit être fermée par le serveur (cap atteint)"
    );

    // Les deux premières restent ouvertes (le refus de la 3e ne les a pas affectées).
    assert!(
        !is_closed_soon(&mut ws1, Duration::from_millis(500)).await,
        "ws1 doit rester ouverte"
    );
    assert!(
        !is_closed_soon(&mut ws2, Duration::from_millis(500)).await,
        "ws2 doit rester ouverte"
    );
}

#[tokio::test]
async fn connection_cap_is_per_org() {
    // Le cap est PAR ORG (durcissement B8b) : une org saturée n'empêche pas une AUTRE
    // org de se connecter. Cap=1 : org A prend son unique slot, sa 2e est refusée, mais
    // org B garde son propre slot intact.
    let base = spawn_app_with(|c| {
        c.rate_limit_login_email_per_min = 10_000;
        c.rate_limit_login_ip_per_min = 10_000;
        c.rate_limit_refresh_ip_per_min = 10_000;
        c.ws_max_connections_per_org = 1;
    })
    .await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;
    let token_a = access_token(&base, &org_a.admin_email).await;
    let token_b = access_token(&base, &org_b.admin_email).await;

    // org A : 1re connexion acceptée (et maintenue), 2e refusée (cap=1 atteint).
    let mut ws_a1 = connect_ws(&base, &token_a).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut ws_a2 = connect_ws(&base, &token_a).await;
    assert!(
        is_closed_soon(&mut ws_a2, Duration::from_secs(2)).await,
        "la 2e connexion d'org A doit être refusée (cap=1)"
    );

    // org B : sa propre connexion passe — le cap d'org A ne la concerne pas.
    let mut ws_b1 = connect_ws(&base, &token_b).await;
    assert!(
        !is_closed_soon(&mut ws_b1, Duration::from_millis(500)).await,
        "org B doit pouvoir se connecter malgré org A au cap"
    );
    // org A reste connectée.
    assert!(
        !is_closed_soon(&mut ws_a1, Duration::from_millis(500)).await,
        "la connexion d'org A doit rester ouverte"
    );
}
