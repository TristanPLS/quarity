//! Tests e2e de la boucle de matching B7 (`back/src/matching.rs`).
//!
//! Prérequis : identiques à `tests/e2e.rs` (3 bases réelles + schéma + seed).
//!
//! ## Isolation des tests (parallélisme)
//!
//! - chaque test crée son org JETABLE (cf. `tests/common/mod.rs`) et sa propre
//!   STATION synthétique (`ref_locations`, id réservé unique par process) — la
//!   seed n'est jamais mutée ;
//! - les évaluations passent par `load_rule_index_for_rule` (l'index d'UNE
//!   règle) : les mesures des autres tests, relues par la même fenêtre
//!   ClickHouse, ne rencontrent JAMAIS nos règles — les compteurs
//!   `breaches`/`inserted` restent déterministes même en parallèle
//!   (`evaluated`, lui, est global : jamais asserté en valeur exacte).
//!
//! ## Ce que la suite prouve
//!
//! snapshot complet + compteur T6, idempotence (0006), bornes `>`/`>=`,
//! périmètre (inactif/pause = non évaluable), cross-tenant au niveau BOUCLE
//! (station étrangère → rien), backstop **T7** à travers le chemin d'insertion
//! de la boucle (la dette notée à la création de T7), et le force-check API.
//!
//! ⚠️ Conteneur local : si l'image du back date d'APRÈS B7, sa boucle de
//! matching (tick 300 s) partage cette base et peut « griller » nos insertions
//! (dédup oblige, `inserted` tomberait à 0 → assertions flaky). Avant un
//! `cargo test` local : `docker compose stop back` (ou MATCHING_INTERVAL_SECS=0).
//! La CI n'est pas concernée (services bases seules, pas de conteneur back).

use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;

use quarity_back::ch::ClickhouseClient;
use quarity_back::matching::{
    insert_events, load_rule_index_for_rule, run_once, Breach, Comparator, StationRule,
};

mod common;
use common::{access_token, create_test_org, pg_pool, spawn_app};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers : station synthétique, insertion ClickHouse, créations via API
// ─────────────────────────────────────────────────────────────────────────────

/// Compteur process-local : combiné au timestamp, garantit l'unicité des ids
/// de station synthétiques entre tests parallèles ET entre exécutions
/// (même plage réservée 999e9+ que `tests/ch.rs`).
static NEXT_STATION: AtomicU64 = AtomicU64::new(0);

fn new_station_id() -> i64 {
    let ts = Utc::now().timestamp() as u64;
    (999_000_000_000 + ts * 1_000 + NEXT_STATION.fetch_add(1, Ordering::Relaxed)) as i64
}

/// Enregistre une station synthétique dans le référentiel Postgres (prérequis
/// de P1 — le POST /api/tracked-locations résout les stations par clé OpenAQ).
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

fn ch_client() -> ClickhouseClient {
    ClickhouseClient::new(
        std::env::var("CLICKHOUSE_URL").expect("CLICKHOUSE_URL"),
        std::env::var("CLICKHOUSE_USER").expect("CLICKHOUSE_USER"),
        std::env::var("CLICKHOUSE_PASSWORD").expect("CLICKHOUSE_PASSWORD"),
        std::env::var("CLICKHOUSE_DB").expect("CLICKHOUSE_DB"),
    )
}

fn fmt_ch(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

/// Insère une mesure brute dans ClickHouse (`ingested_at` = maintenant : la
/// fenêtre `since` des tests la voit ; unité canonique µg/m³ comme l'ingestion
/// stricte D4.2 la garantirait).
async fn insert_ch_measurement(
    location_id: i64,
    sensor_id: i64,
    parameter: &str,
    measured_at: DateTime<Utc>,
    value: f64,
) {
    let sql = format!(
        "INSERT INTO quarity.measurements \
         (location_id, sensor_id, parameter, country, unit, measured_at, \
          value, latitude, longitude, ingested_at) \
         VALUES ({location_id}, {sensor_id}, '{parameter}', 'FR', 'µg/m³', '{}', {value}, 43.7, 7.27, '{}')",
        fmt_ch(measured_at),
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

/// Crée lieu suivi (1 station) + 1 règle via l'API ; renvoie (location_id, rule_id).
async fn create_location_and_rule(
    base: &str,
    token: &str,
    station: i64,
    parameter: &str,
    comparator: &str,
    threshold: f64,
) -> (i64, i64) {
    let res = reqwest::Client::new()
        .post(format!("{base}/api/tracked-locations"))
        .bearer_auth(token)
        .json(&json!({
            "name": format!("Lieu matching {station}"),
            "openaq_location_ids": [station],
            "rules": [{
                "parameter": parameter, "comparator": comparator,
                "threshold_value": threshold, "severity": "critical",
                "name": "Règle matching e2e"
            }]
        }))
        .send()
        .await
        .expect("requête create location");
    assert_eq!(res.status().as_u16(), 201, "création lieu+règle");
    let body: Value = res.json().await.expect("corps 201");
    let location_id = body["id"].as_i64().expect("location id");

    let rule_id: i64 = sqlx::query_scalar(
        "SELECT id FROM alert_rules WHERE tracked_location_id = $1 ORDER BY id LIMIT 1",
    )
    .bind(location_id)
    .fetch_one(&pg_pool().await)
    .await
    .expect("id de la règle créée");
    (location_id, rule_id)
}

/// Mesure « récente » : à l'heure pile − 10 min (déterministe dans la fenêtre,
/// loin des bords de TTL/fenêtres).
fn recent_measured_at() -> DateTime<Utc> {
    let secs = Utc::now().timestamp();
    DateTime::<Utc>::from_timestamp(secs - secs.rem_euclid(3_600), 0).expect("heure pile")
        - Duration::minutes(10)
}

/// Fenêtre d'évaluation des tests : tout ce qui est ARRIVÉ depuis 10 min.
fn since() -> DateTime<Utc> {
    Utc::now() - Duration::minutes(10)
}

// ─────────────────────────────────────────────────────────────────────────────
// Boucle : snapshot, compteur T6, idempotence
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn breach_creates_snapshot_event_and_bumps_unread_counter() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;

    let station = new_station_id();
    register_station(&pool, station).await;
    let (location_id, rule_id) =
        create_location_and_rule(&base, &token, station, "pm25", ">", 15.0).await;

    let measured_at = recent_measured_at();
    insert_ch_measurement(station, 71, "pm25", measured_at, 22.5).await;

    let index = load_rule_index_for_rule(&pool, org.org_id, rule_id)
        .await
        .expect("index de la règle");
    let outcome = run_once(&pool, &ch_client(), &index, since(), 50_000)
        .await
        .expect("run_once");
    assert_eq!(outcome.breaches, 1, "un dépassement attendu");
    assert_eq!(outcome.inserted, 1, "un événement créé");

    // Snapshot COMPLET : tout ce que T4 figera est posé au moment du match.
    let row: (
        i64,
        i64,
        String,
        f64,
        String,
        String,
        String,
        DateTime<Utc>,
        f64,
        bool,
    ) = sqlx::query_as(
        r#"SELECT org_id, openaq_location_id, parameter_code, measured_value::float8,
                      unit, comparator, severity, measured_at, threshold_value::float8, is_read
               FROM alert_events WHERE alert_rule_id = $1"#,
    )
    .bind(rule_id)
    .fetch_one(&pool)
    .await
    .expect("événement créé");
    assert_eq!(row.0, org.org_id, "org = celle de la RÈGLE");
    assert_eq!(row.1, station);
    assert_eq!(row.2, "pm25");
    assert_eq!(row.3, 22.5);
    assert_eq!(row.4, "µg/m³");
    assert_eq!(row.5, ">");
    assert_eq!(row.6, "critical");
    assert_eq!(
        row.7.to_rfc3339_opts(SecondsFormat::Secs, true),
        measured_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        "measured_at du snapshot = celui de la mesure"
    );
    assert_eq!(row.8, 15.0);
    assert!(!row.9, "événement non lu à la création");

    // T6 : le compteur de non-lus de l'org (fraîche : 0 au départ) suit l'INSERT.
    let unread: i32 =
        sqlx::query_scalar("SELECT unread_alert_count FROM organizations WHERE id = $1")
            .bind(org.org_id)
            .fetch_one(&pool)
            .await
            .expect("compteur org");
    assert_eq!(unread, 1, "T6 doit avoir incrémenté le compteur");

    // tracked_location_id du snapshot pointe bien notre lieu.
    let tl: i64 =
        sqlx::query_scalar("SELECT tracked_location_id FROM alert_events WHERE alert_rule_id = $1")
            .bind(rule_id)
            .fetch_one(&pool)
            .await
            .expect("tl id");
    assert_eq!(tl, location_id);
}

#[tokio::test]
async fn rerun_is_idempotent() {
    // La fenêtre d'ingestion revoit les mêmes mesures à chaque tick : rejouer
    // la MÊME fenêtre ne doit rien créer (index 0006 + ON CONFLICT DO NOTHING).
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;

    let station = new_station_id();
    register_station(&pool, station).await;
    let (_, rule_id) = create_location_and_rule(&base, &token, station, "pm25", ">", 15.0).await;
    insert_ch_measurement(station, 72, "pm25", recent_measured_at(), 30.0).await;

    let index = load_rule_index_for_rule(&pool, org.org_id, rule_id)
        .await
        .expect("index");
    let ch = ch_client();

    let first = run_once(&pool, &ch, &index, since(), 50_000)
        .await
        .expect("run 1");
    assert_eq!(first.inserted, 1);

    let second = run_once(&pool, &ch, &index, since(), 50_000)
        .await
        .expect("run 2");
    assert_eq!(second.breaches, 1, "le dépassement est revu…");
    assert_eq!(second.inserted, 0, "…mais AUCUN doublon n'est créé");

    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM alert_events WHERE alert_rule_id = $1")
            .bind(rule_id)
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(count, 1);

    let unread: i32 =
        sqlx::query_scalar("SELECT unread_alert_count FROM organizations WHERE id = $1")
            .bind(org.org_id)
            .fetch_one(&pool)
            .await
            .expect("compteur org");
    assert_eq!(unread, 1, "T6 ne doit pas avoir doublé");
}

#[tokio::test]
async fn comparator_boundary_gt_does_not_fire_ge_does() {
    // valeur == seuil : `>` ne déclenche PAS, `>=` déclenche (US-02 c1).
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;

    let station = new_station_id();
    register_station(&pool, station).await;
    let (location_id, rule_gt) =
        create_location_and_rule(&base, &token, station, "pm25", ">", 20.0).await;

    // Seconde règle `>=` même lieu/polluant/seuil (uq_alert_rule distingue le comparateur).
    let res = reqwest::Client::new()
        .post(format!("{base}/api/alert-rules"))
        .bearer_auth(&token)
        .json(&json!({
            "tracked_location_id": location_id, "parameter": "pm25",
            "comparator": ">=", "threshold_value": 20.0
        }))
        .send()
        .await
        .expect("création règle >=");
    assert_eq!(res.status().as_u16(), 201);
    let rule_ge = res.json::<Value>().await.expect("corps")["id"]
        .as_i64()
        .expect("id");

    insert_ch_measurement(station, 73, "pm25", recent_measured_at(), 20.0).await;
    let ch = ch_client();

    let idx_gt = load_rule_index_for_rule(&pool, org.org_id, rule_gt)
        .await
        .expect("idx >");
    let out_gt = run_once(&pool, &ch, &idx_gt, since(), 50_000)
        .await
        .expect("run >");
    assert_eq!(
        (out_gt.breaches, out_gt.inserted),
        (0, 0),
        "20.0 > 20.0 est faux"
    );

    let idx_ge = load_rule_index_for_rule(&pool, org.org_id, rule_ge)
        .await
        .expect("idx >=");
    let out_ge = run_once(&pool, &ch, &idx_ge, since(), 50_000)
        .await
        .expect("run >=");
    assert_eq!(
        (out_ge.breaches, out_ge.inserted),
        (1, 1),
        "20.0 >= 20.0 est vrai"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Périmètre : inactif / pause / station étrangère
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn inactive_rule_and_paused_location_are_not_evaluable() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;
    let client = reqwest::Client::new();

    let station = new_station_id();
    register_station(&pool, station).await;
    let (location_id, rule_id) =
        create_location_and_rule(&base, &token, station, "pm25", ">", 15.0).await;

    // Règle désactivée → hors périmètre.
    let res = client
        .patch(format!("{base}/api/alert-rules/{rule_id}"))
        .bearer_auth(&token)
        .json(&json!({ "status": "inactive" }))
        .send()
        .await
        .expect("PATCH inactive");
    assert_eq!(res.status().as_u16(), 200);
    let idx = load_rule_index_for_rule(&pool, org.org_id, rule_id)
        .await
        .expect("idx");
    assert!(idx.is_empty(), "règle inactive = non évaluable");

    // Règle réactivée mais LIEU en pause → hors périmètre aussi.
    let res = client
        .patch(format!("{base}/api/alert-rules/{rule_id}"))
        .bearer_auth(&token)
        .json(&json!({ "status": "active" }))
        .send()
        .await
        .expect("PATCH active");
    assert_eq!(res.status().as_u16(), 200);
    let res = client
        .patch(format!("{base}/api/tracked-locations/{location_id}"))
        .bearer_auth(&token)
        .json(&json!({ "is_active": false }))
        .send()
        .await
        .expect("PATCH pause lieu");
    assert_eq!(res.status().as_u16(), 200);
    let idx = load_rule_index_for_rule(&pool, org.org_id, rule_id)
        .await
        .expect("idx");
    assert!(idx.is_empty(), "lieu en pause = règles dormantes");
}

#[tokio::test]
async fn foreign_station_measurement_never_matches() {
    // Cross-tenant AU NIVEAU BOUCLE : une mesure d'une station que le lieu de
    // la règle ne suit PAS (p. ex. la station d'une autre org) ne rencontre
    // jamais la règle — l'index est construit par (station du lieu, polluant).
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;

    let station_mine = new_station_id();
    let station_foreign = new_station_id();
    register_station(&pool, station_mine).await;
    register_station(&pool, station_foreign).await;
    let (_, rule_id) =
        create_location_and_rule(&base, &token, station_mine, "pm25", ">", 15.0).await;

    // Valeur ÉNORME sur la station étrangère : si la boucle fuyait, ça matcherait.
    insert_ch_measurement(station_foreign, 74, "pm25", recent_measured_at(), 999.0).await;

    let index = load_rule_index_for_rule(&pool, org.org_id, rule_id)
        .await
        .expect("idx");
    let outcome = run_once(&pool, &ch_client(), &index, since(), 50_000)
        .await
        .expect("run_once");
    assert_eq!((outcome.breaches, outcome.inserted), (0, 0));

    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM alert_events WHERE alert_rule_id = $1")
            .bind(rule_id)
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(count, 0, "aucun événement pour la règle");
}

// ─────────────────────────────────────────────────────────────────────────────
// LE rempart en profondeur : T7 backstoppe le chemin d'insertion de la boucle
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn t7_backstops_forged_cross_tenant_event_through_loop_insert() {
    // Dette notée à la création de T7 (#30) : « T7 garde la BASE, pas la
    // boucle ». Ici on PROUVE le backstop À TRAVERS le chemin d'écriture de la
    // boucle : un Breach forgé dont l'org ne correspond pas au lieu de la règle
    // (simulation d'un futur bug de compilation d'index) doit être REJETÉ par
    // T7 — erreur SQL check_violation, zéro ligne écrite, compteur T6 intact.
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;
    let token_a = access_token(&base, &org_a.admin_email).await;

    let station = new_station_id();
    register_station(&pool, station).await;
    let (location_a, rule_a) =
        create_location_and_rule(&base, &token_a, station, "pm25", ">", 15.0).await;
    let ref_location_id: i64 =
        sqlx::query_scalar("SELECT id FROM ref_locations WHERE openaq_location_id = $1")
            .bind(station)
            .fetch_one(&pool)
            .await
            .expect("ref id");

    let forged = Breach {
        rule: StationRule {
            rule_id: rule_a,
            org_id: org_b.org_id, // ← MENSONGE cross-tenant : la règle est à A
            tracked_location_id: location_a,
            ref_location_id,
            openaq_location_id: station,
            parameter: "pm25".into(),
            comparator: Comparator::Gt,
            threshold: 15.0,
            severity: "critical".into(),
        },
        openaq_sensor_id: 75,
        value: 99.0,
        unit: "µg/m³".into(),
        measured_at: recent_measured_at(),
    };

    let err = insert_events(&pool, &[forged])
        .await
        .expect_err("T7 doit rejeter un événement dont l'org ≠ celle du lieu de la règle");
    let db_err = err.as_database_error().expect("erreur Postgres attendue");
    assert_eq!(
        db_err.code().as_deref(),
        Some("23514"),
        "check_violation (QRT_T7) attendue : {db_err}"
    );
    assert!(
        db_err.message().contains("QRT_T7"),
        "message du trigger T7 attendu : {}",
        db_err.message()
    );

    // Rien n'est passé : ni événement, ni incrément T6 — pour AUCUNE des 2 orgs.
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM alert_events WHERE alert_rule_id = $1")
            .bind(rule_a)
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(count, 0);
    for org_id in [org_a.org_id, org_b.org_id] {
        let unread: i32 =
            sqlx::query_scalar("SELECT unread_alert_count FROM organizations WHERE id = $1")
                .bind(org_id)
                .fetch_one(&pool)
                .await
                .expect("compteur org");
        assert_eq!(unread, 0, "compteur de l'org {org_id} intact");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Force-check API : POST /api/alert-rules/{id}/run
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn force_check_endpoint_creates_then_zero_on_replay() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;
    let token = access_token(&base, &org.admin_email).await;
    let client = reqwest::Client::new();

    let station = new_station_id();
    register_station(&pool, station).await;
    let (_, rule_id) = create_location_and_rule(&base, &token, station, "pm25", ">", 15.0).await;
    insert_ch_measurement(station, 76, "pm25", recent_measured_at(), 42.0).await;

    let res = client
        .post(format!("{base}/api/alert-rules/{rule_id}/run"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("force-check 1");
    assert_eq!(res.status().as_u16(), 200);
    let body: Value = res.json().await.expect("corps");
    assert_eq!(body["rule_id"].as_i64(), Some(rule_id));
    assert_eq!(body["breaches"].as_u64(), Some(1), "{body:?}");
    assert_eq!(body["events_created"].as_u64(), Some(1), "{body:?}");

    // Rejouage : revu mais rien de créé (idempotence de bout en bout via l'API).
    let res = client
        .post(format!(
            "{base}/api/alert-rules/{rule_id}/run?lookback_hours=24"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .expect("force-check 2");
    assert_eq!(res.status().as_u16(), 200);
    let body: Value = res.json().await.expect("corps 2");
    assert_eq!(body["events_created"].as_u64(), Some(0), "{body:?}");
}

#[tokio::test]
async fn force_check_is_guarded_and_validated() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;
    let admin_a = access_token(&base, &org_a.admin_email).await;
    let admin_b = access_token(&base, &org_b.admin_email).await;
    let reader_a = access_token(&base, &org_a.reader_email).await;
    let client = reqwest::Client::new();

    let station = new_station_id();
    register_station(&pool, station).await;
    let (_, rule_id) = create_location_and_rule(&base, &admin_a, station, "pm25", ">", 15.0).await;

    // Lecteur : 403 read_only_role (mutation opérationnelle).
    let res = client
        .post(format!("{base}/api/alert-rules/{rule_id}/run"))
        .bearer_auth(&reader_a)
        .send()
        .await
        .expect("run lecteur");
    assert_eq!(res.status().as_u16(), 403);

    // Autre org : 404 (anti-énumération), pas 403.
    let res = client
        .post(format!("{base}/api/alert-rules/{rule_id}/run"))
        .bearer_auth(&admin_b)
        .send()
        .await
        .expect("run cross-org");
    assert_eq!(res.status().as_u16(), 404);

    // lookback hors borne : 400.
    let res = client
        .post(format!(
            "{base}/api/alert-rules/{rule_id}/run?lookback_hours=0"
        ))
        .bearer_auth(&admin_a)
        .send()
        .await
        .expect("run lookback 0");
    assert_eq!(res.status().as_u16(), 400);

    // Règle inactive : 422 (elle EXISTE — pas un 404 — mais n'est pas évaluable).
    let res = client
        .patch(format!("{base}/api/alert-rules/{rule_id}"))
        .bearer_auth(&admin_a)
        .json(&json!({ "status": "inactive" }))
        .send()
        .await
        .expect("PATCH inactive");
    assert_eq!(res.status().as_u16(), 200);
    let res = client
        .post(format!("{base}/api/alert-rules/{rule_id}/run"))
        .bearer_auth(&admin_a)
        .send()
        .await
        .expect("run inactive");
    assert_eq!(res.status().as_u16(), 422);
    let body: Value = res.json().await.expect("corps 422");
    assert_eq!(body["error"], "unprocessable_entity");

    // Sans Bearer : 401.
    let res = client
        .post(format!("{base}/api/alert-rules/{rule_id}/run"))
        .send()
        .await
        .expect("run sans Bearer");
    assert_eq!(res.status().as_u16(), 401);
}
