-- =============================================================================
-- Quarity — Migration 0005 : trigger T7 (durcissement post-analyse 2026-06-08)
-- =============================================================================
-- Referme l'asymétrie d'isolation relevée par la revue multi-agents : T2
-- (alert_rules) et T3 (alert_rule_recipients) verrouillent déjà la cohérence
-- d'org au niveau base, mais alert_events — la table des FAITS d'alerte, la plus
-- sensible (audit) — n'avait AUCUN garde-fou. Un INSERT applicatif (future boucle
-- de matching B7) avec un org_id incohérent aurait fui dans le dashboard d'un
-- AUTRE tenant et faussé le compteur T6 / org_alert_stats_view. T7 ferme ce
-- chemin côté base (défense en profondeur, AVANT de brancher B7).
--
-- Conventions identiques à 0003 : CREATE OR REPLACE + DROP TRIGGER IF EXISTS ;
-- violation = RAISE 'QRT_T7:' / ERRCODE 'check_violation' (23514) ;
-- FOR SHARE sur les référents (anti-TOCTOU), comme T2/T3.
-- =============================================================================

-- -----------------------------------------------------------------------------
-- T7 — alert_events : org_id DOIT concorder avec celui de ses référents non-NULL
-- (le lieu suivi ET la règle déclenchante). Les colonnes snapshot (ref_location_id,
-- openaq_*, etc.) restent libres : sans FK par conception, elles survivent à la
-- purge des dimensions. Quand les DEUX référents sont NULL (event ayant survécu à
-- un SET NULL en cascade), seul org_id porte le tenant et la FK organizations en
-- garantit l'existence — rien d'autre à vérifier.
--
-- BEFORE INSERT seulement : org_id est immuable (T4 rejette toute évolution de
-- org_id sur UPDATE) et alert_rule_id / tracked_location_id ne peuvent évoluer que
-- vers NULL (T4) — aucune incohérence ne peut donc naître d'un UPDATE.
-- -----------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION trg_alert_events_org_coherence() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    v_loc_org  BIGINT;
    v_rule_org BIGINT;
BEGIN
    IF NEW.tracked_location_id IS NOT NULL THEN
        SELECT tl.org_id INTO v_loc_org
          FROM tracked_locations tl
         WHERE tl.id = NEW.tracked_location_id
           FOR SHARE;
        -- Lieu inexistant : on laisse la FK produire son erreur canonique.
        IF v_loc_org IS NOT NULL AND NEW.org_id IS DISTINCT FROM v_loc_org THEN
            RAISE EXCEPTION 'QRT_T7: alert_events.org_id (%) != org du lieu suivi % (%) — violation d''isolation multi-tenant',
                NEW.org_id, NEW.tracked_location_id, v_loc_org
                USING ERRCODE = 'check_violation';
        END IF;
    END IF;

    IF NEW.alert_rule_id IS NOT NULL THEN
        SELECT ar.org_id INTO v_rule_org
          FROM alert_rules ar
         WHERE ar.id = NEW.alert_rule_id
           FOR SHARE;
        -- Règle inexistante : la FK tranchera.
        IF v_rule_org IS NOT NULL AND NEW.org_id IS DISTINCT FROM v_rule_org THEN
            RAISE EXCEPTION 'QRT_T7: alert_events.org_id (%) != org de la règle % (%) — violation d''isolation multi-tenant',
                NEW.org_id, NEW.alert_rule_id, v_rule_org
                USING ERRCODE = 'check_violation';
        END IF;
    END IF;

    RETURN NEW;
END $$;

DROP TRIGGER IF EXISTS t7_org_coherence ON alert_events;
CREATE TRIGGER t7_org_coherence
    BEFORE INSERT ON alert_events
    FOR EACH ROW EXECUTE FUNCTION trg_alert_events_org_coherence();
