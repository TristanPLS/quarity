-- =============================================================================
-- Quarity — Migration 0002 : vues métier (Jalon 3 — backlog B1)
-- =============================================================================
-- Deux vues de lecture pour le back (B6) et les dashboards (B10/B11) :
--   * org_active_zones_view : les lieux suivis ACTIFS d'une org, avec leurs
--     compteurs (stations, règles actives, profils) et la dernière alerte.
--   * org_alert_stats_view  : statistiques d'alerte par org (volumes, non-lus,
--     ratio lu/non-lu, 30 derniers jours).
-- Idempotente (CREATE OR REPLACE) — convention identique à 0001.
-- =============================================================================

-- -----------------------------------------------------------------------------
-- B1.1 org_active_zones_view — « zones actives » par org (US-01/US-10)
-- Sous-requêtes scalaires plutôt que jointures agrégées : évite l'explosion
-- combinatoire stations × règles × events (fan-out) et reste lisible.
-- -----------------------------------------------------------------------------
CREATE OR REPLACE VIEW org_active_zones_view AS
SELECT
    o.id        AS org_id,
    o.slug      AS org_slug,
    tl.id       AS tracked_location_id,
    tl.name     AS location_name,
    tl.description,
    (SELECT count(*)
       FROM tracked_location_stations tls
      WHERE tls.tracked_location_id = tl.id)                       AS station_count,
    (SELECT rl.name
       FROM tracked_location_stations tls
       JOIN ref_locations rl ON rl.id = tls.ref_location_id
      WHERE tls.tracked_location_id = tl.id
        AND tls.is_primary)                                        AS primary_station_name,
    (SELECT count(DISTINCT rl.country)
       FROM tracked_location_stations tls
       JOIN ref_locations rl ON rl.id = tls.ref_location_id
      WHERE tls.tracked_location_id = tl.id)                       AS country_count,
    (SELECT count(*)
       FROM alert_rules ar
      WHERE ar.tracked_location_id = tl.id
        AND ar.status = 'active')                                  AS active_rule_count,
    (SELECT count(*)
       FROM tracked_location_profiles tlp
      WHERE tlp.tracked_location_id = tl.id
        AND tlp.is_active)                                         AS exposure_profile_count,
    (SELECT max(ae.fired_at)
       FROM alert_events ae
      WHERE ae.tracked_location_id = tl.id)                        AS last_alert_at
FROM tracked_locations tl
JOIN organizations o ON o.id = tl.org_id
WHERE tl.is_active
  AND o.deleted_at IS NULL;

COMMENT ON VIEW org_active_zones_view IS
  'Lieux suivis actifs par org + compteurs (stations, règles actives, profils) et dernière alerte. Filtrer par org_id côté API (isolation multi-tenant — la vue ne porte aucune isolation).';

-- -----------------------------------------------------------------------------
-- B1.2 org_alert_stats_view — statistiques d'alerte par org (US-10)
-- LEFT JOIN : une org sans alerte apparaît avec des compteurs à 0 (ranking).
-- -----------------------------------------------------------------------------
CREATE OR REPLACE VIEW org_alert_stats_view AS
SELECT
    o.id    AS org_id,
    o.slug  AS org_slug,
    o.name  AS org_name,
    count(ae.id)                                                       AS total_events,
    count(*) FILTER (WHERE ae.is_read IS FALSE)                        AS unread_events,
    count(*) FILTER (WHERE ae.severity = 'critical')                   AS critical_events,
    count(*) FILTER (WHERE ae.severity = 'warning')                    AS warning_events,
    count(*) FILTER (WHERE ae.fired_at >= now() - interval '30 days')  AS events_last_30d,
    max(ae.fired_at)                                                   AS last_event_at,
    -- ratio lu = lus / total (NULL si aucune alerte — évite un faux 100 %)
    round(avg((ae.is_read)::int), 2)                                   AS read_ratio
FROM organizations o
LEFT JOIN alert_events ae ON ae.org_id = o.id
WHERE o.deleted_at IS NULL
GROUP BY o.id, o.slug, o.name;

COMMENT ON VIEW org_alert_stats_view IS
  'Statistiques d''alerte par org (volumes par sévérité, non-lus, 30 j, ratio lu). Une org sans alerte a des compteurs à 0 et un read_ratio NULL.';
