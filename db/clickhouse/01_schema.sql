-- =============================================================================
-- Quarity — Schéma analytics ClickHouse 24.x  (Jalon 1)
-- =============================================================================
-- Source de vérité des mesures OpenAQ (séries temporelles denses).
-- AUCUNE FK vers Postgres : la corrélation se fait par clé naturelle OpenAQ
-- (location_id = openaq_location_id). Voir la frontière dans docs/data-model.md.
--
-- Décisions appliquées :
--   2. location_id/sensor_id = ENTIERS NATIFS (UInt64), PAS LowCardinality ;
--      LowCardinality réservé à parameter/country/unit (basse cardinalité).
--   3. ReplacingMergeTree(ingested_at) → idempotence (re-ingest API v3 + backfill S3).
--   TTL 90j sur le brut (ttl_only_drop_parts → purge par DROP PARTITION, pas de réécriture).
-- =============================================================================

CREATE DATABASE IF NOT EXISTS quarity;

DROP VIEW  IF EXISTS quarity.mv_measurements_hourly;
DROP VIEW  IF EXISTS quarity.mv_measurements_daily;
DROP TABLE IF EXISTS quarity.measurements_hourly;
DROP TABLE IF EXISTS quarity.measurements_daily;
DROP TABLE IF EXISTS quarity.measurements;

-- -----------------------------------------------------------------------------
-- C.1  Table brute : measurements (ReplacingMergeTree, TTL 90j)
-- -----------------------------------------------------------------------------
CREATE TABLE quarity.measurements
(
    -- Clé naturelle OpenAQ. UInt64 (revue : évite toute troncature vs BIGINT Postgres) + DoubleDelta (IDs triés).
    location_id   UInt64                CODEC(DoubleDelta, ZSTD(1)),
    sensor_id     UInt64                CODEC(DoubleDelta, ZSTD(1)),

    -- Basse cardinalité ⇒ LowCardinality (décision 2)
    parameter     LowCardinality(String),
    country       LowCardinality(String),
    unit          LowCardinality(String),

    measured_at   DateTime64(3, 'UTC')  CODEC(DoubleDelta, ZSTD(1)),

    value         Float64               CODEC(Gorilla, ZSTD(1)),   -- valeurs capteurs lisses ⇒ Gorilla
    latitude      Float64               CODEC(Gorilla, ZSTD(1)),
    longitude     Float64               CODEC(Gorilla, ZSTD(1)),

    -- version ReplacingMergeTree = instant d'ingestion (décision 3)
    ingested_at   DateTime64(3, 'UTC')  DEFAULT now64(3) CODEC(DoubleDelta, ZSTD(1)),

    -- Data-skipping : minmax sur la date (élagage par plage), bloom sur location_id
    -- (utile surtout pour les requêtes multi-locations IN(...) du ranking ; le préfixe d'ORDER BY couvre le `=`).
    INDEX idx_measured_at measured_at TYPE minmax           GRANULARITY 4,
    INDEX idx_location    location_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_parameter   parameter   TYPE set(16)           GRANULARITY 4
)
ENGINE = ReplacingMergeTree(ingested_at)              -- décision 3 : la dernière version ingérée gagne
PARTITION BY toYYYYMM(measured_at)                    -- partition mensuelle (roadmap l.81)
ORDER BY (location_id, parameter, measured_at)        -- décision 3 + roadmap l.82 (location_id en 1re clé)
TTL toDateTime(measured_at) + INTERVAL 90 DAY DELETE  -- purge des bruts > 90j (cast requis : TTL exige Date/DateTime, pas DateTime64)
SETTINGS index_granularity = 8192,
         ttl_only_drop_parts = 1;                     -- revue : purge par DROP PARTITION (zéro réécriture)

-- Lecture de VÉRITÉ (dédupliquée) — toujours via FINAL ou argMax(value, ingested_at) :
--   SELECT location_id, parameter, measured_at, argMax(value, ingested_at) AS value
--   FROM quarity.measurements
--   WHERE location_id = ? AND parameter = ? AND measured_at BETWEEN ? AND ?
--   GROUP BY location_id, parameter, measured_at;

-- -----------------------------------------------------------------------------
-- C.2  Rollups (AggregatingMergeTree) — SimpleAggregateFunction où c'est additif
-- -----------------------------------------------------------------------------
-- Rollup HORAIRE — rétention 2 ans. country retiré (revue : non déterministe hors ORDER BY ;
-- se rejoint via location_id côté app).
CREATE TABLE quarity.measurements_hourly
(
    location_id   UInt64  CODEC(DoubleDelta, ZSTD(1)),
    parameter     LowCardinality(String),
    bucket_hour   DateTime('UTC')  CODEC(DoubleDelta, ZSTD(1)),

    avg_state     AggregateFunction(avg, Float64),                              -- moyenne : état binaire (avgMerge)
    min_value     SimpleAggregateFunction(min, Float64),                        -- min/max : collapse natif au merge
    max_value     SimpleAggregateFunction(max, Float64),
    sample_count  SimpleAggregateFunction(sum, UInt64),                         -- compteur : additif
    last_state    AggregateFunction(argMax, Float64, DateTime64(3, 'UTC'))      -- dernière valeur du bucket (argMaxMerge)
)
ENGINE = AggregatingMergeTree
PARTITION BY toYYYYMM(bucket_hour)
ORDER BY (location_id, parameter, bucket_hour)
TTL bucket_hour + INTERVAL 2 YEAR DELETE
SETTINGS index_granularity = 8192, ttl_only_drop_parts = 1;

-- Rollup JOURNALIER — rétention 5 ans, partition mensuelle (revue : purge plus fine que toYYYY)
CREATE TABLE quarity.measurements_daily
(
    location_id   UInt64  CODEC(DoubleDelta, ZSTD(1)),
    parameter     LowCardinality(String),
    bucket_day    Date    CODEC(DoubleDelta, ZSTD(1)),

    avg_state     AggregateFunction(avg, Float64),
    min_value     SimpleAggregateFunction(min, Float64),
    max_value     SimpleAggregateFunction(max, Float64),
    sample_count  SimpleAggregateFunction(sum, UInt64)
)
ENGINE = AggregatingMergeTree
PARTITION BY toYYYYMM(bucket_day)
ORDER BY (location_id, parameter, bucket_day)
TTL bucket_day + INTERVAL 5 YEAR DELETE
SETTINGS index_granularity = 8192, ttl_only_drop_parts = 1;

-- -----------------------------------------------------------------------------
-- C.3  Materialized Views (alimentation continue depuis les INSERT)
-- -----------------------------------------------------------------------------
CREATE MATERIALIZED VIEW quarity.mv_measurements_hourly
TO quarity.measurements_hourly
AS
SELECT
    location_id,
    parameter,
    toStartOfHour(measured_at)        AS bucket_hour,
    avgState(value)                   AS avg_state,
    min(value)                        AS min_value,
    max(value)                        AS max_value,
    toUInt64(count())                 AS sample_count,
    argMaxState(value, measured_at)   AS last_state
FROM quarity.measurements
GROUP BY location_id, parameter, bucket_hour;

CREATE MATERIALIZED VIEW quarity.mv_measurements_daily
TO quarity.measurements_daily
AS
SELECT
    location_id,
    parameter,
    toDate(measured_at)  AS bucket_day,
    avgState(value)      AS avg_state,
    min(value)           AS min_value,
    max(value)           AS max_value,
    toUInt64(count())    AS sample_count
FROM quarity.measurements
GROUP BY location_id, parameter, bucket_day;

-- Lecture d'un rollup horaire (US-06/US-07) :
--   SELECT location_id, parameter, bucket_hour,
--          avgMerge(avg_state)      AS avg_value,
--          min(min_value)           AS min_value,
--          max(max_value)           AS max_value,
--          sum(sample_count)        AS n,
--          argMaxMerge(last_state)  AS last_value
--   FROM quarity.measurements_hourly
--   WHERE location_id = ? AND parameter = ? AND bucket_hour BETWEEN ? AND ?
--   GROUP BY location_id, parameter, bucket_hour
--   ORDER BY bucket_hour;

-- -----------------------------------------------------------------------------
-- C.4  Idempotence & (re)population après backfill — IMPORTANT
-- -----------------------------------------------------------------------------
-- ⚠ Une MV se déclenche sur le BLOC INSÉRÉ, AVANT la déduplication ReplacingMergeTree.
--   Donc le chemin LIVE peut sur-compter un doublon (même clé ré-ingérée). Règle :
--     • VÉRITÉ de comptage  = `measurements` lue en FINAL / argMax(value, ingested_at).
--     • Rollups LIVE        = approximation (avg/min/max peu sensibles aux rares doublons).
--   Mitigation à l'ingestion : ne ré-insérer que des clés nouvelles, ou activer
--   `deduplicate_blocks_in_dependent_materialized_views = 1` sur les INSERT par blocs identiques.
--
-- Après un BACKFILL S3 d'historique : repeupler explicitement les rollups depuis les bruts
-- DÉDUPLIQUÉS (FINAL), partition par partition, en repartant d'une partition propre :
--   ALTER TABLE quarity.measurements_hourly DROP PARTITION 202604;
--   INSERT INTO quarity.measurements_hourly
--   SELECT location_id, parameter, toStartOfHour(measured_at) AS bucket_hour,
--          avgState(value), min(value), max(value), toUInt64(count()),
--          argMaxState(value, measured_at)
--   FROM quarity.measurements FINAL
--   WHERE toYYYYMM(measured_at) = 202604
--   GROUP BY location_id, parameter, bucket_hour;
-- =============================================================================
