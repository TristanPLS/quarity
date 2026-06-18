//! Tests e2e de `GET /api/alert-events` (4b) — backfill du panneau temps réel B11a.
//!
//! Prérequis : identiques à `tests/e2e.rs` (3 bases réelles + schéma + seed).
//! Chaque test crée ses orgs JETABLES (cf. `tests/common/mod.rs`) : la seed n'est
//! JAMAIS mutée, les tests restent parallélisables. Les événements sont insérés
//! directement en base (le setup ne passe pas par l'API — la boucle de matching B7
//! a sa propre suite `b7_matching.rs`) ; on ne teste ici que la LECTURE scopée org.

use serde_json::Value;
use sqlx::PgPool;

mod common;
use common::{access_token, create_test_org, pg_pool, spawn_app};

/// Insère un `alert_event` minimal pour une org, en contrôlant `fired_at` (ordre) et
/// `openaq_sensor_id` (`None` exerce le chemin nullable). `alert_rule_id` et
/// `tracked_location_id` restent NULL : le trigger T7 ne vérifie la cohérence d'org
/// que pour les référents NON-NULL — un event « orphelin » (référents purgés) est
/// exactement ce que la FK `ON DELETE SET NULL` produit en service.
async fn insert_event(
    pool: &PgPool,
    org_id: i64,
    fired_at: &str,
    severity: &str,
    sensor_id: Option<i64>,
) {
    sqlx::query(
        r#"
        INSERT INTO alert_events
            (org_id, ref_location_id, openaq_location_id, openaq_sensor_id,
             parameter_code, measured_value, unit, measured_at,
             threshold_value, comparator, severity, fired_at)
        VALUES ($1, 1, 1001, $2, 'pm25', 50.0, 'µg/m³', $3::timestamptz,
                15.0, '>', $4, $3::timestamptz)
        "#,
    )
    .bind(org_id)
    .bind(sensor_id)
    .bind(fired_at)
    .bind(severity)
    .execute(pool)
    .await
    .expect("insertion alert_event de test");
}

/// GET (path relatif) avec Bearer → (status, corps JSON éventuel).
async fn get_json(base: &str, token: &str, path: &str) -> (u16, Option<Value>) {
    let res = reqwest::Client::new()
        .get(format!("{base}{path}"))
        .bearer_auth(token)
        .send()
        .await
        .expect("requête GET");
    (res.status().as_u16(), res.json::<Value>().await.ok())
}

/// Une org ne voit QUE ses propres événements, triés `fired_at` décroissant, avec
/// les colonnes nullables correctement sérialisées (FK historisées + capteur).
#[tokio::test]
async fn alert_events_are_org_scoped_and_ordered_by_fired_at_desc() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org_a = create_test_org(&pool).await;
    let org_b = create_test_org(&pool).await;

    // Org A : 3 events, fired_at croissant (info < warning < critical).
    insert_event(
        &pool,
        org_a.org_id,
        "2026-01-01 10:00:00+00",
        "info",
        Some(5001),
    )
    .await;
    insert_event(
        &pool,
        org_a.org_id,
        "2026-01-01 11:00:00+00",
        "warning",
        None,
    )
    .await;
    insert_event(
        &pool,
        org_a.org_id,
        "2026-01-01 12:00:00+00",
        "critical",
        Some(5003),
    )
    .await;
    // Org B : 1 event (ne doit JAMAIS apparaître pour A).
    insert_event(
        &pool,
        org_b.org_id,
        "2026-01-01 13:00:00+00",
        "critical",
        Some(9999),
    )
    .await;

    let token_a = access_token(&base, &org_a.admin_email).await;
    let (status, body) = get_json(&base, &token_a, "/api/alert-events").await;
    assert_eq!(status, 200, "lecture des alertes : {body:?}");
    let body = body.expect("corps de la page");

    assert_eq!(body["total"], 3, "org A ne compte QUE ses 3 événements");
    assert_eq!(body["count"], 3);
    let data = body["data"].as_array().expect("data est un tableau");
    assert_eq!(data.len(), 3);

    // Ordre par défaut `fired_at` décroissant : critical (12h), warning (11h), info (10h).
    assert_eq!(data[0]["severity"], "critical");
    assert_eq!(data[1]["severity"], "warning");
    assert_eq!(data[2]["severity"], "info");

    // Colonnes nullables sérialisées : FK historisées NULL ; le capteur de l'event
    // « warning » est null, ceux des autres sont des entiers.
    assert!(
        data[0]["alert_rule_id"].is_null(),
        "alert_rule_id null (orphelin)"
    );
    assert!(
        data[0]["tracked_location_id"].is_null(),
        "tracked_location_id null"
    );
    assert_eq!(data[0]["openaq_sensor_id"], 5003);
    assert!(
        data[1]["openaq_sensor_id"].is_null(),
        "capteur null sérialisé"
    );

    // Champs castés `::float8` exposés comme des nombres (pas des chaînes NUMERIC).
    assert_eq!(data[0]["measured_value"], 50.0);
    assert_eq!(data[0]["threshold_value"], 15.0);
    assert_eq!(data[0]["org_id"], org_a.org_id);

    // Isolation symétrique : org B ne voit QUE son unique événement.
    let token_b = access_token(&base, &org_b.admin_email).await;
    let (status_b, body_b) = get_json(&base, &token_b, "/api/alert-events").await;
    assert_eq!(status_b, 200);
    assert_eq!(body_b.expect("corps B")["total"], 1, "org B isolée de A");
}

/// La pagination borne `data` (et `total` reste le total filtré) ; sans Bearer, 401.
#[tokio::test]
async fn alert_events_pagination_bounds_and_requires_auth() {
    let base = spawn_app().await;
    let pool = pg_pool().await;
    let org = create_test_org(&pool).await;

    for h in 0..3 {
        let fired = format!("2026-02-0{} 09:00:00+00", h + 1);
        insert_event(&pool, org.org_id, &fired, "info", Some(7000 + h)).await;
    }

    let token = access_token(&base, &org.admin_email).await;
    let (status, body) = get_json(&base, &token, "/api/alert-events?page_size=2").await;
    assert_eq!(status, 200);
    let body = body.expect("corps paginé");
    assert_eq!(body["page_size"], 2);
    assert_eq!(body["count"], 2, "page bornée à 2 lignes");
    assert_eq!(body["total"], 3, "total = lignes filtrées, toutes pages");
    assert_eq!(body["data"].as_array().expect("data").len(), 2);

    // Sans Bearer : 401 (route protégée par AuthUser).
    let res = reqwest::Client::new()
        .get(format!("{base}/api/alert-events"))
        .send()
        .await
        .expect("requête sans token");
    assert_eq!(res.status().as_u16(), 401, "alert-events exige un Bearer");
}
