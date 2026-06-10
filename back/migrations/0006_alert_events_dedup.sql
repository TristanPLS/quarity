-- =============================================================================
-- 0006 — B7 : déduplication des alert_events écrits par la boucle de matching.
-- =============================================================================
--
-- POURQUOI : la fenêtre d'ingestion (scheduler A6a, OPENAQ_DAYS=1) fait revoir
-- LES MÊMES mesures à chaque tick — et la boucle de matching réévalue tout ce
-- que l'ingestion vient d'apporter. Sans verrou d'unicité, chaque passage
-- re-déclencherait les mêmes événements (tempête d'alertes + compteur T6 faux).
-- L'idempotence est garantie EN BASE, pas en mémoire : un redémarrage du back
-- (perte de watermark) ou un re-matching manuel (force-check B7) restent sûrs.
--
-- CONTRAT : un triplet (règle, capteur déclencheur, horodatage de MESURE) ne
-- produit qu'UN événement — la boucle insère en ON CONFLICT DO NOTHING sur
-- cette clé. `measured_at` (fait physique) et non `fired_at` (heure du match,
-- jamais rejouable à l'identique).
--
-- Index PARTIEL : `alert_rule_id` devient NULL à la suppression de la règle
-- (FK ON DELETE SET NULL) — ces lignes historiques sortent du périmètre
-- d'unicité (des NULL seraient de toute façon distincts au sens SQL ; le WHERE
-- rend l'intention explicite et garde l'index petit).
CREATE UNIQUE INDEX IF NOT EXISTS uq_alert_events_dedup
    ON alert_events (alert_rule_id, openaq_sensor_id, measured_at)
    WHERE alert_rule_id IS NOT NULL;

COMMENT ON INDEX uq_alert_events_dedup IS
  'B7 : idempotence du matching — 1 événement max par (règle, capteur, measured_at). Cible du ON CONFLICT DO NOTHING de la boucle.';
