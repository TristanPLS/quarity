//! Lecture `/api/alert-events` (4b) — historique des alertes d'une organisation.
//!
//! **Backfill du panneau temps réel** (B11a) : au montage, le front charge les N
//! dernières alertes via cet endpoint, puis le flux WebSocket B8 (`/api/ws`) ajoute
//! les nouvelles en tête (dédup par `id`). Sans ce backfill, un hard-refresh laissait
//! le panneau vide jusqu'au prochain push — la limite assumée de B11a, refermée ici.
//!
//! Conventions (mêmes que le gabarit `alert_rules`) :
//! - **Isolation multi-tenant par construction** : la requête est filtrée
//!   `org_id = <org du JWT>` (colonne portée par `alert_events`, cohérence garantie
//!   par le trigger **T7**) — un événement d'une autre org est invisible.
//! - **Lecture seule** : les événements sont écrits par la boucle de matching (B7) /
//!   le force-check, jamais par cet endpoint → [`AuthUser`] suffit (pas de `CanWrite`).
//! - **Colonnes nullables** : `alert_rule_id`, `tracked_location_id` et
//!   `openaq_sensor_id` deviennent NULL quand leur référent est supprimé
//!   (FK `ON DELETE SET NULL`, snapshot immuable T4) → `Option<>` côté Rust,
//!   `| null` côté front. NUMERIC SELECTé `::float8` (pas de map direct `f64`).
//! - **Tri** : par allowlist (défaut `fired_at` décroissant), clé secondaire `id`
//!   pour une pagination déterministe.

use axum::extract::State;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;

use crate::error::AppError;
use crate::listing::ListParams;
use crate::security::AuthUser;
use crate::state::AppState;
use crate::validation::ValidatedQuery;

// ─────────────────────────────────────────────────────────────────────────────
// DTOs
// ─────────────────────────────────────────────────────────────────────────────

/// Événement d'alerte (snapshot figé au moment du dépassement). Mêmes champs que le
/// message WebSocket B8 ([`crate::matching::InsertedEvent`]) — le front déduplique les
/// deux flux par `id` —, MOINS `ref_location_id` (surrogate interne, non exposé), et
/// AVEC la nullabilité des FK historisées (un référent supprimé → `null`).
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct AlertEventDto {
    pub id: i64,
    /// Règle déclenchante — `null` si elle a été supprimée depuis (FK `SET NULL`).
    pub alert_rule_id: Option<i64>,
    /// Org propriétaire — toujours celle du JWT (les autres sont invisibles).
    pub org_id: i64,
    /// Lieu suivi — `null` si supprimé depuis (FK `SET NULL`).
    pub tracked_location_id: Option<i64>,
    /// Clé naturelle OpenAQ figée (cohérente avec `measurements.location_id`).
    pub openaq_location_id: i64,
    /// Capteur déclencheur figé — `null` si non renseigné à l'écriture.
    pub openaq_sensor_id: Option<i64>,
    #[schema(example = "pm25")]
    pub parameter_code: String,
    /// SELECTé `::float8` : NUMERIC ne se mappe pas sur `f64` sans cast.
    #[schema(example = 42.5)]
    pub measured_value: f64,
    pub unit: String,
    pub measured_at: DateTime<Utc>,
    /// Seuil franchi, dans l'unité du polluant. SELECTé `::float8` (NUMERIC → f64).
    #[schema(example = 15.0)]
    pub threshold_value: f64,
    #[schema(example = ">")]
    pub comparator: String,
    #[schema(example = "warning")]
    pub severity: String,
    pub fired_at: DateTime<Utc>,
}

/// Page d'événements d'alerte (`{ page, page_size, count, total, data }`) —
/// homogène avec les listings CRUD (B6).
#[derive(Debug, Serialize, ToSchema)]
pub struct AlertEventsPage {
    pub page: u32,
    pub page_size: u32,
    /// Taille de `data`.
    pub count: usize,
    /// Total filtré (toutes pages confondues).
    pub total: i64,
    pub data: Vec<AlertEventDto>,
}

// ─────────────────────────────────────────────────────────────────────────────
// SQL (scopé org_id — JAMAIS de requête sans le filtre)
// ─────────────────────────────────────────────────────────────────────────────

/// Colonnes du DTO — NUMERIC castés `::float8` (NUMERIC ne se mappe pas sur `f64`).
const DTO_COLUMNS: &str = r#"
    id, alert_rule_id, org_id, tracked_location_id,
    openaq_location_id, openaq_sensor_id, parameter_code,
    measured_value::float8 AS measured_value, unit, measured_at,
    threshold_value::float8 AS threshold_value, comparator, severity, fired_at
"#;

/// Allowlist de tri : `nom_api → colonne SQL` (le défaut `-fired_at` couvre le
/// backfill « les plus récentes d'abord »).
const SORT_ALLOW: &[(&str, &str)] = &[
    ("fired_at", "fired_at"),
    ("measured_at", "measured_at"),
    ("severity", "severity"),
];

// ─────────────────────────────────────────────────────────────────────────────
// Handler
// ─────────────────────────────────────────────────────────────────────────────

/// Liste paginée des événements d'alerte de l'org de l'appelant (les plus récents
/// d'abord par défaut) — backfill du panneau temps réel (B11a).
#[utoipa::path(
    get,
    path = "/api/alert-events",
    tag = "alert-events",
    params(ListParams),
    security(("bearer_jwt" = [])),
    responses(
        (status = 200, description = "Page d'événements (tri par défaut : `fired_at` décroissant)", body = AlertEventsPage),
        (status = 400, description = "Pagination/tri invalide (corps `ErrorBody`, ou rejet `Query` en corps texte)", body = crate::openapi::ErrorBody),
        (status = 401, description = "Bearer manquant, invalide ou expiré", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Json<AlertEventsPage>, AppError> {
    let p = params.pagination();
    let order_by = params.order_by(SORT_ALLOW, "-fired_at")?;

    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM alert_events WHERE org_id = $1")
        .bind(user.org_id)
        .fetch_one(&state.pg)
        .await?;

    let data = sqlx::query_as::<_, AlertEventDto>(&format!(
        "SELECT {DTO_COLUMNS} FROM alert_events WHERE org_id = $1 {order_by} LIMIT $2 OFFSET $3"
    ))
    .bind(user.org_id)
    .bind(p.limit)
    .bind(p.offset)
    .fetch_all(&state.pg)
    .await?;

    Ok(Json(AlertEventsPage {
        page: p.page,
        page_size: p.page_size,
        count: data.len(),
        total,
        data,
    }))
}
