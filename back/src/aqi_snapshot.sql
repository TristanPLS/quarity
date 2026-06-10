-- ===== Q2 : aqi_snapshot =====
-- AQI US EPA instantané : fenêtres closes à la FIN du bucket horaire de {at}
-- (anchor_hour = heure pleine de {at}). TOUT le bucket [anchor_hour,
-- anchor_hour + 1 h[ compte, y compris les mesures de la même heure
-- POSTÉRIEURES à {at} : ce n'est pas un instantané strict à {at} (cf. test
-- q2_o3_converts_to_ppb_then_interpolates, {at} = h + 30 min).
-- Paramètres serveur : {loc:UInt64}, {at:String} (query-string : param_loc, param_at).
-- Pour chaque polluant à breakpoints (pm25, pm10, o3, no2) ayant AU MOINS un
-- bucket horaire dans SA fenêtre (24/24/8/1 h) :
--   parameter, window_end (heure d'ancrage), window_hours, hours_present,
--   coverage (0..1), is_valid (≥ 0.75), rolling_avg_ugm3 (µg/m³),
--   conc_value + conc_unit (concentration convertie : ppb pour o3/no2, µg/m³ sinon),
--   aqi (entier, arrondi EPA floor(x+0.5)),
--   is_dominant (aqi == max des aqi — convention EPA : l'AQI global du lieu est
--   le MAX des AQI par polluant ; les ex æquo sont tous marqués dominants).
-- Un polluant sans aucun bucket dans sa fenêtre ne produit pas de ligne.
WITH
    -- Heure d'ancrage = fin de fenêtre, arrondie à l'heure pleine de {at}.
    toStartOfHour(toDateTime(parseDateTime64BestEffort({at:String}, 3, 'UTC'), 'UTC')) AS anchor_hour,
    -- MIROIR VERBATIM des paliers de db/sql/02_seed.sql (source de vérité
    -- Postgres) — 24 lignes : 6 catégories × 4 polluants. Paliers pm25/pm10 en
    -- µg/m³, o3/no2 en ppb (d'où la conversion plus bas).
    breakpoints AS
    (
        SELECT parameter, conc_low, conc_high, aqi_low, aqi_high
        FROM VALUES(
            'parameter String, conc_low Float64, conc_high Float64, aqi_low Float64, aqi_high Float64',
            -- PM2.5 / 24 h (µg/m³, barème EPA 2024)
            ('pm25',   0.0,    9.0,    0,  50),
            ('pm25',   9.1,   35.4,   51, 100),
            ('pm25',  35.5,   55.4,  101, 150),
            ('pm25',  55.5,  125.4,  151, 200),
            ('pm25', 125.5,  225.4,  201, 300),
            ('pm25', 225.5,  325.4,  301, 500),
            -- PM10 / 24 h (µg/m³)
            ('pm10',   0,   54,    0,  50),
            ('pm10',  55,  154,   51, 100),
            ('pm10', 155,  254,  101, 150),
            ('pm10', 255,  354,  151, 200),
            ('pm10', 355,  424,  201, 300),
            ('pm10', 425,  604,  301, 500),
            -- O3 / 8 h (ppb — cat. 5/6 sur 8 h : approximation démo, comme le seed)
            ('o3',     0,   54,    0,  50),
            ('o3',    55,   70,   51, 100),
            ('o3',    71,   85,  101, 150),
            ('o3',    86,  105,  151, 200),
            ('o3',   106,  200,  201, 300),
            ('o3',   201,  504,  301, 500),
            -- NO2 / 1 h (ppb)
            ('no2',    0,   53,    0,  50),
            ('no2',   54,  100,   51, 100),
            ('no2',  101,  360,  101, 150),
            ('no2',  361,  649,  151, 200),
            ('no2',  650, 1249,  201, 300),
            ('no2', 1250, 2049,  301, 500)
        )
    ),
    -- Fenêtre réglementaire par polluant (US-07).
    win AS
    (
        SELECT parameter, window_hours
        FROM VALUES('parameter String, window_hours UInt32',
                    ('pm25', 24), ('pm10', 24), ('o3', 8), ('no2', 1))
    ),
    -- Borne haute du barème par polluant : la concentration tronquée y est
    -- clampée AVANT le join ⇒ le palier max matche toujours et l'AQI plafonne à 500.
    caps AS
    (
        SELECT parameter, max(conc_high) AS conc_cap
        FROM breakpoints
        GROUP BY parameter
    ),
    -- Bruts dédupliqués (vérité argMax) sur la plus grande fenêtre :
    -- 24 h ⇒ buckets [anchor − 23 h, anchor].
    dedup AS
    (
        SELECT
            parameter,
            measured_at,
            argMax(value, ingested_at) AS value
        FROM quarity.measurements
        WHERE location_id = {loc:UInt64}
          AND parameter IN ('pm25', 'pm10', 'o3', 'no2')  -- so2/co : pas de breakpoints seedés (cf. en-tête)
          AND measured_at >= anchor_hour - INTERVAL 23 HOUR
          AND measured_at <  anchor_hour + INTERVAL 1 HOUR
        GROUP BY parameter, measured_at
    ),
    hourly AS
    (
        SELECT
            parameter,
            toStartOfHour(toDateTime(measured_at, 'UTC')) AS bucket_hour,
            avg(value) AS hourly_avg
        FROM dedup
        GROUP BY parameter, bucket_hour
    ),
    -- Moyenne glissante de la fenêtre PROPRE à chaque polluant, close à anchor_hour.
    rolled AS
    (
        SELECT
            h.parameter         AS parameter,
            any(w.window_hours) AS window_hours,
            avg(h.hourly_avg)   AS rolling_avg_ugm3,
            toUInt64(count())   AS hours_present
        FROM hourly AS h
        INNER JOIN win AS w ON w.parameter = h.parameter
        WHERE h.bucket_hour >  anchor_hour - toIntervalHour(w.window_hours)
          AND h.bucket_hour <= anchor_hour
        GROUP BY h.parameter
    ),
    -- Conversion ppb (o3/no2, cf. en-tête) puis troncature EPA : pm25 à 0.1
    -- près (floor(x, 1)), pm10/o3/no2 à l'entier. greatest(., 0) : une
    -- concentration négative (artefact capteur) est ramenée à 0.
    converted AS
    (
        SELECT
            parameter,
            window_hours,
            hours_present,
            rolling_avg_ugm3,
            multiIf(
                parameter = 'o3',  rolling_avg_ugm3 * 24.45 / 48.00,  -- M(O3)  = 48.00 g/mol
                parameter = 'no2', rolling_avg_ugm3 * 24.45 / 46.01,  -- M(NO2) = 46.01 g/mol
                rolling_avg_ugm3
            ) AS conc_value,
            if(parameter IN ('o3', 'no2'), 'ppb', 'µg/m³') AS conc_unit,
            if(parameter = 'pm25',
               floor(greatest(conc_value, 0.), 1),
               floor(greatest(conc_value, 0.))) AS conc_trunc
        FROM rolled
    ),
    -- Lookup du palier : CROSS JOIN (24 lignes) + BETWEEN en WHERE — ClickHouse
    -- 24.8 ne supporte pas le non-equi JOIN ON.
    looked_up AS
    (
        SELECT
            c.parameter        AS parameter,
            c.window_hours     AS window_hours,
            c.hours_present    AS hours_present,
            c.rolling_avg_ugm3 AS rolling_avg_ugm3,
            c.conc_value       AS conc_value,
            c.conc_unit        AS conc_unit,
            least(c.conc_trunc, caps.conc_cap) AS conc_lookup,  -- clamp au barème
            bp.conc_low        AS conc_low,
            bp.conc_high       AS conc_high,
            bp.aqi_low         AS aqi_low,
            bp.aqi_high        AS aqi_high
        FROM converted AS c
        INNER JOIN caps ON caps.parameter = c.parameter
        CROSS JOIN breakpoints AS bp
        WHERE bp.parameter = c.parameter
          AND conc_lookup BETWEEN bp.conc_low AND bp.conc_high
    )
SELECT
    parameter,
    anchor_hour                  AS window_end,
    window_hours,
    hours_present,
    hours_present / window_hours AS coverage,
    coverage >= 0.75             AS is_valid,
    rolling_avg_ugm3,
    conc_value,
    conc_unit,
    -- Interpolation EPA + arrondi arithmétique (floor(x + 0.5), PAS round() — banker's).
    toInt32(floor((aqi_high - aqi_low) / (conc_high - conc_low) * (conc_lookup - conc_low) + aqi_low + 0.5)) AS aqi,
    aqi = max(aqi) OVER ()       AS is_dominant
FROM looked_up
ORDER BY parameter
FORMAT JSONEachRow


