//! `GET /api/aqi` (B10) — AQI US EPA courant des lieux suivis de l'org de l'appelant.
//!
//! Tranche verticale de la jauge AQI du dashboard : pour CHAQUE lieu suivi ACTIF de
//! l'org du JWT, l'AQI instantané est calculé en réutilisant la requête **B5 Q2**
//! (`aqi_snapshot`, ClickHouse — barème EPA, conversion ppb figée D4.2, troncature +
//! interpolation). Aucun calcul d'AQI côté Rust/JS : la source de vérité reste Q2.
//!
//! - **Isolation multi-tenant** : la requête Postgres est scopée `tl.org_id = <org du
//!   JWT>` — un lieu d'une autre org n'apparaît jamais.
//! - **Multi-stations** (data-model §D.5) : un lieu agrège ses stations par **MAX par
//!   polluant** (principe de précaution sanitaire) ; l'AQI global du lieu = MAX des
//!   AQI par polluant (convention EPA — polluant dominant).
//! - **Best-effort** : une station dont la lecture ClickHouse échoue est tracée et
//!   traitée comme « sans donnée » — elle n'efface pas le reste du tableau de bord.
//! - **Pas de donnée récente** : un lieu sans bucket dans les fenêtres sort quand même
//!   (`has_data = false`, `overall_aqi = null`) — le front distingue « pas de donnée »
//!   d'un AQI bas.

use std::collections::HashMap;

use axum::extract::State;
use axum::Json;
use chrono::{SecondsFormat, Utc};
use futures_util::stream::StreamExt;
use serde::Serialize;
use utoipa::ToSchema;

use crate::ch::AqiSnapshotRow;
use crate::error::AppError;
use crate::security::AuthUser;
use crate::state::AppState;

/// Concurrence MAX des requêtes ClickHouse Q2 d'une vue d'ensemble AQI (durcissement) :
/// un org à N lieux suivis ne déclenche PAS N requêtes ClickHouse d'un coup — au plus
/// ce nombre en vol, le reste s'écoule au fur et à mesure (anti-amplification de coût).
const AQI_MAX_CONCURRENT_QUERIES: usize = 16;

/// AQI d'un polluant pour un lieu (meilleure — MAX — de ses stations).
#[derive(Debug, Serialize, ToSchema)]
pub struct AqiPollutant {
    #[schema(example = "pm25")]
    pub parameter: String,
    /// AQI entier (arrondi EPA), 0..=500.
    #[schema(example = 71)]
    pub aqi: i32,
    /// Concentration glissante convertie (ppb pour o3/no2, µg/m³ sinon).
    pub conc_value: f64,
    #[schema(example = "µg/m³")]
    pub conc_unit: String,
    /// Validité réglementaire (couverture de la fenêtre ≥ 75 %).
    pub is_valid: bool,
    /// Porte l'AQI global du lieu (ex æquo tous marqués).
    pub is_dominant: bool,
}

/// AQI courant d'un lieu suivi.
#[derive(Debug, Serialize, ToSchema)]
pub struct LocationAqi {
    pub tracked_location_id: i64,
    #[schema(example = "Écoles du centre-ville")]
    pub name: String,
    /// Coordonnées de la 1ʳᵉ station (pour le marqueur carte — B10b). `null` si absentes.
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// AQI global = MAX des AQI par polluant/station (précaution). `null` si aucune donnée.
    #[schema(example = 71)]
    pub overall_aqi: Option<i32>,
    /// Polluant portant l'AQI global. `null` si aucune donnée.
    #[schema(example = "pm25")]
    pub dominant_parameter: Option<String>,
    /// Validité réglementaire du polluant dominant (≥ 75 % de couverture).
    pub valid: bool,
    /// `false` = aucune mesure récente dans les fenêtres (≠ AQI bas).
    pub has_data: bool,
    /// Détail par polluant (tri AQI décroissant).
    pub pollutants: Vec<AqiPollutant>,
}

/// Vue d'ensemble AQI : un élément par lieu suivi actif de l'org.
#[derive(Debug, Serialize, ToSchema)]
pub struct AqiOverview {
    /// Instant de calcul (UTC, RFC 3339).
    pub computed_at: String,
    pub data: Vec<LocationAqi>,
}

/// Regroupement interne d'un lieu (ordre d'apparition préservé).
struct Loc {
    name: String,
    lat: Option<f64>,
    lon: Option<f64>,
    stations: Vec<i64>,
}

/// Ligne brute lieux × stations : (tracked_location_id, nom, openaq_location_id, lat, lon).
type StationRow = (i64, String, i64, Option<f64>, Option<f64>);

/// AQI courant des lieux suivis actifs de l'org de l'appelant.
#[utoipa::path(
    get,
    path = "/api/aqi",
    tag = "aqi",
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "AQI courant par lieu suivi (vide si l'org n'a aucun lieu actif)", body = AqiOverview),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
    )
)]
pub async fn overview(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<AqiOverview>, AppError> {
    Ok(Json(overview_for_org(&state, user.org_id).await?))
}

/// Cœur de la vue d'ensemble AQI d'une org — partagé entre `/api/aqi` (JWT) et
/// `/api/public/aqi` (clé API, B9b-2). `org_id` provient TOUJOURS de l'auth (isolation).
pub async fn overview_for_org(state: &AppState, org_id: i64) -> Result<AqiOverview, AppError> {
    // 1. Lieux ACTIFS de l'org + leurs stations + coords — SCOPÉ org (isolation).
    let rows: Vec<StationRow> = sqlx::query_as(
        r#"
        SELECT tl.id, tl.name, rl.openaq_location_id, rl.latitude::float8, rl.longitude::float8
        FROM tracked_locations tl
        JOIN tracked_location_stations tls ON tls.tracked_location_id = tl.id
        JOIN ref_locations rl              ON rl.id = tls.ref_location_id
        WHERE tl.org_id = $1 AND tl.is_active
        ORDER BY tl.id, rl.openaq_location_id
        "#,
    )
    .bind(org_id)
    .fetch_all(&state.pg)
    .await?;

    // Regroupe par lieu (ordre préservé via `order`) ; collecte les stations DISTINCTES.
    let mut order: Vec<i64> = Vec::new();
    let mut locs: HashMap<i64, Loc> = HashMap::new();
    let mut stations: Vec<i64> = Vec::new();
    for (tl_id, name, openaq, lat, lon) in rows {
        let loc = locs.entry(tl_id).or_insert_with(|| {
            order.push(tl_id);
            Loc {
                name,
                lat,
                lon,
                stations: Vec::new(),
            }
        });
        loc.stations.push(openaq);
        if !stations.contains(&openaq) {
            stations.push(openaq);
        }
    }

    // 2. Q2 (aqi_snapshot) par station DISTINCTE, en parallèle BORNÉE à l'heure courante :
    //    au plus AQI_MAX_CONCURRENT_QUERIES requêtes ClickHouse en vol (l'ordre des
    //    résultats n'importe pas — l'agrégation §3 indexe par station).
    let now = Utc::now();
    let at = now.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
    let snapshots: Vec<_> = futures_util::stream::iter(stations.into_iter().map(|st| {
        let ch = state.ch.clone();
        let at = at.clone();
        async move { (st, ch.query_aqi_snapshot(st as u64, &at).await) }
    }))
    .buffer_unordered(AQI_MAX_CONCURRENT_QUERIES)
    .collect()
    .await;

    let mut by_station: HashMap<i64, Vec<AqiSnapshotRow>> = HashMap::new();
    for (st, res) in snapshots {
        match res {
            Ok(rows) => {
                by_station.insert(st, rows);
            }
            Err(e) => {
                tracing::warn!(station = st, error = %e,
                    "AQI : lecture ClickHouse d'une station échouée — lieu traité sans cette station");
                by_station.insert(st, Vec::new());
            }
        }
    }

    // 3. Agrégation par lieu : MAX par polluant à travers ses stations, puis MAX global.
    let mut data = Vec::with_capacity(order.len());
    for tl_id in order {
        let loc = locs.remove(&tl_id).expect("lieu regroupé");
        let mut best: HashMap<String, &AqiSnapshotRow> = HashMap::new();
        for st in &loc.stations {
            for row in by_station.get(st).map(Vec::as_slice).unwrap_or(&[]) {
                best.entry(row.parameter.clone())
                    .and_modify(|cur| {
                        if row.aqi > cur.aqi {
                            *cur = row;
                        }
                    })
                    .or_insert(row);
            }
        }

        let overall = best.values().map(|r| r.aqi).max();
        let dominant = overall.and_then(|max| {
            best.values()
                .find(|r| r.aqi == max)
                .map(|r| r.parameter.clone())
        });
        // Valide si le polluant dominant a une couverture suffisante.
        let valid = match overall {
            Some(max) => best.values().any(|r| r.aqi == max && r.is_valid != 0),
            None => false,
        };

        let mut pollutants: Vec<AqiPollutant> = best
            .values()
            .map(|r| AqiPollutant {
                parameter: r.parameter.clone(),
                aqi: r.aqi,
                conc_value: r.conc_value,
                conc_unit: r.conc_unit.clone(),
                is_valid: r.is_valid != 0,
                is_dominant: Some(r.aqi) == overall,
            })
            .collect();
        pollutants.sort_by(|a, b| {
            b.aqi
                .cmp(&a.aqi)
                .then_with(|| a.parameter.cmp(&b.parameter))
        });

        data.push(LocationAqi {
            tracked_location_id: tl_id,
            name: loc.name,
            latitude: loc.lat,
            longitude: loc.lon,
            overall_aqi: overall,
            dominant_parameter: dominant,
            valid,
            has_data: !pollutants.is_empty(),
            pollutants,
        });
    }

    Ok(AqiOverview {
        computed_at: now.to_rfc3339_opts(SecondsFormat::Secs, true),
        data,
    })
}
