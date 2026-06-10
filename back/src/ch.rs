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
}
