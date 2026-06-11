//! Client ClickHouse via HTTP (reqwest). Valeurs en paramètres `param_*` (anti-injection),
//! fragments SQL non paramétrables en allowlist stricte.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone)]
pub struct ClickhouseClient {
    http: Client,
    base_url: String,
    user: String,
    password: String,
    database: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ClickhouseError {
    #[error("clickhouse http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("clickhouse json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Mesure « nouvelle » vue par la boucle de matching (B7) : mêmes colonnes que
/// [`MeasurementRow`] + le `ingested_at` maximal retenu — c'est LUI qui sert de
/// curseur (watermark) : la boucle relit « tout ce qui est arrivé depuis », pas
/// « tout ce qui a été mesuré depuis » (un backfill ancien doit être réévalué).
#[derive(Debug, Deserialize)]
pub struct NewMeasurementRow {
    pub location_id: u64,
    pub sensor_id: u64,
    pub parameter: String,
    pub unit: String,
    /// Horodatage UTC de la mesure (`YYYY-MM-DD hh:mm:ss.fff`).
    pub measured_at: String,
    pub value: f64,
    /// `max(ingested_at)` du groupe (`YYYY-MM-DD hh:mm:ss.fff`) — base du watermark.
    /// Nom DISTINCT de la colonne : ClickHouse substitue les alias jusque dans le
    /// WHERE (un alias `ingested_at` y injecterait l'agrégat → ILLEGAL_AGGREGATION).
    pub last_ingested_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct MeasurementRow {
    #[schema(example = 4085)]
    pub location_id: u64,
    pub sensor_id: u64,
    #[schema(example = "pm25")]
    pub parameter: String,
    #[schema(example = "µg/m³")]
    pub unit: String,
    /// Horodatage UTC de la mesure (`YYYY-MM-DD hh:mm:ss.fff`).
    pub measured_at: String,
    /// Valeur dédupliquée (dernière ingestion : `argMax(value, ingested_at)`).
    pub value: f64,
}

/// Allowlist stricte des polluants (anti-injection ; cohérent avec parameters.code en base).
pub fn validate_parameter(p: &str) -> Option<&'static str> {
    match p {
        "pm25" => Some("pm25"),
        "pm10" => Some("pm10"),
        "no2" => Some("no2"),
        "o3" => Some("o3"),
        "so2" => Some("so2"),
        "co" => Some("co"),
        _ => None,
    }
}

/// Requête B5 **Q2 (`aqi_snapshot`) EMBARQUÉE** dans le crate (`back/src/aqi_snapshot.sql`),
/// servie telle quelle par le handler `/api/aqi` (B10).
///
/// Pourquoi une COPIE embarquée plutôt qu'un `include_str!` du fichier canonique
/// `db/clickhouse/queries/rolling_regulatory.sql` : `include_str!` est résolu **à la
/// compilation**, or le contexte de build Docker du back est **`back/` SEUL** (le
/// dossier `db/` n'y est pas copié) — un chemin hors-crate casse `cargo build --release`
/// dans l'image (`back/src/aqi_snapshot.sql` est, lui, copié par `COPY src ./src`).
/// La copie est tenue SYNCHRONE du canonique par un test de dérive
/// (`tests/ch.rs::embedded_aqi_sql_matches_canonical_q2`, comparaison normalisée) :
/// modifier Q2 sans répercuter ici fait ÉCHOUER la CI.
pub const AQI_SNAPSHOT_SQL: &str = include_str!("aqi_snapshot.sql");

/// Une ligne de Q2 (`aqi_snapshot`) — l'AQI US EPA d'UN polluant pour UNE station, à
/// l'instant demandé. Champs non utilisés par B10 (window_end, rolling_avg_ugm3…)
/// ignorés par serde. `is_valid`/`is_dominant` arrivent en 0/1 (UInt8 ClickHouse).
#[derive(Debug, Deserialize)]
pub struct AqiSnapshotRow {
    pub parameter: String,
    /// AQI entier (arrondi EPA), 0..=500.
    pub aqi: i32,
    /// Concentration glissante convertie (ppb pour o3/no2, µg/m³ sinon).
    pub conc_value: f64,
    pub conc_unit: String,
    /// Couverture de la fenêtre (0..1).
    pub coverage: f64,
    /// Validité réglementaire (≥ 75 % de couverture) — 0/1.
    pub is_valid: u8,
    /// Polluant dominant du lieu (AQI = max) — 0/1.
    pub is_dominant: u8,
}

/// Requête de dose d'exposition (B9a-3) EMBARQUÉE (`back/src/exposure_dose.sql`),
/// copie synchrone du canonique `db/clickhouse/queries/exposure_dose.sql` (test de
/// dérive `tests/ch.rs::embedded_exposure_dose_sql_matches_canonical` ; même raison
/// que `AQI_SNAPSHOT_SQL` : build Docker du back = `back/` seul).
pub const EXPOSURE_DOSE_SQL: &str = include_str!("exposure_dose.sql");

/// Résultat brut de la requête de dose : heures > seuil et heures évaluées (couverture).
#[derive(Debug, Deserialize)]
pub struct DoseRow {
    pub hours_over_threshold: u64,
    pub sample_count: u64,
}

/// Paramètres de la requête de dose (cf. `exposure_dose.sql`).
pub struct DoseParams<'a> {
    /// Stations openaq du lieu (règle MAX multi-stations).
    pub locs: &'a [i64],
    pub parameter: &'a str,
    /// Fenêtre de moyennage en heures (1 / 8 / 24).
    pub avg_hours: u32,
    pub threshold: f64,
    /// Bornes de période, dates locales `YYYY-MM-DD` (incluses).
    pub from: &'a str,
    pub to: &'a str,
    /// Plage horaire quotidienne en minutes du jour `[start, end)`.
    pub start_min: u16,
    pub end_min: u16,
    /// Bitmask des jours (bit0=Lundi … bit6=Dimanche).
    pub days_mask: i16,
    pub tz: &'a str,
}

impl ClickhouseClient {
    pub fn new(base_url: String, user: String, password: String, database: String) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .expect("construction du client reqwest");
        Self {
            http,
            base_url,
            user,
            password,
            database,
        }
    }

    pub async fn ping(&self) -> Result<(), ClickhouseError> {
        self.http
            .get(format!("{}/ping", self.base_url))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// Lecture dédupliquée (argMax sur ingested_at) filtrée + paginée.
    pub async fn query_measurements(
        &self,
        location_id: u64,
        parameter: &str,
        from: &str,
        to: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<MeasurementRow>, ClickhouseError> {
        let sql = r#"
            SELECT
                location_id,
                sensor_id,
                parameter,
                any(unit)                  AS unit,
                measured_at,
                argMax(value, ingested_at) AS value
            FROM quarity.measurements
            WHERE location_id = {loc:UInt64}
              AND parameter   = {param:String}
              AND measured_at >= parseDateTime64BestEffort({from:String}, 3, 'UTC')
              AND measured_at <  parseDateTime64BestEffort({to:String}, 3, 'UTC')
            GROUP BY location_id, sensor_id, parameter, measured_at
            ORDER BY measured_at DESC
            LIMIT {lim:UInt32} OFFSET {off:UInt32}
            FORMAT JSONEachRow
        "#;

        let resp = self
            .http
            .post(&self.base_url)
            .basic_auth(&self.user, Some(&self.password))
            .query(&[
                ("database", self.database.as_str()),
                // UInt64 en nombres JSON (et non en chaînes) pour le parsing u64 :
                ("output_format_json_quote_64bit_integers", "0"),
            ])
            .query(&[
                ("param_loc", location_id.to_string()),
                ("param_param", parameter.to_string()),
                ("param_from", from.to_string()),
                ("param_to", to.to_string()),
                ("param_lim", limit.to_string()),
                ("param_off", offset.to_string()),
            ])
            .body(sql)
            .send()
            .await?
            .error_for_status()?;

        let text = resp.text().await?;
        let mut out = Vec::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            out.push(serde_json::from_str::<MeasurementRow>(line)?);
        }
        Ok(out)
    }

    /// Mesures arrivées depuis `since` (curseur sur `ingested_at`, EXCLUSIF),
    /// dédupliquées (`argMax`), bornées à `limit`, triées par arrivée croissante.
    ///
    /// Conçu pour la boucle de matching (B7) : le tri par `ingested_at` croissant
    /// permet d'avancer le watermark au DERNIER `ingested_at` lu — si `limit`
    /// tronque, le tick suivant reprend exactement où celui-ci s'est arrêté
    /// (l'idempotence de l'insertion tolère de toute façon un recouvrement).
    /// Aucun filtre par station ici : l'index des règles trie en mémoire —
    /// la sélectivité du `WHERE ingested_at` suffit (index MinMax implicite des
    /// parts récentes).
    pub async fn query_new_measurements(
        &self,
        since: &str,
        limit: u32,
    ) -> Result<Vec<NewMeasurementRow>, ClickhouseError> {
        let sql = r#"
            SELECT
                location_id,
                sensor_id,
                parameter,
                any(unit)                  AS unit,
                measured_at,
                argMax(value, ingested_at) AS value,
                max(ingested_at)           AS last_ingested_at
            FROM quarity.measurements
            WHERE ingested_at > parseDateTime64BestEffort({since:String}, 3, 'UTC')
            GROUP BY location_id, sensor_id, parameter, measured_at
            ORDER BY last_ingested_at ASC
            LIMIT {lim:UInt32}
            FORMAT JSONEachRow
        "#;

        let resp = self
            .http
            .post(&self.base_url)
            .basic_auth(&self.user, Some(&self.password))
            .query(&[
                ("database", self.database.as_str()),
                ("output_format_json_quote_64bit_integers", "0"),
            ])
            .query(&[
                ("param_since", since.to_string()),
                ("param_lim", limit.to_string()),
            ])
            .body(sql)
            .send()
            .await?
            .error_for_status()?;

        let text = resp.text().await?;
        let mut out = Vec::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            out.push(serde_json::from_str::<NewMeasurementRow>(line)?);
        }
        Ok(out)
    }

    /// AQI US EPA instantané (B5 Q2) d'UNE station OpenAQ, à l'instant `at`
    /// (`YYYY-MM-DD hh:mm:ss[.fff]`, UTC — la requête ancre sur l'heure pleine).
    /// Une ligne par polluant à breakpoints AYANT des données dans sa fenêtre
    /// (24/24/8/1 h) ; aucune ligne pour un polluant/une station sans donnée récente.
    pub async fn query_aqi_snapshot(
        &self,
        location_id: u64,
        at: &str,
    ) -> Result<Vec<AqiSnapshotRow>, ClickhouseError> {
        let resp = self
            .http
            .post(&self.base_url)
            .basic_auth(&self.user, Some(&self.password))
            .query(&[
                ("database", self.database.as_str()),
                ("output_format_json_quote_64bit_integers", "0"),
            ])
            .query(&[
                ("param_loc", location_id.to_string()),
                ("param_at", at.to_string()),
            ])
            .body(AQI_SNAPSHOT_SQL)
            .send()
            .await?
            .error_for_status()?;

        let text = resp.text().await?;
        let mut out = Vec::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            out.push(serde_json::from_str::<AqiSnapshotRow>(line)?);
        }
        Ok(out)
    }

    /// Dose d'exposition (B9a-3) : heures où la concentration glissante MAX multi-stations
    /// dépasse le seuil, dans la plage horaire locale du profil, sur la période. L'agrégat
    /// (sans GROUP BY) renvoie TOUJOURS une ligne — `0/0` si aucune donnée.
    pub async fn query_exposure_dose(
        &self,
        p: &DoseParams<'_>,
    ) -> Result<DoseRow, ClickhouseError> {
        let locs = format!(
            "[{}]",
            p.locs
                .iter()
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        let resp = self
            .http
            .post(&self.base_url)
            .basic_auth(&self.user, Some(&self.password))
            .query(&[
                ("database", self.database.as_str()),
                ("output_format_json_quote_64bit_integers", "0"),
            ])
            .query(&[
                ("param_locs", locs),
                ("param_param", p.parameter.to_string()),
                ("param_avg_hours", p.avg_hours.to_string()),
                ("param_threshold", p.threshold.to_string()),
                ("param_from", p.from.to_string()),
                ("param_to", p.to.to_string()),
                ("param_start_min", p.start_min.to_string()),
                ("param_end_min", p.end_min.to_string()),
                ("param_days_mask", p.days_mask.to_string()),
                ("param_tz", p.tz.to_string()),
            ])
            .body(EXPOSURE_DOSE_SQL)
            .send()
            .await?
            .error_for_status()?;

        let text = resp.text().await?;
        if let Some(line) = text.lines().find(|l| !l.trim().is_empty()) {
            return Ok(serde_json::from_str::<DoseRow>(line)?);
        }
        Ok(DoseRow {
            hours_over_threshold: 0,
            sample_count: 0,
        })
    }
}
