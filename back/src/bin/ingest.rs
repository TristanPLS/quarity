//! Ingestion OpenAQ -> ClickHouse (mesures) + Postgres (liaison org pour la visibilité front).
//!
//! Usage : `quarity-ingest [location_id]`  (défaut 4085 = NICE PROMENADE)
//! Env requis : OPENAQ_API_KEY, CLICKHOUSE_URL/USER/PASSWORD/DB, DATABASE_URL.
//! Env optionnels : OPENAQ_LOCATION_ID, OPENAQ_DAYS (défaut 7), OPENAQ_ORG_SLUG (défaut agglo-riviera).
//!
//! Récupère une station OpenAQ v3, ses mesures horaires des N derniers jours, les insère dans
//! quarity.measurements, et lie la station à une organisation (ref_locations + ref_sensors +
//! tracked_location + tracked_location_stations) pour qu'elle soit interrogeable depuis le front.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

const OPENAQ_BASE: &str = "https://api.openaq.org";
/// Polluants gérés par Quarity (allowlist alignée sur la table `parameters`).
const ALLOWED: &[&str] = &["pm25", "pm10", "no2", "o3", "so2", "co"];

// ---------- Structures de réponse OpenAQ v3 ----------
#[derive(Deserialize)]
struct OaqResp<T> {
    results: Vec<T>,
}
#[derive(Deserialize)]
struct OaqLocation {
    id: i64,
    name: Option<String>,
    country: Option<OaqCountry>,
    coordinates: Option<OaqCoords>,
    #[serde(default)]
    sensors: Vec<OaqSensor>,
}
#[derive(Deserialize)]
struct OaqCountry {
    code: Option<String>,
}
#[derive(Deserialize, Clone, Copy)]
struct OaqCoords {
    latitude: f64,
    longitude: f64,
}
#[derive(Deserialize)]
struct OaqSensor {
    id: i64,
    parameter: OaqParam,
}
#[derive(Deserialize)]
struct OaqParam {
    name: String,
    units: Option<String>,
}
#[derive(Deserialize)]
struct OaqHour {
    value: Option<f64>,
    parameter: OaqParam,
    period: OaqPeriod,
}
#[derive(Deserialize)]
struct OaqPeriod {
    #[serde(rename = "datetimeFrom")]
    datetime_from: Option<OaqDt>,
}
#[derive(Deserialize)]
struct OaqDt {
    utc: String,
}

// ---------- Ligne ClickHouse (FORMAT JSONEachRow) ----------
#[derive(Serialize)]
struct ChRow {
    location_id: u64,
    sensor_id: u64,
    parameter: String,
    country: String,
    unit: String,
    measured_at: String, // 'YYYY-MM-DD HH:MM:SS.mmm'
    value: f64,
    latitude: f64,
    longitude: f64,
}

fn env(k: &str) -> Result<String> {
    std::env::var(k).map_err(|_| anyhow!("variable d'environnement manquante : {k}"))
}

/// "2026-05-28T02:00:00Z" -> "2026-05-28 02:00:00.000" (DateTime64(3) ClickHouse).
fn to_ch_datetime(utc: &str) -> String {
    let s = utc.trim_end_matches('Z').replace('T', " ");
    if s.contains('.') {
        s
    } else {
        format!("{s}.000")
    }
}

async fn oaq_get<T: for<'de> Deserialize<'de>>(client: &Client, key: &str, url: &str) -> Result<Vec<T>> {
    let resp = client
        .get(url)
        .header("X-API-Key", key)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("statut HTTP OpenAQ pour {url}"))?;
    let parsed: OaqResp<T> = resp.json().await.context("parse JSON OpenAQ")?;
    Ok(parsed.results)
}

#[tokio::main]
async fn main() -> Result<()> {
    let key = env("OPENAQ_API_KEY")?;
    let ch_url = env("CLICKHOUSE_URL")?;
    let ch_user = env("CLICKHOUSE_USER")?;
    let ch_pass = env("CLICKHOUSE_PASSWORD")?;
    let ch_db = std::env::var("CLICKHOUSE_DB").unwrap_or_else(|_| "quarity".into());
    let database_url = env("DATABASE_URL")?;

    let location_id: i64 = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("OPENAQ_LOCATION_ID").ok())
        .unwrap_or_else(|| "4085".into())
        .parse()
        .context("location_id invalide")?;
    let days: i64 = std::env::var("OPENAQ_DAYS").ok().and_then(|s| s.parse().ok()).unwrap_or(7);
    let org_slug = std::env::var("OPENAQ_ORG_SLUG").unwrap_or_else(|_| "agglo-riviera".into());

    let http = Client::builder().timeout(Duration::from_secs(30)).build()?;

    // 1) Station + capteurs
    let mut locs: Vec<OaqLocation> = oaq_get(&http, &key, &format!("{OPENAQ_BASE}/v3/locations/{location_id}")).await?;
    let loc = locs.pop().ok_or_else(|| anyhow!("location {location_id} introuvable"))?;
    let name = loc.name.clone().unwrap_or_else(|| format!("OpenAQ {location_id}"));
    let country = loc.country.as_ref().and_then(|c| c.code.clone()).unwrap_or_else(|| "XX".into());
    let coords = loc.coordinates.unwrap_or(OaqCoords { latitude: 0.0, longitude: 0.0 });
    println!("Station #{location_id} « {name} » ({country}) — {} capteur(s)", loc.sensors.len());

    // 2) Fenêtre temporelle : N derniers jours
    let to = Utc::now();
    let from = to - chrono::Duration::days(days);
    let from_s = from.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let to_s = to.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    println!("Fenêtre : {from_s} -> {to_s}");

    // 3) Mesures horaires par capteur (allowlist) -> lignes ClickHouse
    let mut rows: Vec<ChRow> = Vec::new();
    let mut per_param: HashMap<String, usize> = HashMap::new();
    let mut sensors_to_link: Vec<(i64, String)> = Vec::new(); // (openaq_sensor_id, parameter_code)

    for s in &loc.sensors {
        let pname = s.parameter.name.to_lowercase();
        if !ALLOWED.contains(&pname.as_str()) {
            continue;
        }
        sensors_to_link.push((s.id, pname.clone()));
        let url = format!(
            "{OPENAQ_BASE}/v3/sensors/{}/hours?datetime_from={}&datetime_to={}&limit=1000",
            s.id, from_s, to_s
        );
        let hours: Vec<OaqHour> = oaq_get(&http, &key, &url).await?;
        for h in hours {
            let (Some(value), Some(dt)) = (h.value, h.period.datetime_from.as_ref()) else {
                continue;
            };
            rows.push(ChRow {
                location_id: location_id as u64,
                sensor_id: s.id as u64,
                parameter: pname.clone(),
                country: country.clone(),
                unit: h.parameter.units.unwrap_or_else(|| "µg/m³".into()),
                measured_at: to_ch_datetime(&dt.utc),
                value,
                latitude: coords.latitude,
                longitude: coords.longitude,
            });
            *per_param.entry(pname.clone()).or_insert(0) += 1;
        }
    }
    println!("Mesures récupérées : {} (", rows.len());
    for (p, n) in &per_param {
        println!("   {p}: {n}");
    }
    println!(")");

    if rows.is_empty() {
        println!("Aucune mesure dans la fenêtre — rien à insérer.");
        return Ok(());
    }

    // 4) INSERT batch dans ClickHouse (FORMAT JSONEachRow)
    let mut body = String::from(
        "INSERT INTO quarity.measurements (location_id, sensor_id, parameter, country, unit, measured_at, value, latitude, longitude) FORMAT JSONEachRow\n",
    );
    for r in &rows {
        body.push_str(&serde_json::to_string(r)?);
        body.push('\n');
    }
    http.post(&ch_url)
        .basic_auth(&ch_user, Some(&ch_pass))
        .query(&[("database", ch_db.as_str())])
        .body(body)
        .send()
        .await
        .context("INSERT ClickHouse")?
        .error_for_status()
        .context("statut HTTP ClickHouse")?;
    println!("✓ {} mesures insérées dans quarity.measurements", rows.len());

    // 5) Liaison Postgres : la station devient suivie par l'org (visibilité front)
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&database_url)
        .await
        .context("connexion Postgres")?;
    link_org(&pool, location_id, &name, &country, coords, &org_slug, &sensors_to_link).await?;

    println!();
    println!("✅ Terminé. Dans le front, interroge : location_id={location_id}, parameter=pm25 (org « {org_slug} »).");
    Ok(())
}

async fn link_org(
    pool: &PgPool,
    location_id: i64,
    name: &str,
    country: &str,
    coords: OaqCoords,
    org_slug: &str,
    sensors: &[(i64, String)],
) -> Result<()> {
    // org
    let org_id: i64 = sqlx::query_scalar("SELECT id FROM organizations WHERE slug = $1")
        .bind(org_slug)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow!("organisation '{org_slug}' introuvable (seed appliqué ?)"))?;

    // ref_locations (upsert sur la clé naturelle OpenAQ)
    let ref_location_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO ref_locations (openaq_location_id, name, country, latitude, longitude, last_seen_at)
        VALUES ($1, $2, $3, $4, $5, now())
        ON CONFLICT (openaq_location_id) DO UPDATE SET last_seen_at = now(), name = EXCLUDED.name
        RETURNING id
        "#,
    )
    .bind(location_id)
    .bind(name)
    .bind(&country[..country.len().min(2)])
    .bind(coords.latitude)
    .bind(coords.longitude)
    .fetch_one(pool)
    .await
    .context("upsert ref_locations")?;

    // ref_sensors (1 par capteur OpenAQ ; parameter_id résolu via parameters.code)
    for (sensor_id, code) in sensors {
        sqlx::query(
            r#"
            INSERT INTO ref_sensors (openaq_sensor_id, ref_location_id, parameter_id)
            SELECT $1, $2, p.id FROM parameters p WHERE p.code = $3
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(sensor_id)
        .bind(ref_location_id)
        .bind(code)
        .execute(pool)
        .await
        .context("upsert ref_sensors")?;
    }

    // tracked_location de l'org + liaison station
    let tl_name = format!("{name} (OpenAQ)");
    let tracked_location_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO tracked_locations (org_id, name, description)
        VALUES ($1, $2, 'Station OpenAQ ingérée automatiquement')
        ON CONFLICT (org_id, name) DO UPDATE SET updated_at = now()
        RETURNING id
        "#,
    )
    .bind(org_id)
    .bind(&tl_name)
    .fetch_one(pool)
    .await
    .context("upsert tracked_locations")?;

    sqlx::query(
        r#"
        INSERT INTO tracked_location_stations (tracked_location_id, ref_location_id, is_primary)
        VALUES ($1, $2, true)
        ON CONFLICT (tracked_location_id, ref_location_id) DO NOTHING
        "#,
    )
    .bind(tracked_location_id)
    .bind(ref_location_id)
    .execute(pool)
    .await
    .context("upsert tracked_location_stations")?;

    println!("✓ Station liée à l'org « {org_slug} » (lieu suivi « {tl_name} »)");
    Ok(())
}
