-- =============================================================================
-- Quarity — Moyennes glissantes réglementaires + AQI instantané (Jalon 3 — backlog B5)
-- =============================================================================
-- Fichier de requêtes STANDALONE (aucun DDL : le projet n'a pas de mécanisme de
-- migration ClickHouse). Quatre requêtes paramétrées, exécutables TELLES QUELLES
-- via HTTP (POST du SQL en body + query-string param_loc/param_from/param_to/param_at/param_year,
-- même style que back/src/ch.rs) :
--   Q1 : rolling_regulatory — moyennes glissantes réglementaires par polluant.
--   Q2 : aqi_snapshot       — AQI US EPA instantané (fin de fenêtre {at}).
--   Q3 : o3_daily_max_8h    — O3 : max journalier de la moyenne glissante 8 h
--                             (extension B5, roadmap.md l.146).
--   Q4 : no2_annual_mean    — NO2 : moyenne annuelle, depuis le rollup journalier
--                             (extension B5, roadmap.md l.146).
-- Les tests (back/tests/ch.rs) exécutent CE fichier tel quel (include_str!) et
-- le découpent sur les séparateurs `-- ===== Qn : ... =====` (stables : ne pas
-- les renommer).
-- Sortie FORMAT JSONEachRow : ClickHouse sérialise les UInt64 (ex. hours_present)
-- en CHAÎNES JSON par défaut — tout consommateur doit passer
-- output_format_json_quote_64bit_integers=0 en query-string pour obtenir des
-- nombres, comme le font back/src/ch.rs et back/tests/ch.rs.
--
-- Décisions figées :
--   • Barème US EPA (décision n°6, verrouillée). Les paliers sont seedés côté
--     Postgres (db/sql/02_seed.sql, SOURCE DE VÉRITÉ). ClickHouse ne pouvant
--     pas joindre Postgres, le CTE `breakpoints` de Q2 en est un MIROIR copié
--     VERBATIM : toute évolution du seed doit être répercutée ici (et vice versa).
--   • Conversion d'unités (décision D4.2, docs/foundations.md) : OpenAQ stocke
--     O3 et NO2 en µg/m³, mais leurs paliers EPA sont en ppb. Conversion figée
--     à 25 °C / 1 atm (volume molaire Vm = 24.45 L/mol) :
--       ppb = µg/m³ × 24.45 / M, avec M(O3) = 48.00 g/mol et M(NO2) = 46.01 g/mol.
--   • so2/co : EXCLUS du calcul AQI — aucun breakpoint seedé pour eux (décision
--     de périmètre B5 du 2026-06-07). Exclus aussi de Q1 : US-07 ne leur définit
--     pas de fenêtre réglementaire.
--   • Multi-stations (docs/data-model.md D.5) : un lieu suivi agrège plusieurs
--     stations par MAX par polluant (principe de précaution sanitaire). HORS
--     SCOPE ici : ces requêtes sont MONO-STATION ({loc} = location_id OpenAQ
--     d'UNE station) ; l'agrégation MAX inter-stations se fera au-dessus.
--   • Moyennes RÉGLEMENTAIRES calculées depuis les bruts DÉDUPLIQUÉS, JAMAIS
--     depuis les rollups measurements_hourly/_daily : leurs MV se déclenchent
--     AVANT la déduplication ReplacingMergeTree et sur-comptent les doublons
--     (cf. db/clickhouse/01_schema.sql §C.4). Lecture de vérité = argMax(value,
--     ingested_at) GROUP BY (location_id, parameter, measured_at) — le pattern
--     canonique du projet (back/src/ch.rs).
--   • EXCEPTION à la règle ci-dessus (extension B5) — Q4 no2_annual_mean : le
--     TTL 90 j des bruts rend la fenêtre ANNUELLE impossible depuis
--     measurements ⇒ Q4 lit le rollup quarity.measurements_daily (rétention
--     5 ans), en assumant l'approximation documentée en tête de section Q4
--     (MV déclenchée AVANT la dédup, cf. 01_schema.sql §C.4). Q1/Q2/Q3 restent
--     sur les bruts dédupliqués.
--
-- Méthode EPA appliquée (US-07 + décision n°6) :
--   1. buckets HORAIRES dédupliqués (moyenne des bruts de l'heure),
--   2. moyenne glissante = moyenne des moyennes HORAIRES (pratique EPA) :
--      pm25 24 h, pm10 24 h, o3 8 h, no2 1 h,
--   3. couverture = heures présentes / heures attendues de la fenêtre ;
--      validité réglementaire si couverture ≥ 0.75 (règle EPA des 75 %),
--   4. AQI : troncature EPA de la concentration (pm25 à 0.1 près, pm10/o3/no2 à
--      l'entier — c'est elle qui comble les « trous » entre paliers, ex. 9.0/9.1),
--      valeurs négatives ramenées à 0, clamp à la borne haute du barème (l'AQI
--      plafonne ainsi naturellement à 500), interpolation linéaire
--      AQI = (aqi_high − aqi_low) / (conc_high − conc_low) × (C − conc_low) + aqi_low
--      (back/migrations/0001_init.sql l.200), puis arrondi ARITHMÉTIQUE
--      floor(x + 0.5) — PAS round() : le round() de ClickHouse arrondit « au
--      pair » (banker's rounding) et fausserait les .5.
-- =============================================================================


-- ===== Q1 : rolling_regulatory =====
-- Moyennes glissantes réglementaires par (parameter, bucket_hour) sur [{from}, {to}[.
-- Paramètres serveur : {loc:UInt64}, {from:String}, {to:String}
--                      (query-string : param_loc, param_from, param_to).
-- Sortie : parameter, bucket_hour, rolling_avg (en µg/m³, l'unité de stockage
--          OpenAQ — la conversion ppb n'intervient qu'au calcul d'AQI, Q2),
--          window_hours, hours_present, coverage (0..1), is_valid (≥ 0.75).
--
-- Amorçage du frame : les bruts sont lus depuis {from} − 24 h pour que la
-- fenêtre du PREMIER bucket affiché soit déjà nourrie ; le SELECT final ne
-- garde que [from, to[.
-- {from} non aligné à l'heure : le filtre de sortie compare le DÉBUT de bucket,
-- donc le bucket horaire contenant {from} (commencé avant lui) n'est PAS affiché
-- mais nourrit les frames des buckets suivants — cohérent avec l'ancrage à
-- l'heure pleine de Q2.
-- Fenêtre glissante : RANGE BETWEEN N PRECEDING AND CURRENT ROW sur
-- bucket_hour (DateTime ⇒ offsets en SECONDES : 86399 = 24 h − 1 s,
-- 28799 = 8 h − 1 s). PAS de ROWS : les heures creuses n'existent pas en table.
-- La borne du frame doit être CONSTANTE ⇒ une branche par fenêtre (UNION ALL).
WITH
    parseDateTime64BestEffort({from:String}, 3, 'UTC') AS dt_from,
    parseDateTime64BestEffort({to:String},   3, 'UTC') AS dt_to,
    -- Bruts dédupliqués (vérité argMax, cf. en-tête), lus dès {from} − 24 h (amorçage).
    dedup AS
    (
        SELECT
            parameter,
            measured_at,
            argMax(value, ingested_at) AS value
        FROM quarity.measurements
        WHERE location_id = {loc:UInt64}
          AND parameter IN ('pm25', 'pm10', 'o3', 'no2')  -- so2/co : pas de fenêtre US-07 (cf. en-tête)
          AND measured_at >= dt_from - INTERVAL 24 HOUR
          AND measured_at <  dt_to
        GROUP BY parameter, measured_at
    ),
    -- Buckets horaires : moyenne des bruts dédupliqués de l'heure (pratique EPA).
    hourly AS
    (
        SELECT
            parameter,
            toStartOfHour(toDateTime(measured_at, 'UTC')) AS bucket_hour,
            avg(value) AS hourly_avg
        FROM dedup
        GROUP BY parameter, bucket_hour
    ),
    windowed AS
    (
        -- pm25 + pm10 : fenêtre 24 h.
        SELECT
            parameter,
            bucket_hour,
            avg(hourly_avg) OVER w AS rolling_avg,
            toUInt32(24)           AS window_hours,
            count()         OVER w AS hours_present
        FROM hourly
        WHERE parameter IN ('pm25', 'pm10')
        WINDOW w AS (PARTITION BY parameter ORDER BY bucket_hour ASC
                     RANGE BETWEEN 86399 PRECEDING AND CURRENT ROW)

        UNION ALL

        -- o3 : fenêtre 8 h.
        SELECT
            parameter,
            bucket_hour,
            avg(hourly_avg) OVER w AS rolling_avg,
            toUInt32(8)            AS window_hours,
            count()         OVER w AS hours_present
        FROM hourly
        WHERE parameter = 'o3'
        WINDOW w AS (PARTITION BY parameter ORDER BY bucket_hour ASC
                     RANGE BETWEEN 28799 PRECEDING AND CURRENT ROW)

        UNION ALL

        -- no2 : fenêtre 1 h = le bucket horaire lui-même (aucun frame nécessaire).
        SELECT
            parameter,
            bucket_hour,
            hourly_avg  AS rolling_avg,
            toUInt32(1) AS window_hours,
            toUInt64(1) AS hours_present
        FROM hourly
        WHERE parameter = 'no2'
    )
SELECT
    parameter,
    bucket_hour,
    rolling_avg,
    window_hours,
    hours_present,
    hours_present / window_hours AS coverage,
    coverage >= 0.75             AS is_valid
FROM windowed
WHERE bucket_hour >= dt_from  -- l'amorçage ({from} − 24 h) nourrit le frame mais ne sort jamais
  AND bucket_hour <  dt_to
ORDER BY parameter, bucket_hour
FORMAT JSONEachRow


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


-- ===== Q3 : o3_daily_max_8h =====
-- O3 : MAXIMUM JOURNALIER de la moyenne glissante 8 h (extension B5,
-- roadmap.md l.146) — la métrique réglementaire EPA qui complète la moyenne
-- 8 h « continue » de Q1.
-- Paramètres serveur : {loc:UInt64}, {from:String}, {to:String}
--                      (query-string : param_loc, param_from, param_to).
-- Sortie PAR JOUR UTC dans [{from}, {to}[ : day, daily_max_8h (en µg/m³,
-- l'unité de stockage OpenAQ, comme Q1 — la conversion ppb n'intervient qu'au
-- calcul d'AQI, Q2), windows_present, valid_windows, day_valid.
--
-- Convention EPA HISTORIQUE (40 CFR Part 50 Appendix I, NAAQS O3 1997/2008,
-- rattachement des fenêtres) : le jour J considère les 24 fenêtres de 8 h
-- DÉBUTANT à chaque heure de J (fenêtre h..h+7) ; une fenêtre est rattachée au
-- jour de son heure de DÉPART, c.-à-d. au jour de (fin de fenêtre − 7 h).
-- NB : la méthode EPA EN VIGUEUR (Appendix U, NAAQS 2015) ne retient que les
-- 17 fenêtres débutant 07:00–23:00 en heure LOCALE standard (jour valide si
-- ≥ 13/17, ou si le max dépasse le standard) — l'Appendix I est retenu ici en
-- approximation démo (cohérent avec les paliers o3 de Q2), auto-consistant sur
-- des jours UTC explicites ; seul le seuil 6/8 par fenêtre est commun aux deux
-- méthodes. La dernière fenêtre de J (départ 23:00) se termine à
-- J+1 06:59 ⇒ les bruts sont lus jusqu'à {to} + 7 h pour fermer les fenêtres
-- démarrant en fin de dernier jour (ÉPILOGUE +7 h, symétrique de l'amorçage
-- −24 h de Q1). Aucun amorçage AVANT {from} : la première fenêtre du premier
-- jour démarre à {from} même. {from}/{to} sont attendus ALIGNÉS AU JOUR UTC :
-- seules les fenêtres dont l'heure de DÉPART est dans [{from}, {to}[ sortent.
--
-- Validité EPA : une fenêtre 8 h est valide si ≥ 6/8 heures présentes (75 %).
-- daily_max_8h = max des fenêtres VALIDES du jour. Exposé aussi :
--   windows_present = nb de fenêtres matérialisées (≥ 1 bucket),
--   valid_windows   = nb de fenêtres ≥ 6/8 heures,
--   day_valid       = valid_windows ≥ 18 (75 % des 24 fenêtres — seuil App. I).
-- FALLBACK documenté : si AUCUNE fenêtre valide dans le jour, daily_max_8h =
-- max des fenêtres PRÉSENTES (mieux qu'aucune valeur pour un capteur
-- clairsemé) ; day_valid = 0 distingue alors ce mode dégradé du mode
-- réglementaire (day_valid = 1 implique un max issu de fenêtres valides).
--
-- Limite de construction (assumée) : le frame RANGE PRECEDING n'émet une ligne
-- que par bucket horaire EXISTANT ⇒ une fenêtre n'est matérialisée que si un
-- bucket existe à son heure de FIN. Sur données clairsemées,
-- windows_present/valid_windows peuvent donc sous-compter l'énumération EPA
-- stricte des 24 fenêtres ; sur données denses (cas nominal d'une station
-- active), les 24 fenêtres du jour sont toutes matérialisées. Cas extrême :
-- un jour AVEC mesures peut ne produire AUCUNE ligne si aucun bucket n'existe
-- aux heures de FIN (h+7) des fenêtres démarrant ce jour (ex. mesures
-- uniquement 00:00–06:00 sur le seul jour interrogé : ces buckets ne servent
-- de fin qu'à des fenêtres démarrant la VEILLE). L'absence de ligne ne
-- signifie donc PAS « aucune donnée » — vérifier les bruts avant de conclure.
WITH
    parseDateTime64BestEffort({from:String}, 3, 'UTC') AS dt_from,
    parseDateTime64BestEffort({to:String},   3, 'UTC') AS dt_to,
    -- Bruts dédupliqués (vérité argMax, cf. en-tête de fichier), lus jusqu'à
    -- {to} + 7 h (ÉPILOGUE, cf. en-tête de section).
    dedup AS
    (
        SELECT
            measured_at,
            argMax(value, ingested_at) AS value
        FROM quarity.measurements
        WHERE location_id = {loc:UInt64}
          AND parameter = 'o3'                        -- métrique mono-polluant
          AND measured_at >= dt_from
          AND measured_at <  dt_to + INTERVAL 7 HOUR  -- ÉPILOGUE +7 h
        GROUP BY measured_at
    ),
    -- Buckets horaires : moyenne des bruts dédupliqués de l'heure (pratique
    -- EPA, même construction que Q1 — sans `parameter`, le filtre étant
    -- mono-polluant).
    hourly AS
    (
        SELECT
            toStartOfHour(toDateTime(measured_at, 'UTC')) AS bucket_hour,
            avg(value) AS hourly_avg
        FROM dedup
        GROUP BY bucket_hour
    ),
    -- Une ligne par fenêtre 8 h matérialisée : frame RANGE 28799 s (8 h − 1 s),
    -- identique à la branche o3 de Q1 (et même garde-fou : PAS de ROWS, les
    -- heures creuses n'existent pas en table). bucket_hour = heure de FIN de
    -- fenêtre ⇒ heure de DÉPART = bucket_hour − 7 h, la clé de rattachement.
    -- hours_present = heures PRÉSENTES dans le frame (convention de nommage de
    -- Q1/Q2 — la TAILLE de fenêtre, elle, est la constante 8).
    windowed AS
    (
        SELECT
            bucket_hour - INTERVAL 7 HOUR AS window_start,
            avg(hourly_avg) OVER w AS window_avg,
            count()         OVER w AS hours_present
        FROM hourly
        WINDOW w AS (ORDER BY bucket_hour ASC
                     RANGE BETWEEN 28799 PRECEDING AND CURRENT ROW)
    )
SELECT
    toDate(window_start) AS day,
    -- max des fenêtres VALIDES (≥ 6/8 h) ; FALLBACK max des présentes si
    -- aucune (cf. en-tête — maxIf seul rendrait 0, d'où le garde countIf > 0).
    if(countIf(hours_present >= 6) > 0,
       maxIf(window_avg, hours_present >= 6),
       max(window_avg))                   AS daily_max_8h,
    toUInt64(count())                     AS windows_present,
    toUInt64(countIf(hours_present >= 6)) AS valid_windows,
    valid_windows >= 18                  AS day_valid  -- 75 % des 24 fenêtres
FROM windowed
WHERE window_start >= dt_from  -- rattachement au jour de DÉPART : [{from}, {to}[
  AND window_start <  dt_to
GROUP BY day
ORDER BY day
FORMAT JSONEachRow


-- ===== Q4 : no2_annual_mean =====
-- NO2 : MOYENNE ANNUELLE (extension B5, roadmap.md l.146). Valeur limite UE :
-- 40 µg/m³ en moyenne annuelle (directive 2008/50/CE, en vigueur ; la
-- directive (UE) 2024/2881 l'abaissera à 20 µg/m³ au 1er janvier 2030) —
-- citée À TITRE INFORMATIF uniquement, AUCUN seuil dur dans la requête (la
-- comparaison réglementaire se fera au-dessus, côté consommateur).
-- Paramètres serveur : {loc:UInt64}, {year:UInt16}
--                      (query-string : param_loc, param_year).
-- Sortie : year, annual_avg (µg/m³), days_present (nb de bucket_day
--          distincts), days_in_year (365/366), coverage (0..1),
--          is_valid (≥ 0.75). Année sans aucune donnée ⇒ AUCUNE ligne
--          (cohérent avec Q2 : pas de ligne sans bucket).
--
-- EXCEPTION à la règle « jamais les rollups » (cf. Décisions figées) : la
-- fenêtre annuelle est IMPOSSIBLE depuis les bruts (TTL 90 j) ⇒ lecture du
-- rollup quarity.measurements_daily (rétention 5 ans). Deux choix assumés :
--   (a) APPROXIMATION — la MV mv_measurements_daily se déclenche sur chaque
--       bloc inséré AVANT la déduplication ReplacingMergeTree : un doublon
--       ré-ingéré compte deux fois dans avg_state (cf. 01_schema.sql §C.4,
--       qui documente aussi le correctif après backfill : repeuplement
--       DROP PARTITION + INSERT SELECT ... FINAL). Comportement GELÉ par le
--       test q4_rollup_overcounts_reingested_duplicates_by_design.
--   (b) PONDÉRATION (seconde approximation) — avgMerge(avg_state) pondère par
--       le NOMBRE DE MESURES brutes : annual_avg est la moyenne des MESURES de
--       l'année — ni la moyenne des moyennes journalières, ni exactement la
--       définition UE (moyenne des valeurs HORAIRES de l'année civile : une
--       heure à N mesures pèse ici N fois plus qu'une heure à 1 mesure).
--       Q1/Q2/Q3 neutralisent ce biais par bucketisation horaire ; impossible
--       ici, le rollup journalier agrège les bruts. Comportement GELÉ par le
--       test q4_annual_mean_is_weighted_by_measurement_not_by_day.
--
-- Lecture obligatoire d'un AggregatingMergeTree : avgMerge + re-GROUP BY (les
-- parts ne sont pas forcément fusionnées ⇒ plusieurs lignes par bucket_day
-- possibles ; d'où aussi uniqExact(bucket_day) — exact, pas uniq — pour
-- days_present).
-- days_in_year VRAIMENT calculé (bissextile) : toDaysInYear n'existe pas en
-- ClickHouse 24.8 (vérifié : UNKNOWN_FUNCTION) ⇒ différence des 1ers janvier
-- de {year}+1 et {year} (Date − Date ⇒ Int32 : 365 ou 366).
-- is_valid : seuil ≥ 0.75 par COHÉRENCE avec Q1/Q2/Q3 ; la règle UE de capture
-- de données est plus stricte (90 % des valeurs horaires de l'année) — à
-- durcir côté consommateur si besoin réglementaire strict.
WITH
    makeDate(toUInt32({year:UInt16}),     1, 1) AS year_start,
    makeDate(toUInt32({year:UInt16}) + 1, 1, 1) AS year_next
SELECT
    {year:UInt16}                    AS year,
    avgMerge(avg_state)              AS annual_avg,    -- pondérée par mesure (cf. (b))
    toUInt64(uniqExact(bucket_day))  AS days_present,
    toUInt32(year_next - year_start) AS days_in_year,  -- 365/366, bissextile incluse
    days_present / days_in_year      AS coverage,
    coverage >= 0.75                 AS is_valid
FROM quarity.measurements_daily
WHERE location_id = {loc:UInt64}
  AND parameter = 'no2'
  AND bucket_day >= year_start
  AND bucket_day <  year_next
GROUP BY year  -- année sans donnée ⇒ zéro groupe ⇒ aucune ligne (comme Q2)
FORMAT JSONEachRow
