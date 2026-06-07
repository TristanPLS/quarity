-- =============================================================================
-- Quarity — Requête métier complexe (Jalon 3 — backlog B4, roadmap l.141)
-- =============================================================================
-- Question métier : « Top 10 des orgs par volume d'alertes CRITIQUES sur le
-- dernier trimestre, avec leur plan d'abonnement et le ratio lu/non-lu » —
-- pilotage commercial (quelles orgs saturent ? upsell de plan ?) et produit
-- (quelles orgs ignorent leurs alertes ?).
--
-- Exigences B4 couvertes : 1 CTE + 4 jointures + GROUP BY + HAVING.
-- Lecture seule — exécutable telle quelle : psql -f complex_business_query.sql
-- Sur le seed : groupeindus (2 critiques, plan Enterprise) devant agglo-riviera
-- (1 critique, plan Pro) ; cityair (0 critique) est éliminée par le HAVING.
-- =============================================================================

WITH recent_critical AS (
    -- CTE : les faits d'alerte critiques du dernier trimestre glissant.
    SELECT ae.id,
           ae.org_id,
           ae.tracked_location_id,
           ae.is_read
      FROM alert_events ae
     WHERE ae.severity = 'critical'
       AND ae.fired_at >= now() - interval '3 months'
)
SELECT o.name                                          AS org_name,
       o.slug                                          AS org_slug,
       sp.name                                         AS plan_name,
       count(rc.id)                                    AS critical_events,
       count(DISTINCT tl.id)                           AS locations_hit,
       count(*) FILTER (WHERE rc.is_read IS FALSE)     AS unread_critical,
       round(avg((rc.is_read)::int), 2)                AS read_ratio
  FROM recent_critical rc
  JOIN organizations o               ON o.id = rc.org_id          -- j1
                                    AND o.deleted_at IS NULL
  JOIN organization_subscriptions os ON os.org_id = o.id          -- j2
                                    AND os.status = 'active'
  JOIN subscription_plans sp         ON sp.id = os.plan_id        -- j3
  LEFT JOIN tracked_locations tl     ON tl.id = rc.tracked_location_id  -- j4 (LEFT : le lieu a pu être supprimé, le fait reste)
 GROUP BY o.id, o.name, o.slug, sp.name
HAVING count(rc.id) >= 1                  -- exclut les orgs sans alerte critique
 ORDER BY critical_events DESC, unread_critical DESC, o.name
 LIMIT 10;
