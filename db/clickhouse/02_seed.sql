-- =============================================================================
-- Quarity — Seed de démonstration ClickHouse  (Jalon 1)
-- =============================================================================
-- Mesures horaires sur les stations suivies, avec des valeurs AU-DESSUS et
-- EN-DESSOUS des seuils (matching, AQI, moyennes glissantes, dose). Inclut un
-- DOUBLON (même clé, ingested_at différent) pour prouver la déduplication
-- ReplacingMergeTree. Prérequis : 01_schema.sql appliqué.
--
-- NB ClickHouse : le format VALUES n'accepte PAS de commentaires `--` ENTRE les
-- tuples → les annotations sont placées AVANT chaque INSERT, jamais dans les VALUES.
--
-- Convention sensor_id (miroir du seed Postgres) : openaq_location_id*10 + parameter_id
--   parameter_id : pm25=1, pm10=2, no2=3, o3=4   →  ex 1001/pm25 = 10011
-- =============================================================================

-- Nice Promenade (1001) — PM2.5 : 09:00 (22.5) et 28/05 08:00 (31.0) franchissent le seuil enfants (>15).
INSERT INTO quarity.measurements
    (location_id, sensor_id, parameter, country, unit, measured_at, value, latitude, longitude, ingested_at) VALUES
    (1001, 10011, 'pm25', 'FR', 'µg/m³', '2026-05-12 07:00:00.000', 12.0, 43.6951, 7.2659, '2026-05-12 07:05:00.000'),
    (1001, 10011, 'pm25', 'FR', 'µg/m³', '2026-05-12 08:00:00.000', 18.4, 43.6951, 7.2659, '2026-05-12 08:05:00.000'),
    (1001, 10011, 'pm25', 'FR', 'µg/m³', '2026-05-12 09:00:00.000', 22.5, 43.6951, 7.2659, '2026-05-12 09:05:00.000'),
    (1001, 10011, 'pm25', 'FR', 'µg/m³', '2026-05-12 10:00:00.000', 14.1, 43.6951, 7.2659, '2026-05-12 10:05:00.000'),
    (1001, 10011, 'pm25', 'FR', 'µg/m³', '2026-05-28 08:00:00.000', 31.0, 43.6951, 7.2659, '2026-05-28 08:05:00.000');

-- Nice Promenade (1001) — O3 (µg/m³ brut OpenAQ).
INSERT INTO quarity.measurements
    (location_id, sensor_id, parameter, country, unit, measured_at, value, latitude, longitude, ingested_at) VALUES
    (1001, 10014, 'o3', 'FR', 'µg/m³', '2026-05-12 14:00:00.000', 95.0, 43.6951, 7.2659, '2026-05-12 14:05:00.000');

-- Nice Centre (1002) — NO2 : 18:00 (52.0) franchit le seuil 40 (>=) le 2026-04-03.
INSERT INTO quarity.measurements
    (location_id, sensor_id, parameter, country, unit, measured_at, value, latitude, longitude, ingested_at) VALUES
    (1002, 10023, 'no2', 'FR', 'µg/m³', '2026-04-03 17:00:00.000', 38.0, 43.7010, 7.2680, '2026-04-03 17:05:00.000'),
    (1002, 10023, 'no2', 'FR', 'µg/m³', '2026-04-03 18:00:00.000', 52.0, 43.7010, 7.2680, '2026-04-03 18:05:00.000');

-- Lyon Part-Dieu (2001) — PM2.5 : 40.0 et 28.0 franchissent 25.
INSERT INTO quarity.measurements
    (location_id, sensor_id, parameter, country, unit, measured_at, value, latitude, longitude, ingested_at) VALUES
    (2001, 20011, 'pm25', 'FR', 'µg/m³', '2026-05-20 14:00:00.000', 40.0, 45.7606, 4.8579, '2026-05-20 14:05:00.000'),
    (2001, 20011, 'pm25', 'FR', 'µg/m³', '2026-06-01 07:00:00.000', 28.0, 45.7606, 4.8579, '2026-06-01 07:05:00.000');

-- Antwerpen (3001) — PM2.5 : 33.0 franchit 25.
INSERT INTO quarity.measurements
    (location_id, sensor_id, parameter, country, unit, measured_at, value, latitude, longitude, ingested_at) VALUES
    (3001, 30011, 'pm25', 'BE', 'µg/m³', '2026-05-15 11:00:00.000', 33.0, 51.2194, 4.4025, '2026-05-15 11:05:00.000');

-- DOUBLON volontaire : même clé (1001, pm25, 09:00) ré-ingéré avec une valeur corrigée (22.9)
-- et un ingested_at PLUS RÉCENT → ReplacingMergeTree(ingested_at) gardera 22.9 (lecture FINAL).
INSERT INTO quarity.measurements
    (location_id, sensor_id, parameter, country, unit, measured_at, value, latitude, longitude, ingested_at) VALUES
    (1001, 10011, 'pm25', 'FR', 'µg/m³', '2026-05-12 09:00:00.000', 22.9, 43.6951, 7.2659, '2026-05-12 12:30:00.000');

-- Vérifs (décommenter) :
--   SELECT count() FROM quarity.measurements;          -- 12 lignes brutes (dont le doublon)
--   SELECT count() FROM quarity.measurements FINAL;    -- 11 après déduplication
--   SELECT value FROM quarity.measurements FINAL
--     WHERE location_id=1001 AND parameter='pm25' AND measured_at='2026-05-12 09:00:00.000';  -- 22.9
