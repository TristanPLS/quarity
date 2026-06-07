//! Tests d'intégration ClickHouse du Jalon 3 (backlog B5) : moyennes glissantes
//! réglementaires (Q1 `rolling_regulatory`) + AQI instantané US EPA (Q2 `aqi_snapshot`).
//! Extension B5 (roadmap.md l.146) : O3 max journalier de la moyenne 8 h
//! (Q3 `o3_daily_max_8h`) + NO2 moyenne annuelle (Q4 `no2_annual_mean`, rollup).
//!
//! ⚠️ Nécessitent un ClickHouse RÉEL avec le schéma `db/clickhouse/01_schema.sql`
//! appliqué. Variables d'environnement attendues (mêmes noms que la CI et `e2e.rs`) :
//! CLICKHOUSE_URL, CLICKHOUSE_USER, CLICKHOUSE_PASSWORD, CLICKHOUSE_DB.
//!
//! Le SQL exécuté est LE fichier versionné
//! `db/clickhouse/queries/rolling_regulatory.sql` (include_str! + découpe sur les
//! séparateurs de section `-- ===== Qn : ... =====`) : aucun SQL métier dupliqué ici,
//! le fichier est prouvé exécutable TEL QUEL (même précédent que `db.rs` pour B4).
//!
//! Autonomie : AUCUNE dépendance au seed ClickHouse (TTL 90 j sur les bruts → le
//! seed d'avril–juin 2026 sera purgé ; et les bases locales contiennent des données
//! réelles imprévisibles, ex. Nice #4085). Chaque test INSÈRE ses propres mesures
//! sur un location_id réservé improbable et UNIQUE PAR EXÉCUTION (base 999×10⁹ +
//! horodatage de lancement × 100 + n° de test, n < 100 : deux exécutions
//! distinctes — même lancées à 1 s d'écart — ne peuvent pas se contaminer ; des
//! buckets résiduels d'une exécution précédente fausseraient `hours_present`),
//! à des heures récentes relatives à now() arrondies
//! à l'heure. Aucun nettoyage : le TTL purgera ces lignes comme les autres.

use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};
use serde_json::Value;

// ─────────────────────────────────────────────────────────────────────────────
// Le fichier SQL versionné, découpé sur ses séparateurs de section (stables)
// ─────────────────────────────────────────────────────────────────────────────

const SQL_FILE: &str = include_str!("../../db/clickhouse/queries/rolling_regulatory.sql");
const SEP_Q1: &str = "-- ===== Q1 : rolling_regulatory =====";
const SEP_Q2: &str = "-- ===== Q2 : aqi_snapshot =====";
const SEP_Q3: &str = "-- ===== Q3 : o3_daily_max_8h =====";
const SEP_Q4: &str = "-- ===== Q4 : no2_annual_mean =====";

/// Section Q1 du fichier versionné (du séparateur Q1 exclu du Q2).
fn q1_sql() -> &'static str {
    let start = SQL_FILE
        .find(SEP_Q1)
        .expect("séparateur Q1 présent dans le fichier");
    let end = SQL_FILE
        .find(SEP_Q2)
        .expect("séparateur Q2 présent dans le fichier");
    &SQL_FILE[start..end]
}

/// Section Q2 du fichier versionné (du séparateur Q2 exclu du Q3 — Q2 n'est
/// plus la dernière section depuis l'extension B5 Q3/Q4).
fn q2_sql() -> &'static str {
    let start = SQL_FILE
        .find(SEP_Q2)
        .expect("séparateur Q2 présent dans le fichier");
    let end = SQL_FILE
        .find(SEP_Q3)
        .expect("séparateur Q3 présent dans le fichier");
    &SQL_FILE[start..end]
}

/// Section Q3 du fichier versionné (du séparateur Q3 exclu du Q4).
fn q3_sql() -> &'static str {
    let start = SQL_FILE
        .find(SEP_Q3)
        .expect("séparateur Q3 présent dans le fichier");
    let end = SQL_FILE
        .find(SEP_Q4)
        .expect("séparateur Q4 présent dans le fichier");
    &SQL_FILE[start..end]
}

/// Section Q4 du fichier versionné (du séparateur Q4 à la fin).
fn q4_sql() -> &'static str {
    let start = SQL_FILE
        .find(SEP_Q4)
        .expect("séparateur Q4 présent dans le fichier");
    &SQL_FILE[start..]
}

// ─────────────────────────────────────────────────────────────────────────────
// Client ClickHouse HTTP minimal (même style d'exécution que back/src/ch.rs)
// ─────────────────────────────────────────────────────────────────────────────

struct Ch {
    http: reqwest::Client,
    url: String,
    user: String,
    password: String,
    db: String,
}

fn ch() -> Ch {
    Ch {
        http: reqwest::Client::new(),
        url: std::env::var("CLICKHOUSE_URL").expect("CLICKHOUSE_URL"),
        user: std::env::var("CLICKHOUSE_USER").expect("CLICKHOUSE_USER"),
        password: std::env::var("CLICKHOUSE_PASSWORD").expect("CLICKHOUSE_PASSWORD"),
        db: std::env::var("CLICKHOUSE_DB").expect("CLICKHOUSE_DB"),
    }
}

/// Une mesure brute à insérer (sensor/pays/unité/coordonnées : constantes sans
/// incidence sur Q1–Q4, qui n'agrègent que value par (location, parameter, heure)
/// — ou par (location, parameter, jour) pour le rollup que lit Q4).
struct Sample {
    location_id: u64,
    parameter: &'static str,
    measured_at: DateTime<Utc>,
    value: f64,
    ingested_at: DateTime<Utc>,
}

fn sample(
    location_id: u64,
    parameter: &'static str,
    measured_at: DateTime<Utc>,
    value: f64,
    ingested_at: DateTime<Utc>,
) -> Sample {
    Sample {
        location_id,
        parameter,
        measured_at,
        value,
        ingested_at,
    }
}

impl Ch {
    /// POST du SQL en body + valeurs en query-string `param_*` (anti-injection),
    /// comme `back/src/ch.rs`. Panique avec le corps d'erreur ClickHouse si échec.
    async fn exec(&self, sql: &str, params: &[(&str, String)]) -> String {
        let mut req = self
            .http
            .post(&self.url)
            .basic_auth(&self.user, Some(&self.password))
            .query(&[
                ("database", self.db.as_str()),
                // UInt64 en nombres JSON (et non en chaînes), comme back/src/ch.rs :
                ("output_format_json_quote_64bit_integers", "0"),
            ]);
        for (k, v) in params {
            req = req.query(&[(*k, v.as_str())]);
        }
        let resp = req
            .body(sql.to_string())
            .send()
            .await
            .expect("requête HTTP ClickHouse (base de test démarrée ?)");
        let status = resp.status();
        let body = resp.text().await.expect("corps de réponse ClickHouse");
        assert!(
            status.is_success(),
            "ClickHouse a refusé la requête ({status}) : {body}"
        );
        body
    }

    /// Exécute une section du fichier versionné et parse sa sortie JSONEachRow.
    async fn rows(&self, sql: &str, params: &[(&str, String)]) -> Vec<Value> {
        self.exec(sql, params)
            .await
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("ligne JSONEachRow valide"))
            .collect()
    }

    /// Insère des mesures brutes de test (ingested_at EXPLICITE pour contrôler
    /// quelle version la déduplication ReplacingMergeTree/argMax doit retenir).
    async fn insert(&self, samples: &[Sample]) {
        let mut sql = String::from(
            "INSERT INTO quarity.measurements \
             (location_id, sensor_id, parameter, country, unit, measured_at, \
              value, latitude, longitude, ingested_at) VALUES ",
        );
        for (i, s) in samples.iter().enumerate() {
            if i > 0 {
                sql.push_str(", ");
            }
            sql.push_str(&format!(
                "({}, 1, '{}', 'FR', 'µg/m³', '{}', {}, 43.7, 7.27, '{}')",
                s.location_id,
                s.parameter,
                fmt(s.measured_at),
                s.value,
                fmt(s.ingested_at),
            ));
        }
        self.exec(&sql, &[]).await;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers temps / identifiants / extraction JSON
// ─────────────────────────────────────────────────────────────────────────────

/// Début de l'heure courante (UTC) : ancre temporelle RÉCENTE de chaque test
/// (le TTL 90 j interdit les dates en dur — elles finiraient purgées).
fn current_hour() -> DateTime<Utc> {
    let secs = Utc::now().timestamp();
    Utc.timestamp_opt(secs - secs.rem_euclid(3600), 0).unwrap()
}

/// location_id réservé improbable, unique par exécution ET par test (cf. en-tête).
/// L'unicité inter-exécutions exige n < 100 : avec ×10, 10T + 13 = 10(T+1) + 3,
/// deux exécutions lancées à 1 s d'écart pouvaient partager un id (revue Q3/Q4).
fn test_location(n: u64) -> u64 {
    999_000_000_000 + (Utc::now().timestamp() as u64) * 100 + n
}

/// Début (00:00 UTC) du jour J − `days_ago` : Q3/Q4 raisonnent sur des jours
/// UTC COMPLETS déjà passés (jamais aujourd'hui — la frontière de jour serait
/// mouvante pendant le test), récents (TTL 90 j, cf. `current_hour`).
fn day_start(days_ago: i64) -> DateTime<Utc> {
    let secs = Utc::now().timestamp();
    Utc.timestamp_opt(secs - secs.rem_euclid(86_400), 0)
        .unwrap()
        - Duration::days(days_ago)
}

/// Format DateTime64(3) des littéraux INSERT et des `param_*` (parseDateTime64BestEffort).
fn fmt(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

/// Format de sortie JSONEachRow d'un DateTime (`bucket_hour`, `window_end`).
fn fmt_hour(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn f64_of(row: &Value, key: &str) -> f64 {
    row[key]
        .as_f64()
        .unwrap_or_else(|| panic!("champ flottant `{key}` attendu dans {row}"))
}

fn u64_of(row: &Value, key: &str) -> u64 {
    row[key]
        .as_u64()
        .unwrap_or_else(|| panic!("champ entier `{key}` attendu dans {row}"))
}

fn i64_of(row: &Value, key: &str) -> i64 {
    row[key]
        .as_i64()
        .unwrap_or_else(|| panic!("champ entier `{key}` attendu dans {row}"))
}

fn str_of<'a>(row: &'a Value, key: &str) -> &'a str {
    row[key]
        .as_str()
        .unwrap_or_else(|| panic!("champ texte `{key}` attendu dans {row}"))
}

/// Égalité flottante (les moyennes ClickHouse et Rust peuvent différer d'un ULP
/// selon l'ordre de sommation).
fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

// ─────────────────────────────────────────────────────────────────────────────
// Q1 — moyennes glissantes réglementaires
// ─────────────────────────────────────────────────────────────────────────────

/// La moyenne glissante 24 h part de la VÉRITÉ DÉDUPLIQUÉE (argMax) : une clé
/// ré-ingérée (22.5 puis 22.9, ingested_at plus récent) ne compte qu'une fois,
/// avec sa dernière valeur.
#[tokio::test]
async fn q1_pm25_24h_rolling_uses_deduplicated_truth() {
    let ch = ch();
    let loc = test_location(1);
    let h4 = current_hour();
    let (h1, h2, h3) = (
        h4 - Duration::hours(3),
        h4 - Duration::hours(2),
        h4 - Duration::hours(1),
    );

    ch.insert(&[
        sample(loc, "pm25", h1, 12.0, h4),
        sample(loc, "pm25", h2, 18.4, h4),
        sample(loc, "pm25", h3, 22.5, h4),
        // Ré-ingestion de la MÊME clé (loc, pm25, h3) avec ingested_at + 1 s :
        // la vérité dédupliquée doit retenir 22.9 et oublier 22.5.
        sample(loc, "pm25", h3, 22.9, h4 + Duration::seconds(1)),
        sample(loc, "pm25", h4, 14.1, h4),
    ])
    .await;

    let rows = ch
        .rows(
            q1_sql(),
            &[
                ("param_loc", loc.to_string()),
                ("param_from", fmt(h1)),
                ("param_to", fmt(h4 + Duration::hours(1))),
            ],
        )
        .await;

    assert_eq!(rows.len(), 4, "4 buckets horaires pm25 attendus : {rows:?}");
    assert!(
        rows.iter().all(|r| str_of(r, "parameter") == "pm25"),
        "uniquement du pm25 sur ce lieu de test : {rows:?}"
    );

    let last = rows
        .iter()
        .find(|r| str_of(r, "bucket_hour") == fmt_hour(h4))
        .unwrap_or_else(|| panic!("bucket {} attendu : {rows:?}", fmt_hour(h4)));
    // Attendu (calcul manuel) : dédup ⇒ (12.0 + 18.4 + 22.9 + 14.1) / 4 = 67.4 / 4 = 16.85.
    //   Si 22.5 ET 22.9 comptaient : (12.0+18.4+22.7+14.1)/4 = 16.8 ;
    //   si argMax retenait 22.5    : (12.0+18.4+22.5+14.1)/4 = 16.75.
    //   16.85 prouve donc que SEUL 22.9 (dernier ingéré) a été retenu.
    let avg = f64_of(last, "rolling_avg");
    assert!(approx_eq(avg, 16.85), "rolling_avg attendu 16.85 : {avg}");
    assert_eq!(u64_of(last, "window_hours"), 24);
    assert_eq!(u64_of(last, "hours_present"), 4);
    let coverage = f64_of(last, "coverage");
    assert!(
        approx_eq(coverage, 4.0 / 24.0),
        "coverage attendue 4/24 : {coverage}"
    );
    assert_eq!(
        u64_of(last, "is_valid"),
        0,
        "4/24 < 0.75 ⇒ fenêtre invalide"
    );
}

/// Amorçage du frame : une mesure 1 h AVANT {from} doit nourrir la fenêtre du
/// premier bucket affiché… sans apparaître elle-même en sortie.
#[tokio::test]
async fn q1_priming_bucket_feeds_first_frame_but_is_not_output() {
    let ch = ch();
    let loc = test_location(2);
    let h = current_hour();
    let h0 = h - Duration::hours(1); // AVANT {from}

    ch.insert(&[
        sample(loc, "pm25", h0, 10.0, h),
        sample(loc, "pm25", h, 20.0, h),
    ])
    .await;

    let rows = ch
        .rows(
            q1_sql(),
            &[
                ("param_loc", loc.to_string()),
                ("param_from", fmt(h)),
                ("param_to", fmt(h + Duration::hours(1))),
            ],
        )
        .await;

    // Une SEULE ligne de sortie (le bucket h0 < {from} est exclu de l'affichage)…
    assert_eq!(
        rows.len(),
        1,
        "le bucket d'amorçage ne doit pas sortir : {rows:?}"
    );
    let row = &rows[0];
    assert_eq!(str_of(row, "bucket_hour"), fmt_hour(h));
    // …mais h0 a nourri le frame 24 h du premier bucket affiché :
    // rolling_avg = (10.0 + 20.0) / 2 = 15.0, hours_present = 2.
    let avg = f64_of(row, "rolling_avg");
    assert!(approx_eq(avg, 15.0), "rolling_avg attendu 15.0 : {avg}");
    assert_eq!(u64_of(row, "hours_present"), 2);
}

/// Frontière EXACTE des frames RANGE : offsets 86399 s (24 h − 1 s) et 28799 s
/// (8 h − 1 s). Un bucket à exactement h − 24 h (resp. h − 8 h) est HORS fenêtre,
/// un bucket à h − 23 h (resp. h − 7 h) est DEDANS. Une régression off-by-one
/// (86399 → 86400 ou 28799 → 28800) ferait échouer ce test.
#[tokio::test]
async fn q1_range_frame_excludes_bucket_at_exact_window_boundary() {
    let ch = ch();
    let loc = test_location(8);
    let h = current_hour();

    ch.insert(&[
        // pm25 (fenêtre 24 h) : h − 24 h doit être EXCLU du frame du bucket h,
        // h − 23 h doit être INCLUS. La valeur aberrante 100.0 trahirait l'inclusion.
        sample(loc, "pm25", h - Duration::hours(24), 100.0, h),
        sample(loc, "pm25", h - Duration::hours(23), 10.0, h),
        sample(loc, "pm25", h, 20.0, h),
        // o3 (fenêtre 8 h) : h − 8 h EXCLU, h − 7 h INCLUS.
        sample(loc, "o3", h - Duration::hours(8), 999.0, h),
        sample(loc, "o3", h - Duration::hours(7), 30.0, h),
        sample(loc, "o3", h, 50.0, h),
    ])
    .await;

    let rows = ch
        .rows(
            q1_sql(),
            &[
                ("param_loc", loc.to_string()),
                ("param_from", fmt(h)),
                ("param_to", fmt(h + Duration::hours(1))),
            ],
        )
        .await;

    // Seul le bucket h sort ([from, to[) — une ligne par polluant.
    assert_eq!(rows.len(), 2, "un bucket h par polluant attendu : {rows:?}");
    let row_of = |p: &str| {
        rows.iter()
            .find(|r| str_of(r, "parameter") == p)
            .unwrap_or_else(|| panic!("ligne {p} attendue : {rows:?}"))
    };

    // pm25 attendu : (10.0 + 20.0) / 2 = 15.0 sur 2 heures présentes.
    //   Off-by-one (86400) ⇒ h − 24 h inclus : (100+10+20)/3 = 43.33…, hours 3.
    let pm25 = row_of("pm25");
    let avg = f64_of(pm25, "rolling_avg");
    assert!(
        approx_eq(avg, 15.0),
        "pm25 rolling_avg attendu 15.0 : {avg}"
    );
    assert_eq!(u64_of(pm25, "hours_present"), 2);

    // o3 attendu : (30.0 + 50.0) / 2 = 40.0 sur 2 heures présentes.
    //   Off-by-one (28800) ⇒ h − 8 h inclus : (999+30+50)/3 = 359.67…, hours 3.
    let o3 = row_of("o3");
    let avg = f64_of(o3, "rolling_avg");
    assert!(approx_eq(avg, 40.0), "o3 rolling_avg attendu 40.0 : {avg}");
    assert_eq!(u64_of(o3, "hours_present"), 2);
}

// ─────────────────────────────────────────────────────────────────────────────
// Q2 — AQI instantané (conversion ppb, troncature, interpolation, clamp, dominant)
// ─────────────────────────────────────────────────────────────────────────────

/// o3 : conversion µg/m³ → ppb à 25 °C / 1 atm puis interpolation EPA.
#[tokio::test]
async fn q2_o3_converts_to_ppb_then_interpolates() {
    let ch = ch();
    let loc = test_location(3);
    let h = current_hour();

    ch.insert(&[sample(loc, "o3", h, 95.0, h)]).await;

    // {at} = h + 30 min : prouve aussi que la fin de fenêtre est arrondie à l'heure pleine.
    let rows = ch
        .rows(
            q2_sql(),
            &[
                ("param_loc", loc.to_string()),
                ("param_at", fmt(h + Duration::minutes(30))),
            ],
        )
        .await;

    assert_eq!(rows.len(), 1, "une seule ligne o3 attendue : {rows:?}");
    let r = &rows[0];
    assert_eq!(str_of(r, "parameter"), "o3");
    assert_eq!(str_of(r, "window_end"), fmt_hour(h));
    // Attendu (calcul manuel) : 95.0 µg/m³ × 24.45 / 48.00 = 48.390625 ppb
    //   → troncature entière : 48 → palier (0–54 → AQI 0–50)
    //   → AQI = 50/54 × 48 + 0.5 = 44.944… → floor = 44.
    assert_eq!(str_of(r, "conc_unit"), "ppb");
    let conc = f64_of(r, "conc_value");
    assert!(
        approx_eq(conc, 95.0 * 24.45 / 48.0),
        "conc_value attendue 48.390625 ppb : {conc}"
    );
    assert_eq!(i64_of(r, "aqi"), 44);
    assert_eq!(u64_of(r, "window_hours"), 8);
    assert_eq!(u64_of(r, "hours_present"), 1);
    let coverage = f64_of(r, "coverage");
    assert!(
        approx_eq(coverage, 1.0 / 8.0),
        "coverage attendue 1/8 : {coverage}"
    );
    assert_eq!(u64_of(r, "is_valid"), 0, "1/8 < 0.75 ⇒ invalide");
}

/// no2 : fenêtre 1 h — une seule mesure suffit à une couverture pleine.
#[tokio::test]
async fn q2_no2_single_hour_window_is_fully_covered() {
    let ch = ch();
    let loc = test_location(4);
    let h = current_hour();

    ch.insert(&[sample(loc, "no2", h, 52.0, h)]).await;

    let rows = ch
        .rows(
            q2_sql(),
            &[("param_loc", loc.to_string()), ("param_at", fmt(h))],
        )
        .await;

    assert_eq!(rows.len(), 1, "une seule ligne no2 attendue : {rows:?}");
    let r = &rows[0];
    assert_eq!(str_of(r, "parameter"), "no2");
    // Attendu (calcul manuel) : 52.0 µg/m³ × 24.45 / 46.01 = 27.633… ppb
    //   → troncature entière : 27 → palier (0–53 → AQI 0–50)
    //   → AQI = 50/53 × 27 + 0.5 = 25.971… → floor = 25.
    assert_eq!(str_of(r, "conc_unit"), "ppb");
    let conc = f64_of(r, "conc_value");
    assert!(
        approx_eq(conc, 52.0 * 24.45 / 46.01),
        "conc_value attendue ≈ 27.633 ppb : {conc}"
    );
    assert_eq!(i64_of(r, "aqi"), 25);
    assert_eq!(u64_of(r, "window_hours"), 1);
    assert_eq!(u64_of(r, "hours_present"), 1);
    let coverage = f64_of(r, "coverage");
    assert!(
        approx_eq(coverage, 1.0),
        "coverage attendue 1.0 : {coverage}"
    );
    assert_eq!(u64_of(r, "is_valid"), 1, "1/1 ≥ 0.75 ⇒ valide");
}

/// pm25 : pas de conversion (paliers en µg/m³), troncature à 0.1 près,
/// moyenne issue de la vérité dédupliquée (mêmes 4 heures que le test Q1).
#[tokio::test]
async fn q2_pm25_truncates_to_tenth_then_interpolates() {
    let ch = ch();
    let loc = test_location(5);
    let h4 = current_hour();
    let (h1, h2, h3) = (
        h4 - Duration::hours(3),
        h4 - Duration::hours(2),
        h4 - Duration::hours(1),
    );

    ch.insert(&[
        sample(loc, "pm25", h1, 12.0, h4),
        sample(loc, "pm25", h2, 18.4, h4),
        sample(loc, "pm25", h3, 22.5, h4),
        sample(loc, "pm25", h3, 22.9, h4 + Duration::seconds(1)), // ré-ingestion (dédup)
        sample(loc, "pm25", h4, 14.1, h4),
    ])
    .await;

    let rows = ch
        .rows(
            q2_sql(),
            &[("param_loc", loc.to_string()), ("param_at", fmt(h4))],
        )
        .await;

    assert_eq!(rows.len(), 1, "une seule ligne pm25 attendue : {rows:?}");
    let r = &rows[0];
    assert_eq!(str_of(r, "parameter"), "pm25");
    // Attendu (calcul manuel) : moyenne dédupliquée (12.0+18.4+22.9+14.1)/4 = 16.85 µg/m³
    //   → troncature à 0.1 : floor(16.85, 1) = 16.8 → palier (9.1–35.4 → AQI 51–100)
    //   → AQI = (100−51)/(35.4−9.1) × (16.8−9.1) + 51 + 0.5
    //         = 49/26.3 × 7.7 + 51.5 = 14.346… + 51.5 = 65.846… → floor = 65.
    assert_eq!(str_of(r, "conc_unit"), "µg/m³");
    let conc = f64_of(r, "conc_value");
    assert!(approx_eq(conc, 16.85), "conc_value attendue 16.85 : {conc}");
    assert_eq!(i64_of(r, "aqi"), 65);
    assert_eq!(u64_of(r, "window_hours"), 24);
    assert_eq!(u64_of(r, "hours_present"), 4);
    let coverage = f64_of(r, "coverage");
    assert!(
        approx_eq(coverage, 4.0 / 24.0),
        "coverage attendue 4/24 : {coverage}"
    );
    assert_eq!(u64_of(r, "is_valid"), 0, "4/24 < 0.75 ⇒ invalide");
}

/// Au-delà du dernier palier : la concentration est clampée à la borne haute du
/// barème ⇒ l'AQI plafonne à 500 (et une couverture 24/24 est valide).
#[tokio::test]
async fn q2_concentration_above_scale_clamps_to_aqi_500() {
    let ch = ch();
    let loc = test_location(6);
    let h = current_hour();

    // 24 buckets pleins (h−23 … h) à 400.0 µg/m³ de pm25.
    let samples: Vec<Sample> = (0..24)
        .map(|i| sample(loc, "pm25", h - Duration::hours(i), 400.0, h))
        .collect();
    ch.insert(&samples).await;

    let rows = ch
        .rows(
            q2_sql(),
            &[("param_loc", loc.to_string()), ("param_at", fmt(h))],
        )
        .await;

    assert_eq!(rows.len(), 1, "une seule ligne pm25 attendue : {rows:?}");
    let r = &rows[0];
    // Attendu (calcul manuel) : moyenne 400.0 → troncature 400.0 > 325.4 (borne
    // haute pm25) → clamp à 325.4 → palier (225.5–325.4 → AQI 301–500)
    // → AQI = (500−301)/(325.4−225.5) × (325.4−225.5) + 301 + 0.5 = 500.5 → floor = 500.
    assert_eq!(i64_of(r, "aqi"), 500, "l'AQI doit plafonner à 500");
    assert_eq!(u64_of(r, "hours_present"), 24);
    let coverage = f64_of(r, "coverage");
    assert!(
        approx_eq(coverage, 1.0),
        "coverage attendue 1.0 : {coverage}"
    );
    assert_eq!(u64_of(r, "is_valid"), 1, "24/24 ≥ 0.75 ⇒ valide");
}

/// Multi-polluants : `is_dominant` ne marque que l'AQI maximal (convention EPA :
/// l'AQI global du lieu est le max des AQI par polluant).
#[tokio::test]
async fn q2_dominant_flag_marks_only_the_max_aqi() {
    let ch = ch();
    let loc = test_location(7);
    let h = current_hour();

    // AQI attendus (calculs manuels) :
    //   pm25 30.0 µg/m³ → floor(30.0, 1) = 30.0 → palier (9.1–35.4 → 51–100)
    //        → AQI = 49/26.3 × (30.0−9.1) + 51 + 0.5 = 38.939… + 51.5 = 90.439… → 90 ;
    //   o3   95.0 µg/m³ → 48 ppb → AQI 44 (cf. test o3) ;
    //   no2  52.0 µg/m³ → 27 ppb → AQI 25 (cf. test no2).
    // ⇒ dominant = pm25 (90), unique.
    ch.insert(&[
        sample(loc, "pm25", h, 30.0, h),
        sample(loc, "o3", h, 95.0, h),
        sample(loc, "no2", h, 52.0, h),
    ])
    .await;

    let rows = ch
        .rows(
            q2_sql(),
            &[("param_loc", loc.to_string()), ("param_at", fmt(h))],
        )
        .await;

    assert_eq!(rows.len(), 3, "3 polluants attendus : {rows:?}");
    let dominants: Vec<&Value> = rows
        .iter()
        .filter(|r| u64_of(r, "is_dominant") == 1)
        .collect();
    assert_eq!(
        dominants.len(),
        1,
        "exactement un polluant dominant attendu : {rows:?}"
    );
    assert_eq!(str_of(dominants[0], "parameter"), "pm25");
    assert_eq!(i64_of(dominants[0], "aqi"), 90);

    // Et les AQI des non-dominants sont bien ceux calculés à la main.
    let aqi_of = |p: &str| {
        rows.iter()
            .find(|r| str_of(r, "parameter") == p)
            .map(|r| i64_of(r, "aqi"))
            .unwrap_or_else(|| panic!("ligne {p} attendue : {rows:?}"))
    };
    assert_eq!(aqi_of("o3"), 44);
    assert_eq!(aqi_of("no2"), 25);
}

// ─────────────────────────────────────────────────────────────────────────────
// Q3 — O3 : maximum journalier de la moyenne glissante 8 h
// ─────────────────────────────────────────────────────────────────────────────

/// Rattachement au jour de DÉPART + épilogue : une fenêtre démarrant à 23:00
/// le jour J et finissant à 06:59 J+1 compte pour J ; ses buckets de J+1
/// (APRÈS {to}) sont lus grâce à l'épilogue +7 h. Et le max journalier est le
/// max des MOYENNES 8 h, pas des valeurs brutes.
#[tokio::test]
async fn q3_window_belongs_to_its_start_day_and_epilogue_feeds_it() {
    let ch = ch();
    let loc = test_location(9);
    // Deux jours UTC complets déjà passés : J−2 (interrogé) et J−1 (épilogue).
    let d1 = day_start(2);
    let d2 = day_start(1);

    ch.insert(&[
        sample(loc, "o3", d1 + Duration::hours(20), 12.0, d2),
        sample(loc, "o3", d1 + Duration::hours(21), 18.0, d2),
        sample(loc, "o3", d1 + Duration::hours(23), 31.0, d2),
        // J−1 : APRÈS {to}, mais dans l'épilogue +7 h — nourrit les fenêtres de J−2.
        sample(loc, "o3", d2, 60.0, d2),
        sample(loc, "o3", d2 + Duration::hours(1), 6.0, d2),
        sample(loc, "o3", d2 + Duration::hours(6), 24.0, d2),
    ])
    .await;

    // [{from}, {to}[ = [J−2, J−1[ : seul le jour J−2 sort.
    let rows = ch
        .rows(
            q3_sql(),
            &[
                ("param_loc", loc.to_string()),
                ("param_from", fmt(d1)),
                ("param_to", fmt(d2)),
            ],
        )
        .await;

    assert_eq!(rows.len(), 1, "un seul jour (J−2) attendu : {rows:?}");
    let r = &rows[0];
    assert_eq!(str_of(r, "day"), d1.format("%Y-%m-%d").to_string());
    // Fenêtres matérialisées de J−2 (départ = fin − 7 h ; calcul manuel) :
    //   fin 20:00     (départ 13:00) : {12}             → 12.0,   1 h
    //   fin 21:00     (départ 14:00) : {12,18}          → 15.0,   2 h
    //   fin 23:00     (départ 16:00) : {12,18,31}       → 61/3,   3 h
    //   fin 00:00 J−1 (départ 17:00) : {12,18,31,60}    → 30.25,  4 h
    //   fin 01:00 J−1 (départ 18:00) : {12,18,31,60,6}  → 25.4,   5 h
    //   fin 06:00 J−1 (départ 23:00) : {31,60,6,24}     → 30.25,  4 h ← départ 23:00 J−2
    // Aucune fenêtre ≥ 6/8 h ⇒ FALLBACK : max des présentes = 30.25 — qui
    // n'est NI le max brut (60.0, mesuré sur J−1 !) NI une valeur brute de J−2.
    let max = f64_of(r, "daily_max_8h");
    assert!(approx_eq(max, 30.25), "daily_max_8h attendu 30.25 : {max}");
    // 6 fenêtres rattachées à J−2 (dont 3 finissant sur J−1). Un rattachement
    // (faux) au jour de FIN n'en laisserait que 3.
    assert_eq!(u64_of(r, "windows_present"), 6);
    assert_eq!(u64_of(r, "valid_windows"), 0);
    assert_eq!(
        u64_of(r, "day_valid"),
        0,
        "aucune fenêtre valide ⇒ jour invalide"
    );
}

/// Le max journalier ne retient que les fenêtres VALIDES (≥ 6/8 h) quand il y
/// en a : une fenêtre clairsemée à forte moyenne (pic isolé) ne masque pas la
/// valeur réglementaire.
#[tokio::test]
async fn q3_daily_max_only_considers_valid_windows_when_any() {
    let ch = ch();
    let loc = test_location(10);
    let d = day_start(1); // J−1, jour UTC complet déjà passé

    ch.insert(&[
        sample(loc, "o3", d + Duration::hours(1), 6.0, d),
        sample(loc, "o3", d + Duration::hours(6), 24.0, d),
        sample(loc, "o3", d + Duration::hours(8), 100.0, d),
        sample(loc, "o3", d + Duration::hours(9), 10.0, d),
        sample(loc, "o3", d + Duration::hours(10), 12.0, d),
        sample(loc, "o3", d + Duration::hours(11), 14.0, d),
        sample(loc, "o3", d + Duration::hours(12), 16.0, d),
        sample(loc, "o3", d + Duration::hours(13), 11.0, d),
    ])
    .await;

    let rows = ch
        .rows(
            q3_sql(),
            &[
                ("param_loc", loc.to_string()),
                ("param_from", fmt(d)),
                ("param_to", fmt(d + Duration::days(1))),
            ],
        )
        .await;

    assert_eq!(rows.len(), 1, "un seul jour (J−1) attendu : {rows:?}");
    let r = &rows[0];
    assert_eq!(str_of(r, "day"), d.format("%Y-%m-%d").to_string());
    // Fenêtres matérialisées de J−1 (départ ≥ 00:00 ⇒ fin ≥ 07:00 ; calcul manuel) :
    //   fin 08:00 (départ 01:00) : {6,24,100}              → 130/3 ≈ 43.33, 3 h INVALIDE
    //   fin 09:00 (départ 02:00) : {24,100,10}             → 134/3 ≈ 44.67, 3 h INVALIDE
    //   fin 10:00 (départ 03:00) : {24,100,10,12}          → 36.5,          4 h INVALIDE
    //   fin 11:00 (départ 04:00) : {24,100,10,12,14}       → 32.0,          5 h INVALIDE
    //   fin 12:00 (départ 05:00) : {24,100,10,12,14,16}    → 176/6 ≈ 29.33, 6 h VALIDE
    //   fin 13:00 (départ 06:00) : {24,100,10,12,14,16,11} → 187/7 ≈ 26.71, 7 h VALIDE
    // (les fins 01:00/06:00 démarrent la VEILLE ⇒ hors [{from}, {to}[.)
    // 2 fenêtres valides ⇒ max des VALIDES = 176/6 = 29.333… — un max naïf sur
    // toutes les fenêtres présentes rendrait 134/3 ≈ 44.67.
    let max = f64_of(r, "daily_max_8h");
    assert!(
        approx_eq(max, 176.0 / 6.0),
        "daily_max_8h attendu 176/6 ≈ 29.333 : {max}"
    );
    assert_eq!(u64_of(r, "windows_present"), 6);
    assert_eq!(u64_of(r, "valid_windows"), 2);
    assert_eq!(u64_of(r, "day_valid"), 0, "2 < 18 ⇒ jour invalide");
}

/// Jour PLEIN (24 buckets + 7 h d'épilogue) : les 24 fenêtres du jour sont
/// présentes et valides (8/8 h), day_valid = 1, et le max journalier est porté
/// par les 8 fenêtres contenant le pic horaire.
#[tokio::test]
async fn q3_full_day_yields_24_valid_windows_and_day_valid() {
    let ch = ch();
    let loc = test_location(11);
    let d = day_start(2); // J−2 ; l'épilogue couvre J−1 00:00..06:00 (passé aussi)

    // 31 buckets pleins (J−2 00:00 → J−1 06:00) à 10.0, sauf un pic à h+15 : 50.0.
    let samples: Vec<Sample> = (0..31)
        .map(|i| {
            let value = if i == 15 { 50.0 } else { 10.0 };
            sample(loc, "o3", d + Duration::hours(i), value, d)
        })
        .collect();
    ch.insert(&samples).await;

    let rows = ch
        .rows(
            q3_sql(),
            &[
                ("param_loc", loc.to_string()),
                ("param_from", fmt(d)),
                ("param_to", fmt(d + Duration::days(1))),
            ],
        )
        .await;

    assert_eq!(rows.len(), 1, "un seul jour (J−2) attendu : {rows:?}");
    let r = &rows[0];
    assert_eq!(str_of(r, "day"), d.format("%Y-%m-%d").to_string());
    // 24 fenêtres (départs 00:00..23:00), toutes à 8/8 h ⇒ toutes valides,
    // day_valid = 1 (24 ≥ 18). Calcul manuel : les 8 fenêtres couvrant le pic
    // h+15 (départs 08:00..15:00) valent (7 × 10 + 50) / 8 = 15.0 ; les autres
    // 10.0. daily_max_8h = 15.0 ≠ 50.0 : max des moyennes, pas des bruts.
    let max = f64_of(r, "daily_max_8h");
    assert!(approx_eq(max, 15.0), "daily_max_8h attendu 15.0 : {max}");
    assert_eq!(u64_of(r, "windows_present"), 24);
    assert_eq!(u64_of(r, "valid_windows"), 24);
    assert_eq!(u64_of(r, "day_valid"), 1, "24 ≥ 18 ⇒ jour valide");
}

// ─────────────────────────────────────────────────────────────────────────────
// Q4 — NO2 : moyenne annuelle (rollup measurements_daily)
// ─────────────────────────────────────────────────────────────────────────────
// L'INSERT dans `measurements` alimente AUTOMATIQUEMENT le rollup via
// mv_measurements_daily. Particularité : le rollup a une rétention de 5 ans
// (pas 90 j) ⇒ les lignes de test y survivront au TTL des bruts — sans
// incidence, les location_id réservés étant uniques par exécution.

/// La moyenne annuelle est pondérée par MESURE (avgMerge), pas par jour :
/// 2 mesures un jour + 1 un autre ⇒ moyenne sur 3 valeurs.
#[tokio::test]
async fn q4_annual_mean_is_weighted_by_measurement_not_by_day() {
    let ch = ch();
    let loc = test_location(12);
    // Deux jours UTC complets déjà passés et DANS LA MÊME ANNÉE : (J−2, J−1)
    // en temps normal ; (J−3, J−2) si J−1 vient de changer d'année (un 2 janvier).
    let (day_a, day_b) = if day_start(2).year() == day_start(1).year() {
        (day_start(2), day_start(1))
    } else {
        (day_start(3), day_start(2))
    };
    let year = day_b.year();

    ch.insert(&[
        // day_a : DEUX mesures (10.0 + 20.0) ; day_b : UNE seule (60.0).
        sample(loc, "no2", day_a + Duration::hours(8), 10.0, day_b),
        sample(loc, "no2", day_a + Duration::hours(12), 20.0, day_b),
        sample(loc, "no2", day_b + Duration::hours(9), 60.0, day_b),
    ])
    .await;

    let rows = ch
        .rows(
            q4_sql(),
            &[
                ("param_loc", loc.to_string()),
                ("param_year", year.to_string()),
            ],
        )
        .await;

    assert_eq!(
        rows.len(),
        1,
        "une seule ligne annuelle attendue : {rows:?}"
    );
    let r = &rows[0];
    assert_eq!(i64_of(r, "year"), i64::from(year));
    // Attendu (calcul manuel) : moyenne PAR MESURE = (10 + 20 + 60) / 3 = 30.0.
    //   Une (fausse) moyenne des moyennes journalières rendrait (15 + 60) / 2 = 37.5.
    let avg = f64_of(r, "annual_avg");
    assert!(approx_eq(avg, 30.0), "annual_avg attendue 30.0 : {avg}");
    assert_eq!(u64_of(r, "days_present"), 2);
    // 365/366 VRAIMENT calculés : recalcul indépendant côté Rust (grégorien) —
    // 365 pour l'année nominale 2026.
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let expected_days: u64 = if leap { 366 } else { 365 };
    assert_eq!(u64_of(r, "days_in_year"), expected_days);
    let coverage = f64_of(r, "coverage");
    assert!(
        approx_eq(coverage, 2.0 / expected_days as f64),
        "coverage attendue 2/{expected_days} : {coverage}"
    );
    assert_eq!(u64_of(r, "is_valid"), 0, "2 jours sur l'année ⇒ invalide");
}

/// GÈLE l'approximation documentée (en-tête Q4 + 01_schema.sql §C.4) : la MV
/// alimentant le rollup se déclenche AVANT la dédup ReplacingMergeTree, donc
/// une clé ré-ingérée (deux INSERT séparés ⇒ deux déclenchements de MV) compte
/// DEUX fois dans la moyenne annuelle — contrairement à Q1/Q2/Q3 qui lisent la
/// vérité argMax. Si ce test casse un jour (annual_avg = 30.0), c'est que
/// l'approximation a disparu : mettre à jour la doc de Q4 et §C.4.
#[tokio::test]
async fn q4_rollup_overcounts_reingested_duplicates_by_design() {
    let ch = ch();
    let loc = test_location(13);
    let d = day_start(1); // J−1 ; l'année testée est CELLE DE CE JOUR (stable)
    let year = d.year();

    // MÊME clé (loc, no2, J−1 10:00) ré-ingérée : la vérité dédupliquée serait
    // 30.0 (dernier ingested_at) et UNE seule mesure.
    let at = d + Duration::hours(10);
    ch.insert(&[sample(loc, "no2", at, 10.0, d)]).await;
    ch.insert(&[sample(loc, "no2", at, 30.0, d + Duration::seconds(1))])
        .await;

    let rows = ch
        .rows(
            q4_sql(),
            &[
                ("param_loc", loc.to_string()),
                ("param_year", year.to_string()),
            ],
        )
        .await;

    assert_eq!(
        rows.len(),
        1,
        "une seule ligne annuelle attendue : {rows:?}"
    );
    let r = &rows[0];
    // Attendu (calcul manuel, comportement GELÉ) : (10 + 30) / 2 = 20.0 — les
    // DEUX versions de la clé comptent. days_present reste 1 (même bucket_day).
    let avg = f64_of(r, "annual_avg");
    assert!(
        approx_eq(avg, 20.0),
        "annual_avg attendue 20.0 (sur-comptage MV assumé) : {avg}"
    );
    assert_eq!(u64_of(r, "days_present"), 1);
    assert_eq!(u64_of(r, "is_valid"), 0, "1 jour sur l'année ⇒ invalide");
}
