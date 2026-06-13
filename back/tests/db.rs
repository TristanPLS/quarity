//! Tests d'intégration de la couche BDD du Jalon 3 (backlog B1–B4) :
//! vues métier, triggers T1–T6, procédures P1–P3, requête complexe.
//!
//! ⚠️ Nécessitent un Postgres RÉEL, schéma appliqué (migrations 0001→0004) et
//! seedé (`db/sql/02_seed.sql`). Même environnement que `e2e.rs` : DATABASE_URL.
//!
//! Isolation : chaque test ouvre une TRANSACTION et ne committe jamais (drop =
//! rollback) — aucune trace en base, exécutable en parallèle. Les assertions
//! sont relatives (deltas, auto-cohérence) pour tolérer des données locales
//! au-delà du seed (ex. ingestion réelle Nice #4085).

use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

async fn pool() -> PgPool {
    PgPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL"))
        .await
        .expect("connexion Postgres de test (base seedée requise)")
}

/// id d'une org du seed par slug.
async fn org_id(tx: &mut sqlx::PgConnection, slug: &str) -> i64 {
    sqlx::query("SELECT id FROM organizations WHERE slug = $1")
        .bind(slug)
        .fetch_one(tx)
        .await
        .unwrap_or_else(|e| panic!("org {slug} absente du seed : {e}"))
        .get(0)
}

/// id d'un lieu suivi du seed par nom.
async fn location_id(tx: &mut sqlx::PgConnection, name: &str) -> i64 {
    sqlx::query("SELECT id FROM tracked_locations WHERE name = $1")
        .bind(name)
        .fetch_one(tx)
        .await
        .unwrap_or_else(|e| panic!("lieu {name} absent du seed : {e}"))
        .get(0)
}

// ─────────────────────────────────────────────────────────────────────────────
// T1 — invariant « un lieu actif garde ≥ 1 station »
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn t1_cannot_remove_last_station_of_active_location() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    // 'École Jules-Ferry' n'a qu'une station (seed) : le retrait doit être refusé.
    let loc = location_id(&mut tx, "École Jules-Ferry").await;
    let err = sqlx::query("DELETE FROM tracked_location_stations WHERE tracked_location_id = $1")
        .bind(loc)
        .execute(&mut *tx)
        .await
        .expect_err("la dernière station d'un lieu actif ne doit pas être supprimable");
    assert!(
        err.to_string().contains("QRT_T1"),
        "erreur T1 attendue : {err}"
    );
}

#[tokio::test]
async fn t1_can_remove_station_while_another_remains() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    // 'Centre-ville' agrège 2 stations (seed) : retirer la non-primaire passe…
    let loc = location_id(&mut tx, "Centre-ville").await;
    let res = sqlx::query(
        "DELETE FROM tracked_location_stations
          WHERE tracked_location_id = $1 AND is_primary IS FALSE",
    )
    .bind(loc)
    .execute(&mut *tx)
    .await
    .expect("le retrait doit passer tant qu'il reste une station");
    assert_eq!(res.rows_affected(), 1);

    // …mais retirer la dernière est refusé.
    let err = sqlx::query("DELETE FROM tracked_location_stations WHERE tracked_location_id = $1")
        .bind(loc)
        .execute(&mut *tx)
        .await
        .expect_err("le retrait de la dernière station doit échouer");
    assert!(
        err.to_string().contains("QRT_T1"),
        "erreur T1 attendue : {err}"
    );
}

#[tokio::test]
async fn t1_location_delete_cascade_is_still_allowed() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    // Supprimer le LIEU lui-même reste permis : la cascade retire ses stations
    // après disparition du parent (T1 laisse passer), les events passent à
    // tracked_location_id NULL (transition autorisée par T4).
    let loc = location_id(&mut tx, "École Jules-Ferry").await;
    sqlx::query("DELETE FROM tracked_locations WHERE id = $1")
        .bind(loc)
        .execute(&mut *tx)
        .await
        .expect(
            "la suppression d'un lieu (cascade stations + SET NULL events) doit rester possible",
        );
}

// ─────────────────────────────────────────────────────────────────────────────
// T2 — cohérence alert_rules.org_id = org du lieu
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn t2_alert_rule_with_foreign_org_is_rejected() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let cityair = org_id(&mut tx, "cityair").await;
    let ecole = location_id(&mut tx, "École Jules-Ferry").await; // org = agglo-riviera
    let err = sqlx::query(
        "INSERT INTO alert_rules (org_id, tracked_location_id, parameter_id, comparator, threshold_value)
         VALUES ($1, $2, (SELECT id FROM parameters WHERE code = 'so2'), '>', 1.0)",
    )
    .bind(cityair)
    .bind(ecole)
    .execute(&mut *tx)
    .await
    .expect_err("une règle dont l'org diffère de celle du lieu doit être refusée");
    assert!(
        err.to_string().contains("QRT_T2"),
        "erreur T2 attendue : {err}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// T3 — destinataire interne ∈ org de la règle
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn t3_recipient_outside_org_is_rejected() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    // lea@cityair.app n'est PAS membre d'agglo-riviera → refus.
    let err = sqlx::query(
        "INSERT INTO alert_rule_recipients (alert_rule_id, channel, user_id)
         SELECT ar.id, 'email', (SELECT id FROM users WHERE email = 'lea@cityair.app')
           FROM alert_rules ar WHERE ar.name = 'Seuil enfants PM2.5'",
    )
    .execute(&mut *tx)
    .await
    .expect_err("un destinataire hors org doit être refusé");
    assert!(
        err.to_string().contains("QRT_T3"),
        "erreur T3 attendue : {err}"
    );
}

#[tokio::test]
async fn t3_multi_org_member_and_external_email_are_accepted() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    // thomas@groupeindus.com est AUSSI lecteur d'agglo-riviera (seed multi-org) → accepté.
    sqlx::query(
        "INSERT INTO alert_rule_recipients (alert_rule_id, channel, user_id)
         SELECT ar.id, 'websocket', (SELECT id FROM users WHERE email = 'thomas@groupeindus.com')
           FROM alert_rules ar WHERE ar.name = 'Seuil NO2 réglementaire'",
    )
    .execute(&mut *tx)
    .await
    .expect("un membre multi-org de l'org de la règle doit être accepté");

    // Cible externe (email) : aucune vérification d'appartenance, par conception.
    sqlx::query(
        "INSERT INTO alert_rule_recipients (alert_rule_id, channel, email)
         SELECT ar.id, 'email', 'externe@mairie-exemple.fr'
           FROM alert_rules ar WHERE ar.name = 'Seuil NO2 réglementaire'",
    )
    .execute(&mut *tx)
    .await
    .expect("une cible externe (email) reste libre");
}

// ─────────────────────────────────────────────────────────────────────────────
// T4 — immuabilité du snapshot alert_events
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn t4_snapshot_columns_are_immutable() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let err = sqlx::query(
        "UPDATE alert_events SET measured_value = measured_value + 1
          WHERE id = (SELECT min(id) FROM alert_events)",
    )
    .execute(&mut *tx)
    .await
    .expect_err("les colonnes snapshot d'alert_events doivent être immuables");
    assert!(
        err.to_string().contains("QRT_T4"),
        "erreur T4 attendue : {err}"
    );
}

#[tokio::test]
async fn t4_is_read_stays_mutable() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let res = sqlx::query(
        "UPDATE alert_events SET is_read = NOT is_read
          WHERE id = (SELECT min(id) FROM alert_events)",
    )
    .execute(&mut *tx)
    .await
    .expect("is_read doit rester mutable");
    assert_eq!(res.rows_affected(), 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// T5 — audit automatique d'alert_rules
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn t5_audit_create_then_deactivate_with_actor() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let agglo = org_id(&mut tx, "agglo-riviera").await;
    let centre = location_id(&mut tx, "Centre-ville").await;
    let sophie: i64 = sqlx::query("SELECT id FROM users WHERE email = 'sophie@agglo-riviera.fr'")
        .fetch_one(&mut *tx)
        .await
        .unwrap()
        .get(0);

    // L'acteur applicatif est porté par la GUC de session (scope transaction).
    sqlx::query("SELECT set_config('quarity.actor_user_id', $1::text, true)")
        .bind(sophie)
        .execute(&mut *tx)
        .await
        .unwrap();

    let rule_id: i64 = sqlx::query(
        "INSERT INTO alert_rules (org_id, tracked_location_id, parameter_id, comparator, threshold_value, name)
         VALUES ($1, $2, (SELECT id FROM parameters WHERE code = 'pm10'), '>', 99.9, 'Règle test T5')
         RETURNING id",
    )
    .bind(agglo)
    .bind(centre)
    .fetch_one(&mut *tx)
    .await
    .expect("création de la règle de test")
    .get(0);

    // create journalisé, avec l'acteur de la GUC.
    let row = sqlx::query(
        "SELECT action, actor_user_id FROM audit_log
          WHERE entity_type = 'alert_rule' AND entity_id = $1",
    )
    .bind(rule_id)
    .fetch_one(&mut *tx)
    .await
    .expect("une ligne d'audit 'create' doit exister");
    assert_eq!(row.get::<String, _>("action"), "create");
    assert_eq!(row.get::<Option<i64>, _>("actor_user_id"), Some(sophie));

    // Désactivation → 'deactivate', avec diff {status: {from, to}}.
    sqlx::query("UPDATE alert_rules SET status = 'inactive' WHERE id = $1")
        .bind(rule_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    let deactivate: i64 = sqlx::query(
        "SELECT count(*) FROM audit_log
          WHERE entity_type = 'alert_rule' AND entity_id = $1
            AND action = 'deactivate'
            AND diff->'status'->>'to' = 'inactive'",
    )
    .bind(rule_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap()
    .get(0);
    assert_eq!(
        deactivate, 1,
        "le passage actif→inactif doit produire 'deactivate'"
    );

    // Update sans changement réel : AUCUNE ligne d'audit supplémentaire.
    sqlx::query("UPDATE alert_rules SET name = name WHERE id = $1")
        .bind(rule_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    let total: i64 = sqlx::query(
        "SELECT count(*) FROM audit_log WHERE entity_type = 'alert_rule' AND entity_id = $1",
    )
    .bind(rule_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap()
    .get(0);
    assert_eq!(
        total, 2,
        "un update no-op ne doit pas produire de bruit d'audit"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// T6 — compteur dénormalisé d'alertes non-lues
// ─────────────────────────────────────────────────────────────────────────────

/// Compteur T6 d'une org (helper du test de cycle de vie).
async fn unread_count(tx: &mut sqlx::PgConnection, org: i64) -> i32 {
    sqlx::query("SELECT unread_alert_count FROM organizations WHERE id = $1")
        .bind(org)
        .fetch_one(tx)
        .await
        .unwrap()
        .get(0)
}

#[tokio::test]
async fn t6_unread_counter_follows_event_lifecycle() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let agglo = org_id(&mut tx, "agglo-riviera").await;
    let before = unread_count(&mut tx, agglo).await;

    // INSERT non-lu → +1
    let ev: i64 = sqlx::query(
        "INSERT INTO alert_events (org_id, ref_location_id, openaq_location_id, parameter_code,
                                   measured_value, unit, measured_at, threshold_value, comparator,
                                   severity, is_read)
         VALUES ($1, (SELECT id FROM ref_locations WHERE openaq_location_id = 1001), 1001, 'pm25',
                 42.0, 'µg/m³', now(), 15.0, '>', 'warning', false)
         RETURNING id",
    )
    .bind(agglo)
    .fetch_one(&mut *tx)
    .await
    .expect("insertion d'un event de test")
    .get(0);
    assert_eq!(
        unread_count(&mut tx, agglo).await,
        before + 1,
        "INSERT non-lu → +1"
    );

    // Lecture → -1 ; relecture inverse → +1.
    sqlx::query("UPDATE alert_events SET is_read = true WHERE id = $1")
        .bind(ev)
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        unread_count(&mut tx, agglo).await,
        before,
        "passage à lu → -1"
    );

    sqlx::query("UPDATE alert_events SET is_read = false WHERE id = $1")
        .bind(ev)
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        unread_count(&mut tx, agglo).await,
        before + 1,
        "retour à non-lu → +1"
    );

    // DELETE d'un non-lu → -1.
    sqlx::query("DELETE FROM alert_events WHERE id = $1")
        .bind(ev)
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        unread_count(&mut tx, agglo).await,
        before,
        "DELETE d'un non-lu → -1"
    );
}

#[tokio::test]
async fn t6_recompute_function_repairs_a_drifted_counter() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let agglo = org_id(&mut tx, "agglo-riviera").await;
    let before = unread_count(&mut tx, agglo).await;

    // Dérive simulée (équivalent d'un TRUNCATE/restore qui contourne les triggers).
    sqlx::query("UPDATE organizations SET unread_alert_count = 9999 WHERE id = $1")
        .bind(agglo)
        .execute(&mut *tx)
        .await
        .unwrap();

    // Le recalibrage répare TOUTES les orgs.
    sqlx::query("SELECT recompute_unread_alert_counts()")
        .execute(&mut *tx)
        .await
        .expect("la fonction de recalibrage doit s'exécuter");
    assert_eq!(
        unread_count(&mut tx, agglo).await,
        before,
        "recompute_unread_alert_counts doit réparer un compteur dérivé"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// P1 — create_tracked_location_with_rules (+ LE test de ROLLBACK explicite, B3)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn p1_requires_at_least_one_station() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let cityair = org_id(&mut tx, "cityair").await;
    let err = sqlx::query(
        "CALL create_tracked_location_with_rules($1, 'Zone sans station', NULL, NULL,
                                                 ARRAY[]::BIGINT[], '[]'::jsonb, NULL)",
    )
    .bind(cityair)
    .execute(&mut *tx)
    .await
    .expect_err("un lieu sans station doit être refusé (US-01 c2)");
    assert!(
        err.to_string().contains("QRT_P1"),
        "erreur P1 attendue : {err}"
    );
}

#[tokio::test]
async fn p1_unknown_station_aborts_everything() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let cityair = org_id(&mut tx, "cityair").await;
    let err = sqlx::query(
        "CALL create_tracked_location_with_rules($1, 'Zone station fantôme', NULL, NULL,
                                                 ARRAY[999999]::BIGINT[], '[]'::jsonb, NULL)",
    )
    .bind(cityair)
    .execute(&mut *tx)
    .await
    .expect_err("une station OpenAQ inconnue doit faire échouer la création entière");
    assert!(
        err.to_string().contains("QRT_P1"),
        "erreur P1 attendue : {err}"
    );
}

#[tokio::test]
async fn p1_duplicate_stations_are_rejected_explicitly() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let cityair = org_id(&mut tx, "cityair").await;
    // Doublon → QRT_P1 explicite (et PAS une unique_violation 23505 → 500 côté back).
    let err = sqlx::query(
        "CALL create_tracked_location_with_rules($1, 'Zone doublons', NULL, NULL,
                                                 ARRAY[1003, 1003]::BIGINT[], '[]'::jsonb, NULL)",
    )
    .bind(cityair)
    .execute(&mut *tx)
    .await
    .expect_err("des stations en double doivent être refusées explicitement");
    assert!(
        err.to_string().contains("QRT_P1"),
        "erreur P1 (doublons) attendue : {err}"
    );
}

/// B3 — « 1 transaction explicite avec ROLLBACK testé » : on exécute P1 dans une
/// transaction, on VÉRIFIE les effets à l'intérieur, on ROLLBACK explicitement,
/// puis on prouve qu'il ne reste RIEN (atomicité du point d'entrée US-01).
#[tokio::test]
async fn p1_creates_everything_and_explicit_rollback_leaves_no_trace() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let cityair = org_id(&mut tx, "cityair").await;
    let lea: i64 = sqlx::query("SELECT id FROM users WHERE email = 'lea@cityair.app'")
        .fetch_one(&mut *tx)
        .await
        .unwrap()
        .get(0);

    sqlx::query(
        "CALL create_tracked_location_with_rules($1, 'Zone Test P1', 'créée par test', $2,
                ARRAY[1003, 1004]::BIGINT[],
                '[{\"parameter\":\"pm25\",\"comparator\":\">\",\"threshold\":20.0,\"severity\":\"warning\",\"name\":\"Règle P1\"}]'::jsonb,
                NULL)",
    )
    .bind(cityair)
    .bind(lea)
    .execute(&mut *tx)
    .await
    .expect("P1 doit créer lieu + stations + règle");

    // Effets visibles DANS la transaction : lieu + 2 stations (1re primaire) + 1 règle cohérente (T2).
    let row = sqlx::query(
        "SELECT tl.id,
                (SELECT count(*) FROM tracked_location_stations s WHERE s.tracked_location_id = tl.id)             AS stations,
                (SELECT rl.openaq_location_id FROM tracked_location_stations s
                   JOIN ref_locations rl ON rl.id = s.ref_location_id
                  WHERE s.tracked_location_id = tl.id AND s.is_primary)                                            AS primary_station,
                (SELECT count(*) FROM alert_rules ar WHERE ar.tracked_location_id = tl.id AND ar.org_id = tl.org_id) AS rules
           FROM tracked_locations tl WHERE tl.name = 'Zone Test P1'",
    )
    .fetch_one(&mut *tx)
    .await
    .expect("le lieu créé par P1 doit exister dans la transaction");
    assert_eq!(row.get::<i64, _>("stations"), 2);
    assert_eq!(
        row.get::<i64, _>("primary_station"),
        1003,
        "la 1re station du tableau doit être primaire"
    );
    assert_eq!(row.get::<i64, _>("rules"), 1);

    // ROLLBACK EXPLICITE…
    tx.rollback().await.expect("rollback explicite");

    // …et plus aucune trace hors transaction.
    let remaining: i64 =
        sqlx::query("SELECT count(*) FROM tracked_locations WHERE name = 'Zone Test P1'")
            .fetch_one(&pool)
            .await
            .unwrap()
            .get(0);
    assert_eq!(
        remaining, 0,
        "après ROLLBACK, la création P1 ne doit laisser AUCUNE trace"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// P2 — archive_old_alert_events
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn p2_archives_only_old_read_events() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let agglo = org_id(&mut tx, "agglo-riviera").await;

    // Deux events synthétiques vieux de 400 jours : un LU (archivable), un NON-LU (jamais archivé).
    let insert = "INSERT INTO alert_events (org_id, ref_location_id, openaq_location_id, parameter_code,
                          measured_value, unit, measured_at, threshold_value, comparator, severity,
                          fired_at, is_read)
                  VALUES ($1, (SELECT id FROM ref_locations WHERE openaq_location_id = 1001), 1001, 'pm25',
                          77.0, 'µg/m³', now() - interval '400 days', 15.0, '>', 'warning',
                          now() - interval '400 days', $2)
                  RETURNING id";
    let read_ev: i64 = sqlx::query(insert)
        .bind(agglo)
        .bind(true)
        .fetch_one(&mut *tx)
        .await
        .unwrap()
        .get(0);
    let unread_ev: i64 = sqlx::query(insert)
        .bind(agglo)
        .bind(false)
        .fetch_one(&mut *tx)
        .await
        .unwrap()
        .get(0);

    sqlx::query("CALL archive_old_alert_events(365, NULL)")
        .execute(&mut *tx)
        .await
        .expect("l'archivage doit s'exécuter");

    // Le LU ancien a été déplacé (id préservé), le NON-LU est resté.
    let archived: i64 = sqlx::query("SELECT count(*) FROM alert_events_archive WHERE id = $1")
        .bind(read_ev)
        .fetch_one(&mut *tx)
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        archived, 1,
        "l'event lu et ancien doit être dans l'archive (id d'origine préservé)"
    );

    let gone: i64 = sqlx::query("SELECT count(*) FROM alert_events WHERE id = $1")
        .bind(read_ev)
        .fetch_one(&mut *tx)
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        gone, 0,
        "l'event archivé ne doit plus être dans alert_events"
    );

    let kept: i64 = sqlx::query("SELECT count(*) FROM alert_events WHERE id = $1")
        .bind(unread_ev)
        .fetch_one(&mut *tx)
        .await
        .unwrap()
        .get(0);
    assert_eq!(kept, 1, "un event NON-LU n'est JAMAIS archivé, même ancien");
}

// ─────────────────────────────────────────────────────────────────────────────
// P3 — compute_exposure_dose (moitié transactionnelle : snapshot + upsert)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn p3_upserts_dose_with_window_snapshot() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    // tlp du seed : École Jules-Ferry × enfants (08:00–17:00, mask 31, pm25 1h → 15.0).
    let tlp: i64 = sqlx::query(
        "SELECT tlp.id FROM tracked_location_profiles tlp
           JOIN tracked_locations tl ON tl.id = tlp.tracked_location_id
           JOIN exposure_profiles ep ON ep.id = tlp.exposure_profile_id
          WHERE tl.name = 'École Jules-Ferry' AND ep.code = 'enfants'",
    )
    .fetch_one(&mut *tx)
    .await
    .expect("tlp École×enfants attendu (seed)")
    .get(0);

    sqlx::query(
        "CALL compute_exposure_dose($1, 'pm25', '2026-06-01', '2026-06-07', 4.5, 120, '1h', NULL)",
    )
    .bind(tlp)
    .execute(&mut *tx)
    .await
    .expect("P3 doit calculer et insérer la dose");

    let row = sqlx::query(
        "SELECT threshold_value::float8 AS thr, hours_over_threshold::float8 AS hours,
                window_start_time::text AS ws, window_days_mask, sample_count
           FROM exposure_results
          WHERE tracked_location_profile_id = $1 AND period_start = '2026-06-01'",
    )
    .bind(tlp)
    .fetch_one(&mut *tx)
    .await
    .expect("le résultat de dose doit exister");
    assert_eq!(
        row.get::<f64, _>("thr"),
        15.0,
        "seuil figé depuis exposure_thresholds"
    );
    assert_eq!(row.get::<f64, _>("hours"), 4.5);
    assert_eq!(
        row.get::<String, _>("ws"),
        "08:00:00",
        "fenêtre snapshotée depuis le tlp"
    );
    assert_eq!(row.get::<i16, _>("window_days_mask"), 31);

    // Re-calcul sur la même période → UPSERT (pas de doublon), valeurs rafraîchies.
    sqlx::query(
        "CALL compute_exposure_dose($1, 'pm25', '2026-06-01', '2026-06-07', 6.0, 150, '1h', NULL)",
    )
    .bind(tlp)
    .execute(&mut *tx)
    .await
    .unwrap();
    let row2 = sqlx::query(
        "SELECT count(*) OVER () AS n, hours_over_threshold::float8 AS hours
           FROM exposure_results
          WHERE tracked_location_profile_id = $1 AND period_start = '2026-06-01'",
    )
    .bind(tlp)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(
        row2.get::<i64, _>("n"),
        1,
        "upsert : une seule ligne par (tlp, polluant, période)"
    );
    assert_eq!(row2.get::<f64, _>("hours"), 6.0);

    // Polluant sans seuil pour ce profil → erreur P3 explicite.
    let err = sqlx::query(
        "CALL compute_exposure_dose($1, 'co', '2026-06-01', '2026-06-07', 1.0, 10, '1h', NULL)",
    )
    .bind(tlp)
    .execute(&mut *tx)
    .await
    .expect_err("polluant sans seuil → refus");
    assert!(
        err.to_string().contains("QRT_P3"),
        "erreur P3 attendue : {err}"
    );
}

#[tokio::test]
async fn p3_refuses_inactive_profile_association() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    // Défense en profondeur : tlp désactivé → pas de calcul de dose.
    let tlp: i64 = sqlx::query(
        "SELECT tlp.id FROM tracked_location_profiles tlp
           JOIN tracked_locations tl ON tl.id = tlp.tracked_location_id
          WHERE tl.name = 'École Jules-Ferry'",
    )
    .fetch_one(&mut *tx)
    .await
    .expect("tlp École attendu (seed)")
    .get(0);
    sqlx::query("UPDATE tracked_location_profiles SET is_active = false WHERE id = $1")
        .bind(tlp)
        .execute(&mut *tx)
        .await
        .unwrap();

    let err = sqlx::query(
        "CALL compute_exposure_dose($1, 'pm25', '2026-06-01', '2026-06-07', 1.0, 10, '1h', NULL)",
    )
    .bind(tlp)
    .execute(&mut *tx)
    .await
    .expect_err("un tlp désactivé doit être refusé");
    assert!(
        err.to_string().contains("QRT_P3"),
        "erreur P3 (tlp inactif) attendue : {err}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// B1 — vues métier (cohérence avec les données sous-jacentes)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn views_are_consistent_with_underlying_data() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();

    // org_active_zones_view : Centre-ville agrège 2 stations, primaire = Nice Centre (seed).
    let row = sqlx::query(
        "SELECT station_count, primary_station_name, active_rule_count
           FROM org_active_zones_view WHERE location_name = 'Centre-ville'",
    )
    .fetch_one(&mut *tx)
    .await
    .expect("Centre-ville doit apparaître dans org_active_zones_view");
    assert_eq!(row.get::<i64, _>("station_count"), 2);
    assert_eq!(row.get::<String, _>("primary_station_name"), "Nice Centre");
    assert!(row.get::<i64, _>("active_rule_count") >= 1);

    // org_alert_stats_view ⟷ compteur T6 : auto-cohérence pour TOUTES les orgs.
    let mismatches: i64 = sqlx::query(
        "SELECT count(*) FROM org_alert_stats_view s
           JOIN organizations o ON o.id = s.org_id
          WHERE s.unread_events <> o.unread_alert_count",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap()
    .get(0);
    assert_eq!(
        mismatches, 0,
        "unread_events (vue) doit égaler unread_alert_count (compteur T6) pour chaque org"
    );

    // Et la vue elle-même doit égaler un recomptage direct.
    let direct_mismatch: i64 = sqlx::query(
        "SELECT count(*) FROM org_alert_stats_view s
          WHERE s.total_events <> (SELECT count(*) FROM alert_events ae WHERE ae.org_id = s.org_id)",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap()
    .get(0);
    assert_eq!(
        direct_mismatch, 0,
        "total_events (vue) doit égaler le comptage direct"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// B4 — requête métier complexe (le fichier versionné doit s'exécuter tel quel)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn complex_business_query_runs_and_orders_correctly() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let sql = include_str!("../../db/sql/queries/complex_business_query.sql");

    let rows = sqlx::query(sql)
        .fetch_all(&mut *tx)
        .await
        .expect("la requête complexe versionnée doit s'exécuter telle quelle");

    assert!(rows.len() <= 10, "LIMIT 10 respecté");
    // HAVING : aucune org à 0 critique ; ORDER BY : volumes décroissants.
    let mut last = i64::MAX;
    for r in &rows {
        let n: i64 = r.get("critical_events");
        assert!(n >= 1, "HAVING doit exclure les orgs sans alerte critique");
        assert!(n <= last, "tri par critical_events décroissant attendu");
        last = n;
    }
    // Le seed garantit au moins groupeindus (2 critiques récents : Site Lyon).
    let has_groupeindus = rows
        .iter()
        .any(|r| r.get::<String, _>("org_slug") == "groupeindus");
    assert!(
        has_groupeindus,
        "groupeindus (2 alertes critiques au seed) doit ressortir"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// T7 — cohérence alert_events.org_id avec ses référents (lieu suivi / règle)
// (durcissement post-analyse 2026-06-08 — migration 0005)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn t7_alert_event_with_foreign_tracked_location_is_rejected() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let cityair = org_id(&mut tx, "cityair").await;
    let ecole = location_id(&mut tx, "École Jules-Ferry").await; // org = agglo-riviera
    let err = sqlx::query(
        "INSERT INTO alert_events (org_id, tracked_location_id, ref_location_id, openaq_location_id,
                                   parameter_code, measured_value, unit, measured_at,
                                   threshold_value, comparator, severity)
         VALUES ($1, $2, 1, 1001, 'pm25', 30.0, 'µg/m³', now(), 20.0, '>', 'warning')",
    )
    .bind(cityair)
    .bind(ecole)
    .execute(&mut *tx)
    .await
    .expect_err("un alert_event dont l'org diffère de celle de son lieu suivi doit être refusé");
    assert!(
        err.to_string().contains("QRT_T7"),
        "erreur T7 attendue : {err}"
    );
}

#[tokio::test]
async fn t7_alert_event_with_foreign_alert_rule_is_rejected() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let cityair = org_id(&mut tx, "cityair").await;
    // 'Seuil enfants PM2.5' appartient à agglo-riviera ; l'event prétend être cityair.
    let err = sqlx::query(
        "INSERT INTO alert_events (org_id, alert_rule_id, ref_location_id, openaq_location_id,
                                   parameter_code, measured_value, unit, measured_at,
                                   threshold_value, comparator, severity)
         VALUES ($1, (SELECT id FROM alert_rules WHERE name = 'Seuil enfants PM2.5'),
                 1, 1001, 'pm25', 30.0, 'µg/m³', now(), 20.0, '>', 'warning')",
    )
    .bind(cityair)
    .execute(&mut *tx)
    .await
    .expect_err("un alert_event dont l'org diffère de celle de sa règle doit être refusé");
    assert!(
        err.to_string().contains("QRT_T7"),
        "erreur T7 attendue : {err}"
    );
}

#[tokio::test]
async fn t7_coherent_alert_event_is_allowed() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let agglo = org_id(&mut tx, "agglo-riviera").await;
    let ecole = location_id(&mut tx, "École Jules-Ferry").await; // org = agglo-riviera
    let res = sqlx::query(
        "INSERT INTO alert_events (org_id, tracked_location_id, ref_location_id, openaq_location_id,
                                   parameter_code, measured_value, unit, measured_at,
                                   threshold_value, comparator, severity)
         VALUES ($1, $2, 1, 1001, 'pm25', 30.0, 'µg/m³', now(), 20.0, '>', 'warning')",
    )
    .bind(agglo)
    .bind(ecole)
    .execute(&mut *tx)
    .await
    .expect("un event dont l'org concorde avec son lieu doit être accepté");
    assert_eq!(res.rows_affected(), 1);
}

#[tokio::test]
async fn t7_pure_snapshot_event_without_refs_is_allowed() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let cityair = org_id(&mut tx, "cityair").await;
    // Référents NULL (event survivant à un SET NULL en cascade) : seul org_id porte le
    // tenant — T7 ne doit RIEN imposer (la FK org_id garantit l'existence de l'org).
    let res = sqlx::query(
        "INSERT INTO alert_events (org_id, alert_rule_id, tracked_location_id,
                                   ref_location_id, openaq_location_id,
                                   parameter_code, measured_value, unit, measured_at,
                                   threshold_value, comparator, severity)
         VALUES ($1, NULL, NULL, 1, 1001, 'pm25', 30.0, 'µg/m³', now(), 20.0, '>', 'warning')",
    )
    .bind(cityair)
    .execute(&mut *tx)
    .await
    .expect("un event snapshot pur (référents NULL) doit être accepté");
    assert_eq!(res.rows_affected(), 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// T8 — tracked_location_profiles : isolation lieu × profil d'exposition (0008)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn t8_foreign_custom_profile_association_is_rejected() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let cityair = org_id(&mut tx, "cityair").await;
    let ecole = location_id(&mut tx, "École Jules-Ferry").await; // org = agglo-riviera
                                                                 // Profil CUSTOM appartenant à cityair (≠ org du lieu).
    let foreign_profile: i64 = sqlx::query_scalar(
        "INSERT INTO exposure_profiles (org_id, code, name, is_system)
         VALUES ($1, 'general', 'Custom cityair', false) RETURNING id",
    )
    .bind(cityair)
    .fetch_one(&mut *tx)
    .await
    .expect("création d'un profil custom cityair");
    // Associer un profil d'org B à un lieu d'org A → refus T8.
    let err = sqlx::query(
        "INSERT INTO tracked_location_profiles
            (tracked_location_id, exposure_profile_id, start_time, end_time, days_mask)
         VALUES ($1, $2, '08:00', '17:00', 31)",
    )
    .bind(ecole)
    .bind(foreign_profile)
    .execute(&mut *tx)
    .await
    .expect_err("associer un profil custom d'une AUTRE org doit être refusé");
    assert!(
        err.to_string().contains("QRT_T8"),
        "erreur T8 attendue : {err}"
    );
}

#[tokio::test]
async fn t8_system_profile_association_is_allowed() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let ecole = location_id(&mut tx, "École Jules-Ferry").await; // org = agglo-riviera
                                                                 // Profil SYSTÈME (org_id NULL) — 'general' (≠ 'enfants' déjà lié à l'École par le
                                                                 // seed, ce qui heurterait uq_tlp) : utilisable par toute org.
    let sys_profile: i64 = sqlx::query_scalar(
        "SELECT id FROM exposure_profiles WHERE code = 'general' AND org_id IS NULL AND is_system",
    )
    .fetch_one(&mut *tx)
    .await
    .expect("profil système 'general' (seed)");
    let res = sqlx::query(
        "INSERT INTO tracked_location_profiles
            (tracked_location_id, exposure_profile_id, start_time, end_time, days_mask)
         VALUES ($1, $2, '08:00', '17:00', 31)",
    )
    .bind(ecole)
    .bind(sys_profile)
    .execute(&mut *tx)
    .await
    .expect("un profil système doit être associable à n'importe quel lieu");
    assert_eq!(res.rows_affected(), 1);
}

#[tokio::test]
async fn t8_same_org_custom_profile_association_is_allowed() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();
    let agglo = org_id(&mut tx, "agglo-riviera").await;
    let ecole = location_id(&mut tx, "École Jules-Ferry").await; // org = agglo-riviera
                                                                 // Profil CUSTOM de la MÊME org que le lieu → autorisé.
    let own_profile: i64 = sqlx::query_scalar(
        "INSERT INTO exposure_profiles (org_id, code, name, is_system)
         VALUES ($1, 'sportifs', 'Custom agglo', false) RETURNING id",
    )
    .bind(agglo)
    .fetch_one(&mut *tx)
    .await
    .expect("création d'un profil custom agglo");
    let res = sqlx::query(
        "INSERT INTO tracked_location_profiles
            (tracked_location_id, exposure_profile_id, start_time, end_time, days_mask)
         VALUES ($1, $2, '08:00', '17:00', 31)",
    )
    .bind(ecole)
    .bind(own_profile)
    .execute(&mut *tx)
    .await
    .expect("un profil custom de la MÊME org doit être accepté");
    assert_eq!(res.rows_affected(), 1);
}
