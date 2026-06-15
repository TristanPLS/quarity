-- =============================================================================
-- Quarity — Migration 0009 : index de lecture exposure_results (tlp, period_end)
-- =============================================================================
-- list_results (routes/exposure_dose.rs) lit les doses figees d'une association,
-- "periode la plus recente d'abord" :
--   WHERE tracked_location_profile_id = $1
--   ORDER BY period_end DESC, parameter, averaging_period
--
-- L'unique uq_exposure_result (tlp_id, parameter_id, period_start, period_end) sert
-- deja le filtre par prefixe (tlp_id en tete) mais N'ORDONNE PAS par period_end ->
-- Postgres ajoute un tri. Cet index (tlp_id, period_end DESC) couvre filtre + tri
-- (evite le sort) et prepare le scheduler de dose (8c) qui relira ce cache souvent.
--
-- Idempotent (IF NOT EXISTS). Aucune donnee touchee, aucun changement de comportement.
-- =============================================================================

CREATE INDEX IF NOT EXISTS idx_exposure_results_tlp_period
    ON exposure_results (tracked_location_profile_id, period_end DESC);
