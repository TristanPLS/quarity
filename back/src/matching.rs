//! Boucle de matching de seuils (B7) : mesures ingérées × règles actives → `alert_events`.
//!
//! ## Architecture (cf. roadmap « Pourquoi Moka et Redis »)
//!
//! - **Index compilé en mémoire** : les règles ACTIVES (org vivante, lieu actif)
//!   sont chargées de Postgres et compilées en `HashMap<(station, polluant),
//!   Vec<StationRule>>` — le hot path évalue chaque mesure par lookup mémoire
//!   PUR (aucune E/S) : l'exigence « < 5 ms par mesure » est tenue par
//!   construction (le benchmark formel viendra avec C2).
//! - **Cache Moka L1** ([`RuleCache`]) : l'index est re-chargé au plus toutes
//!   les `MATCHING_RULES_TTL_SECS` (TTL léger — une règle créée/modifiée est
//!   prise en compte au tick suivant l'expiration, jamais plus tard).
//! - **Curseur d'ARRIVÉE, pas de mesure** : la boucle relit ClickHouse par
//!   `ingested_at > watermark` — un backfill qui livre des mesures anciennes
//!   est quand même évalué (c'est l'arrivée qui déclenche, pas l'horodatage).
//! - **Idempotence EN BASE** (migration 0006) : la fenêtre d'ingestion revoit
//!   les mêmes mesures à chaque tick → l'insertion est `ON CONFLICT DO NOTHING`
//!   sur `(alert_rule_id, openaq_sensor_id, measured_at)`. Redémarrage (perte du
//!   watermark), recouvrement de fenêtres, force-check manuel : tous sûrs.
//!
//! ## Isolation multi-tenant
//!
//! `org_id` d'un événement vient de la RÈGLE compilée (jamais d'une entrée
//! externe), et une mesure ne rencontre que les règles dont le lieu suit SA
//! station (clé de l'index). En profondeur, le trigger **T7** (BEFORE INSERT)
//! re-vérifie la cohérence org/lieu de chaque ligne — la boucle a son test
//! e2e cross-tenant ET son test de backstop T7 (`tests/b7_matching.rs`).
//!
//! ## Ce que B7 ne fait PAS (la suite au backlog)
//!
//! - pas de notification : Redis pub/sub + WebSocket = **B8** (l'événement est
//!   le fait persistant ; `notification_deliveries` reste vide à ce stade) ;
//! - pas d'agrégation multi-stations (MAX par polluant — data-model §E) : le
//!   matching est PAR MESURE de chaque station suivie, l'agrégation d'affichage
//!   vit dans les vues/rollups ;
//! - pas de fenêtres réglementaires (moyennes 8 h/24 h — B5/Q1) : les seuils
//!   s'appliquent aux mesures BRUTES (US-02), le matching sur fenêtres viendra
//!   avec les profils d'exposition (B9).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sqlx::PgPool;

use crate::ch::{ClickhouseClient, NewMeasurementRow};
use crate::state::AppState;

// ─────────────────────────────────────────────────────────────────────────────
// Règles compilées
// ─────────────────────────────────────────────────────────────────────────────

/// Comparateur de seuil — miroir STRICT du CHECK `alert_rules.comparator`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparator {
    Gt,
    Ge,
}

impl Comparator {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            ">" => Some(Comparator::Gt),
            ">=" => Some(Comparator::Ge),
            _ => None,
        }
    }

    /// Le dépassement est-il constitué ? (hot path — deux flottants, rien d'autre)
    pub fn holds(self, value: f64, threshold: f64) -> bool {
        match self {
            Comparator::Gt => value > threshold,
            Comparator::Ge => value >= threshold,
        }
    }

    pub fn as_sql(self) -> &'static str {
        match self {
            Comparator::Gt => ">",
            Comparator::Ge => ">=",
        }
    }
}

/// Une règle active « dépliée » sur UNE station de son lieu suivi (une règle
/// couvrant N stations produit N entrées d'index — le snapshot d'événement a
/// besoin de la station déclencheuse exacte, `ref_location_id` compris).
#[derive(Debug, Clone)]
pub struct StationRule {
    pub rule_id: i64,
    pub org_id: i64,
    pub tracked_location_id: i64,
    pub ref_location_id: i64,
    pub openaq_location_id: i64,
    pub parameter: String,
    pub comparator: Comparator,
    pub threshold: f64,
    pub severity: String,
}

/// Ligne de chargement (interne) — `comparator` encore textuel.
#[derive(sqlx::FromRow)]
struct RuleRow {
    rule_id: i64,
    org_id: i64,
    tracked_location_id: i64,
    ref_location_id: i64,
    openaq_location_id: i64,
    parameter: String,
    comparator: String,
    threshold: f64,
    severity: String,
}

/// Index mémoire du hot path : `(station OpenAQ, polluant) → règles concernées`.
#[derive(Debug, Default)]
pub struct RuleIndex {
    map: HashMap<(i64, String), Vec<StationRule>>,
    /// Nombre de règles distinctes (pas d'entrées : une règle × N stations = 1).
    pub rule_count: usize,
}

impl RuleIndex {
    fn from_rows(rows: Vec<RuleRow>) -> Self {
        let mut map: HashMap<(i64, String), Vec<StationRule>> = HashMap::new();
        let mut rule_ids: Vec<i64> = Vec::new();
        for r in rows {
            let Some(comparator) = Comparator::parse(&r.comparator) else {
                // Impossible par CHECK — fail-safe : une règle illisible est
                // IGNORÉE (et tracée) plutôt que de faire tomber la boucle.
                tracing::warn!(rule_id = r.rule_id, comparator = %r.comparator,
                    "règle ignorée : comparateur hors CHECK");
                continue;
            };
            rule_ids.push(r.rule_id);
            map.entry((r.openaq_location_id, r.parameter.clone()))
                .or_default()
                .push(StationRule {
                    rule_id: r.rule_id,
                    org_id: r.org_id,
                    tracked_location_id: r.tracked_location_id,
                    ref_location_id: r.ref_location_id,
                    openaq_location_id: r.openaq_location_id,
                    parameter: r.parameter,
                    comparator,
                    threshold: r.threshold,
                    severity: r.severity,
                });
        }
        rule_ids.sort_unstable();
        rule_ids.dedup();
        RuleIndex {
            map,
            rule_count: rule_ids.len(),
        }
    }

    /// Lookup du hot path — O(1), zéro E/S.
    pub fn lookup(&self, openaq_location_id: i64, parameter: &str) -> &[StationRule] {
        self.map
            .get(&(openaq_location_id, parameter.to_string()))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Périmètre des règles évaluables : ACTIVES, sur un lieu ACTIF, d'une org
/// VIVANTE (le soft-delete d'une org éteint son alerting au tick suivant).
const LOAD_SQL_BASE: &str = r#"
    SELECT ar.id                       AS rule_id,
           ar.org_id                   AS org_id,
           ar.tracked_location_id      AS tracked_location_id,
           rl.id                       AS ref_location_id,
           rl.openaq_location_id       AS openaq_location_id,
           p.code                      AS parameter,
           ar.comparator               AS comparator,
           ar.threshold_value::float8  AS threshold,
           ar.severity                 AS severity
    FROM alert_rules ar
    JOIN parameters p                   ON p.id = ar.parameter_id
    JOIN tracked_locations tl           ON tl.id = ar.tracked_location_id AND tl.is_active
    JOIN tracked_location_stations tls  ON tls.tracked_location_id = tl.id
    JOIN ref_locations rl               ON rl.id = tls.ref_location_id
    JOIN organizations o                ON o.id = ar.org_id AND o.deleted_at IS NULL
    WHERE ar.status = 'active'
"#;

/// Charge et compile TOUTES les règles évaluables (boucle périodique).
pub async fn load_rule_index(pg: &PgPool) -> Result<RuleIndex, sqlx::Error> {
    let rows = sqlx::query_as::<_, RuleRow>(LOAD_SQL_BASE)
        .fetch_all(pg)
        .await?;
    Ok(RuleIndex::from_rows(rows))
}

/// Variante force-check (`POST /api/alert-rules/{id}/run`) : UNE règle, déjà
/// vérifiée appartenir à l'org de l'appelant. Index VIDE = règle inactive,
/// lieu en pause ou org supprimée — au handler de traduire (422).
pub async fn load_rule_index_for_rule(
    pg: &PgPool,
    org_id: i64,
    rule_id: i64,
) -> Result<RuleIndex, sqlx::Error> {
    let sql = format!("{LOAD_SQL_BASE} AND ar.id = $1 AND ar.org_id = $2");
    let rows = sqlx::query_as::<_, RuleRow>(&sql)
        .bind(rule_id)
        .bind(org_id)
        .fetch_all(pg)
        .await?;
    Ok(RuleIndex::from_rows(rows))
}

// ─────────────────────────────────────────────────────────────────────────────
// Cache Moka L1
// ─────────────────────────────────────────────────────────────────────────────

/// Cache L1 in-process de l'index des règles (UNE entrée, TTL court).
///
/// Pourquoi Moka et pas un `RwLock<(Instant, Arc<...>)>` artisanal : le
/// rechargement sous TTL expiré est DÉDUPLIQUÉ entre appelants concurrents
/// (`try_get_with` : un seul recharge, les autres attendent la même valeur) —
/// exactement le contrat « règles compilées, rechargées à intervalle léger »
/// du pitch d'architecture, sans réinventer l'invalidation.
#[derive(Clone)]
pub struct RuleCache {
    cache: moka::future::Cache<(), Arc<RuleIndex>>,
    pg: PgPool,
}

impl RuleCache {
    pub fn new(pg: PgPool, ttl: StdDuration) -> Self {
        let cache = moka::future::Cache::builder()
            .max_capacity(1)
            .time_to_live(ttl)
            .build();
        Self { cache, pg }
    }

    /// L'index courant (rechargé si TTL expiré). Une erreur SQL n'est PAS mise
    /// en cache : le prochain appel retente.
    pub async fn index(&self) -> anyhow::Result<Arc<RuleIndex>> {
        let pg = self.pg.clone();
        self.cache
            .try_get_with((), async move { load_rule_index(&pg).await.map(Arc::new) })
            .await
            .map_err(|e: Arc<sqlx::Error>| anyhow::anyhow!("chargement des règles : {e}"))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Évaluation (hot path) et écriture des événements
// ─────────────────────────────────────────────────────────────────────────────

/// Dépassement constaté : la règle déclenchée + le snapshot de la mesure.
#[derive(Debug)]
pub struct Breach {
    pub rule: StationRule,
    pub openaq_sensor_id: i64,
    pub value: f64,
    pub unit: String,
    pub measured_at: DateTime<Utc>,
}

/// Parse l'horodatage ClickHouse (`YYYY-MM-DD hh:mm:ss[.fff]`, UTC).
fn parse_ch_datetime(s: &str) -> Option<DateTime<Utc>> {
    crate::validation::parse_datetime_ish(s)
        .map(|naive| DateTime::from_naive_utc_and_offset(naive, Utc))
}

/// Évalue un lot de mesures contre l'index. Pure (aucune E/S) : chaque mesure
/// fait UN lookup HashMap puis une comparaison par règle concernée.
/// Une mesure à l'horodatage illisible est ignorée et tracée (fail-safe).
pub fn evaluate(index: &RuleIndex, rows: &[NewMeasurementRow]) -> Vec<Breach> {
    let mut breaches = Vec::new();
    for row in rows {
        let rules = index.lookup(row.location_id as i64, &row.parameter);
        if rules.is_empty() {
            continue;
        }
        // NaN/Inf : IEEE 754 rend `NaN > seuil` FAUX en silence — un capteur qui
        // délirerait ne doit pas passer inaperçu, mais ne doit pas alerter non plus
        // (pas de valeur défendable à snapshotter). Tracé, jamais évalué.
        if !row.value.is_finite() {
            tracing::warn!(location_id = row.location_id, parameter = %row.parameter,
                value = row.value, "mesure ignorée : valeur non finie");
            continue;
        }
        let Some(measured_at) = parse_ch_datetime(&row.measured_at) else {
            tracing::warn!(measured_at = %row.measured_at, location_id = row.location_id,
                "mesure ignorée : horodatage ClickHouse illisible");
            continue;
        };
        for rule in rules {
            if rule.comparator.holds(row.value, rule.threshold) {
                breaches.push(Breach {
                    rule: rule.clone(),
                    openaq_sensor_id: row.sensor_id as i64,
                    value: row.value,
                    unit: row.unit.clone(),
                    measured_at,
                });
            }
        }
    }
    breaches
}

/// Événement RÉELLEMENT inséré, renvoyé par le `RETURNING` de [`insert_events`] :
/// le snapshot figé en base. Sérialisé tel quel dans le push WebSocket (B8) —
/// `org_id` route le message vers le bon canal/tenant.
///
/// `alert_rule_id`/`tracked_location_id`/`openaq_sensor_id` sont nullables en base
/// (FK `ON DELETE SET NULL`) mais TOUJOURS posés par l'INSERT (depuis la règle
/// compilée) — leur décodage en `i64` ne peut donc pas rencontrer de NULL ici.
/// `fired_at` est volontairement laissé au DEFAULT `now()` (instant de persistance).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct InsertedEvent {
    pub id: i64,
    pub alert_rule_id: i64,
    pub org_id: i64,
    pub tracked_location_id: i64,
    pub ref_location_id: i64,
    pub openaq_location_id: i64,
    pub openaq_sensor_id: i64,
    pub parameter_code: String,
    pub measured_value: f64,
    pub unit: String,
    pub measured_at: DateTime<Utc>,
    pub threshold_value: f64,
    pub comparator: String,
    pub severity: String,
    pub fired_at: DateTime<Utc>,
}

/// Insère les événements en UN ordre SQL (UNNEST), idempotent : `ON CONFLICT
/// DO NOTHING` sur la clé de dédup 0006. Le `RETURNING` renvoie EXACTEMENT les
/// événements réellement créés (les rejouages, sautés par ON CONFLICT, ne
/// reviennent pas) — c'est CE jeu, et lui seul, qui alimente le push B8 : publier
/// les `Breach` bruts republierait un dépassement déjà alerté à chaque rejouage.
///
/// Triggers : **T7** (BEFORE) joue pour CHAQUE ligne du lot, conflits compris —
/// un échec (org incohérente) fait échouer TOUT le lot : voulu, un tel mismatch
/// signale un bug de compilation d'index, pas un événement à moitié légitime.
/// **T6** (AFTER, compteur non-lus) ne joue que pour les lignes RÉELLEMENT
/// insérées — une ligne sautée par ON CONFLICT n'incrémente rien (c'est
/// exactement ce qu'on veut : pas de double comptage au rejouage).
pub async fn insert_events(
    pg: &PgPool,
    breaches: &[Breach],
) -> Result<Vec<InsertedEvent>, sqlx::Error> {
    if breaches.is_empty() {
        return Ok(Vec::new());
    }

    let n = breaches.len();
    let mut rule_ids = Vec::with_capacity(n);
    let mut org_ids = Vec::with_capacity(n);
    let mut tl_ids = Vec::with_capacity(n);
    let mut ref_ids = Vec::with_capacity(n);
    let mut loc_ids = Vec::with_capacity(n);
    let mut sensor_ids = Vec::with_capacity(n);
    let mut parameters = Vec::with_capacity(n);
    let mut values = Vec::with_capacity(n);
    let mut units = Vec::with_capacity(n);
    let mut measured_ats = Vec::with_capacity(n);
    let mut thresholds = Vec::with_capacity(n);
    let mut comparators = Vec::with_capacity(n);
    let mut severities = Vec::with_capacity(n);
    for b in breaches {
        rule_ids.push(b.rule.rule_id);
        org_ids.push(b.rule.org_id);
        tl_ids.push(b.rule.tracked_location_id);
        ref_ids.push(b.rule.ref_location_id);
        loc_ids.push(b.rule.openaq_location_id);
        sensor_ids.push(b.openaq_sensor_id);
        parameters.push(b.rule.parameter.clone());
        values.push(b.value);
        units.push(b.unit.clone());
        measured_ats.push(b.measured_at);
        thresholds.push(b.rule.threshold);
        comparators.push(b.rule.comparator.as_sql().to_string());
        severities.push(b.rule.severity.clone());
    }

    let inserted = sqlx::query_as::<_, InsertedEvent>(
        r#"
        INSERT INTO alert_events
            (alert_rule_id, org_id, tracked_location_id, ref_location_id, openaq_location_id,
             openaq_sensor_id, parameter_code, measured_value, unit, measured_at,
             threshold_value, comparator, severity)
        SELECT *
        FROM UNNEST($1::bigint[], $2::bigint[], $3::bigint[], $4::bigint[], $5::bigint[],
                    $6::bigint[], $7::text[], $8::float8[], $9::text[], $10::timestamptz[],
                    $11::float8[], $12::text[], $13::text[])
        ON CONFLICT (alert_rule_id, openaq_sensor_id, measured_at)
            WHERE alert_rule_id IS NOT NULL
        DO NOTHING
        RETURNING id, alert_rule_id, org_id, tracked_location_id, ref_location_id,
                  openaq_location_id, openaq_sensor_id, parameter_code,
                  measured_value::float8 AS measured_value, unit, measured_at,
                  threshold_value::float8 AS threshold_value, comparator, severity, fired_at
        "#,
    )
    .bind(&rule_ids)
    .bind(&org_ids)
    .bind(&tl_ids)
    .bind(&ref_ids)
    .bind(&loc_ids)
    .bind(&sensor_ids)
    .bind(&parameters)
    .bind(&values)
    .bind(&units)
    .bind(&measured_ats)
    .bind(&thresholds)
    .bind(&comparators)
    .bind(&severities)
    .fetch_all(pg)
    .await?;

    Ok(inserted)
}

/// Bilan d'une passe de matching.
#[derive(Debug)]
pub struct MatchOutcome {
    /// Mesures (dédupliquées) relues depuis le curseur.
    pub evaluated: usize,
    /// Dépassements constatés (avant dédup en base).
    pub breaches: usize,
    /// Nombre d'événements RÉELLEMENT insérés (idempotence : rejouage ⇒ 0) —
    /// `inserted_events.len()`.
    pub inserted: u64,
    /// Les événements réellement insérés (jeu `RETURNING`) — alimentent le push
    /// B8 (Redis pub/sub → WebSocket). Vide au rejouage.
    pub inserted_events: Vec<InsertedEvent>,
    /// Nouveau curseur (max `ingested_at` lu) — `None` si aucune mesure.
    pub watermark: Option<DateTime<Utc>>,
}

/// Format d'horodatage attendu par `parseDateTime64BestEffort(…, 3, 'UTC')`.
fn fmt_ch(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

/// UNE passe complète : relit les arrivées ClickHouse depuis `since`, évalue
/// contre `index`, écrit les événements (idempotent). N'avance AUCUN état :
/// le curseur retourné est à persister par l'appelant (boucle ou force-check).
pub async fn run_once(
    pg: &PgPool,
    ch: &ClickhouseClient,
    index: &RuleIndex,
    since: DateTime<Utc>,
    limit: u32,
) -> anyhow::Result<MatchOutcome> {
    let rows = ch
        .query_new_measurements(&fmt_ch(since), limit)
        .await
        .map_err(|e| anyhow::anyhow!("lecture des nouvelles mesures : {e}"))?;

    // Curseur = max ingested_at LU. Cas TRONQUÉ (rows == limit) : l'ingestion
    // écrit ses lots avec un MÊME ingested_at — la coupe peut tomber AU MILIEU
    // d'un groupe d'égalité, et le curseur strictement `>` abandonnerait le
    // reste du groupe. On recule alors d'1 ms : le groupe entier est relu au
    // tick suivant (sans doublon — dédup 0006). Résiduel assumé : un groupe
    // d'égalité PLUS GRAND que `limit` ne progresse que par recouvrements
    // successifs (tracé en warn, MATCHING_BATCH_LIMIT à augmenter).
    let truncated = rows.len() as u64 >= u64::from(limit);
    let mut watermark = rows
        .iter()
        .filter_map(|r| parse_ch_datetime(&r.last_ingested_at))
        .max();
    if truncated {
        tracing::warn!(
            limit,
            "matching : lot tronqué — curseur reculé d'1 ms pour relire le groupe d'égalité"
        );
        watermark = watermark.map(|wm| wm - Duration::milliseconds(1));
    }
    if !rows.is_empty() && watermark.is_none() {
        // Toutes les arrivées illisibles : ne JAMAIS avancer en aveugle, mais
        // le signaler fort — un tel tick se rejouerait indéfiniment.
        tracing::warn!("matching : aucun ingested_at lisible dans le lot — curseur inchangé");
    }

    let breaches = evaluate(index, &rows);
    let inserted_events = insert_events(pg, &breaches).await?;

    Ok(MatchOutcome {
        evaluated: rows.len(),
        breaches: breaches.len(),
        inserted: inserted_events.len() as u64,
        inserted_events,
        watermark,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Boucle périodique (spawnée par main.rs si MATCHING_INTERVAL_SECS > 0)
// ─────────────────────────────────────────────────────────────────────────────

/// Boucle de matching : un tick toutes les `MATCHING_INTERVAL_SECS`.
///
/// Résilience (même pattern que le scheduler d'ingestion A6a) : un tick en
/// échec est tracé et RETENTÉ au tick suivant — le curseur n'avance pas sur
/// erreur, donc rien n'est perdu (et rien n'est dupliqué : dédup 0006).
/// Index vide (aucune règle évaluable) : pas de lecture ClickHouse, et le
/// curseur SAUTE à maintenant — une règle créée plus tard ne déclenche que sur
/// les mesures arrivées APRÈS sa prise en compte (pas d'alertes rétroactives
/// sur un backlog antérieur à la règle).
pub async fn run_loop(state: AppState) {
    let interval_secs = state.cfg.matching_interval_secs;
    let cache = RuleCache::new(
        state.pg.clone(),
        StdDuration::from_secs(state.cfg.matching_rules_ttl_secs),
    );
    // Connexion Redis (clonée) pour publier les alertes B8 sur le canal d'org —
    // best-effort, séparé du chemin d'insertion (le fait persistant est en base).
    let redis = state.redis.clone();
    // Arrêt gracieux (B8b) : partagé via AppState — annulé par `main` à l'extinction.
    let shutdown = state.shutdown.clone();
    let mut watermark = Utc::now() - Duration::seconds(state.cfg.matching_lookback_secs as i64);

    let mut ticker = tokio::time::interval(StdDuration::from_secs(interval_secs));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        // Attend le prochain tick OU l'arrêt — un arrêt demandé pendant l'attente
        // interrompt la boucle sans démarrer un tick partiel.
        tokio::select! {
            _ = shutdown.cancelled() => {
                tracing::info!("boucle de matching arrêtée (arrêt gracieux)");
                break;
            }
            _ = ticker.tick() => {}
        }

        let index = match cache.index().await {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!(error = %e, "matching : chargement des règles en échec — tick sauté");
                continue;
            }
        };
        if index.is_empty() {
            tracing::debug!("matching : aucune règle évaluable — curseur avancé");
            watermark = Utc::now();
            continue;
        }

        match run_once(
            &state.pg,
            &state.ch,
            &index,
            watermark,
            state.cfg.matching_batch_limit,
        )
        .await
        {
            Ok(outcome) => {
                if let Some(wm) = outcome.watermark {
                    watermark = wm;
                }
                // Push temps réel B8 : publier les events RÉELLEMENT créés (jamais
                // au rejouage — `inserted_events` est alors vide). Best-effort.
                if !outcome.inserted_events.is_empty() {
                    crate::alerts::publish_alert_events(&redis, &outcome.inserted_events).await;
                }
                if outcome.inserted > 0 {
                    tracing::info!(
                        rules = index.rule_count,
                        evaluated = outcome.evaluated,
                        breaches = outcome.breaches,
                        inserted = outcome.inserted,
                        "matching : événements d'alerte créés"
                    );
                } else {
                    tracing::debug!(
                        rules = index.rule_count,
                        evaluated = outcome.evaluated,
                        "matching : tick sans nouvel événement"
                    );
                }
            }
            Err(e) => {
                // Curseur INCHANGÉ : le tick suivant rejouera la même fenêtre.
                tracing::warn!(error = %e, "matching : tick en échec — sera retenté");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ch::NewMeasurementRow;

    fn rule(comparator: Comparator, threshold: f64) -> StationRule {
        StationRule {
            rule_id: 1,
            org_id: 10,
            tracked_location_id: 100,
            ref_location_id: 1000,
            openaq_location_id: 4085,
            parameter: "pm25".into(),
            comparator,
            threshold,
            severity: "warning".into(),
        }
    }

    fn measurement(location_id: u64, parameter: &str, value: f64) -> NewMeasurementRow {
        NewMeasurementRow {
            location_id,
            sensor_id: 7,
            parameter: parameter.into(),
            unit: "µg/m³".into(),
            measured_at: "2026-06-09 12:00:00.000".into(),
            value,
            last_ingested_at: "2026-06-09 12:05:00.000".into(),
        }
    }

    fn index_with(rules: Vec<StationRule>) -> RuleIndex {
        let mut idx = RuleIndex::default();
        for r in rules {
            idx.map
                .entry((r.openaq_location_id, r.parameter.clone()))
                .or_default()
                .push(r);
        }
        idx.rule_count = 1;
        idx
    }

    #[test]
    fn comparator_boundaries() {
        // `>` : l'égalité ne déclenche PAS ; `>=` : elle déclenche (US-02 c1).
        assert!(!Comparator::Gt.holds(15.0, 15.0));
        assert!(Comparator::Gt.holds(15.01, 15.0));
        assert!(Comparator::Ge.holds(15.0, 15.0));
        assert!(!Comparator::Ge.holds(14.99, 15.0));
        assert_eq!(Comparator::parse(">="), Some(Comparator::Ge));
        assert_eq!(Comparator::parse("<"), None);
    }

    #[test]
    fn evaluate_matches_only_indexed_station_and_parameter() {
        let idx = index_with(vec![rule(Comparator::Gt, 15.0)]);

        // Bonne station + bon polluant + dépassement → 1 breach.
        let b = evaluate(&idx, &[measurement(4085, "pm25", 20.0)]);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].rule.rule_id, 1);
        assert_eq!(b[0].value, 20.0);

        // Sous le seuil → rien.
        assert!(evaluate(&idx, &[measurement(4085, "pm25", 10.0)]).is_empty());
        // Station étrangère à l'index (cross-tenant au niveau boucle) → rien.
        assert!(evaluate(&idx, &[measurement(9999, "pm25", 500.0)]).is_empty());
        // Autre polluant → rien.
        assert!(evaluate(&idx, &[measurement(4085, "no2", 500.0)]).is_empty());
    }

    #[test]
    fn evaluate_skips_unparsable_timestamp() {
        let idx = index_with(vec![rule(Comparator::Gt, 15.0)]);
        let mut m = measurement(4085, "pm25", 20.0);
        m.measured_at = "n'importe quoi".into();
        assert!(evaluate(&idx, &[m]).is_empty());
    }

    #[test]
    fn ch_datetime_roundtrip() {
        let dt = parse_ch_datetime("2026-06-09 12:00:00.000").expect("parse");
        assert_eq!(fmt_ch(dt), "2026-06-09 12:00:00.000");
    }
}
