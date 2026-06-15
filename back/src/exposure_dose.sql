-- Dose d'exposition (B9a-3). Nombre d'heures ou la moyenne glissante (sur
-- averaging_period heures) de la concentration MAX multi-stations DEPASSE le seuil,
-- a l'interieur de la plage horaire locale d'un tracked_location_profile, sur une
-- periode de dates. Renvoie (hours_over_threshold, sample_count).
--
-- Existe en DOUBLE, contenu identique : back/src/exposure_dose.sql (embarque par le
-- crate, include_str!) et db/clickhouse/queries/exposure_dose.sql (canonique, axe BDD).
-- Tenus synchrones par tests/ch.rs::embedded_exposure_dose_sql_matches_canonical (lecon
-- B10 : le contexte de build Docker du back est back/ seul, un include_str! hors-crate
-- casserait cargo build --release).
--
-- Parametres server-side (anti-injection) :
--   {locs:Array(UInt64)}                stations openaq du lieu (regle MAX, data-model D.5)
--   {param:String}                      code polluant
--   {avg_hours:UInt32}                  fenetre de moyennage en heures (1 / 8 / 24)
--   {threshold:Float64}                 seuil applique (depassement STRICT >)
--   {from:Date} {to:Date}               bornes de periode, en DATE LOCALE (incluses)
--   {start_min:UInt16} {end_min:UInt16} plage horaire quotidienne en minutes du jour [start, end)
--   {days_mask:UInt16}                  bitmask jours (bit0=Lundi .. bit6=Dimanche)
--   {tz:String}                         fuseau de la plage (ex Europe/Paris)
--
-- Heures bucketisees en UTC (toStartOfHour) puis converties en local (toTimeZone) pour
-- les filtres date/heure/jour. Mesures dedupliquees par argMax(value, ingested_at) avant
-- la moyenne horaire (re-ingestion). La fenetre amont est elargie de avg_hours pour que
-- la moyenne glissante du premier jour soit complete.
WITH
  dedup AS (
    SELECT location_id, sensor_id, measured_at, argMax(value, ingested_at) AS value
    FROM quarity.measurements
    WHERE location_id IN {locs:Array(UInt64)}
      AND parameter = {param:String}
      AND measured_at >= parseDateTimeBestEffort(concat(toString({from:Date}), ' 00:00:00'), {tz:String}) - toIntervalHour({avg_hours:UInt32})
      AND measured_at <  parseDateTimeBestEffort(concat(toString(addDays({to:Date}, 1)), ' 00:00:00'), {tz:String})
    GROUP BY location_id, sensor_id, measured_at
  ),
  hourly AS (
    SELECT location_id, toStartOfHour(measured_at) AS hour, avg(value) AS hour_value
    FROM dedup
    GROUP BY location_id, hour
  ),
  rolled AS (
    SELECT h1.location_id AS location_id, h1.hour AS hour, avg(h2.hour_value) AS roll
    FROM hourly AS h1
    INNER JOIN hourly AS h2 ON h2.location_id = h1.location_id
    WHERE h2.hour <= h1.hour AND h2.hour > h1.hour - toIntervalHour({avg_hours:UInt32})
    GROUP BY h1.location_id, h1.hour
  ),
  per_hour AS (
    SELECT hour, max(roll) AS conc
    FROM rolled
    GROUP BY hour
  )
SELECT
  countIf(conc > {threshold:Float64}) AS hours_over_threshold,
  count() AS sample_count
FROM per_hour
WHERE toDate(toTimeZone(hour, {tz:String})) >= {from:Date}
  AND toDate(toTimeZone(hour, {tz:String})) <= {to:Date}
  AND (toHour(toTimeZone(hour, {tz:String})) * 60 + toMinute(toTimeZone(hour, {tz:String}))) >= {start_min:UInt16}
  AND (toHour(toTimeZone(hour, {tz:String})) * 60 + toMinute(toTimeZone(hour, {tz:String}))) <  {end_min:UInt16}
  -- Jour actif : bit (0=Lun .. 6=Dim) du days_mask. toDayOfWeek(...,1) est DEJA
  -- 0-based (Lun=0), donc PAS de -1 (l'ancien -1 decalait tous les jours d'un bit et
  -- cassait le lundi). Forme bitAnd/bitShiftLeft et non bitTest : ClickHouse 24.8+
  -- rejette bitTest et bitShiftLeft avec une position non-constante de type signe
  -- (PARAMETER_OUT_OF_BOUND) ; ici le decalage est un UInt8 non signe (0..6).
  AND bitAnd({days_mask:UInt16}, bitShiftLeft(toUInt16(1), toDayOfWeek(toTimeZone(hour, {tz:String}), 1))) != 0
FORMAT JSONEachRow
