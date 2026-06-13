-- =============================================================================
-- Quarity — Migration 0008 : trigger T8 (isolation lieu × profil d'exposition)
-- =============================================================================
-- Referme la derniere asymetrie d'isolation multi-tenant (constat A5, audit 06-12).
-- T2 (alert_rules), T3 (alert_rule_recipients) et T7 (alert_events) verrouillent
-- deja la coherence d'org EN BASE, mais `tracked_location_profiles` — l'association
-- n-aire lieu x profil d'exposition — n'etait protegee QUE par la garde applicative
-- (routes/tracked_location_profiles.rs : le lieu doit etre de l'org, le profil doit
-- etre systeme ou de l'org). Un INSERT/UPDATE direct en base accepterait donc
-- (lieu_orgA, profil_custom_orgB). T8 ferme ce chemin (defense en profondeur).
--
-- Invariant (identique a la garde applicative) : le profil doit etre SYSTEME
-- (org_id NULL, partage par toutes les orgs) OU appartenir a la MEME org que le lieu.
--
-- Conventions identiques a 0003 / 0005 : CREATE OR REPLACE + DROP TRIGGER IF EXISTS ;
-- violation = RAISE 'QRT_T8:' / ERRCODE 'check_violation' (23514) ;
-- FOR SHARE sur les referents (anti-TOCTOU), comme T2/T3/T7.
-- BEFORE INSERT OR UPDATE : lieu/profil ne sont rendus immuables que cote app (pas
-- en base) — on couvre donc aussi un UPDATE qui repointerait vers un profil etranger.
-- =============================================================================

CREATE OR REPLACE FUNCTION trg_tracked_location_profile_org_coherence() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    v_loc_org     BIGINT;
    v_profile_org BIGINT;
BEGIN
    SELECT tl.org_id INTO v_loc_org
      FROM tracked_locations tl
     WHERE tl.id = NEW.tracked_location_id
       FOR SHARE;

    SELECT ep.org_id INTO v_profile_org
      FROM exposure_profiles ep
     WHERE ep.id = NEW.exposure_profile_id
       FOR SHARE;

    -- Profil systeme (org_id NULL) : partage -> toujours OK. Sinon, l'org du profil
    -- DOIT etre celle du lieu. (Lieu/profil inexistant : la FK produit son erreur
    -- canonique ; tracked_locations.org_id etant NOT NULL, v_loc_org n'est jamais NULL
    -- pour un lieu existant.)
    IF v_profile_org IS NOT NULL AND v_loc_org IS NOT NULL
       AND v_profile_org IS DISTINCT FROM v_loc_org THEN
        RAISE EXCEPTION 'QRT_T8: tracked_location_profiles — le profil % (org %) n''est ni systeme ni de l''org du lieu % (org %) — violation d''isolation multi-tenant',
            NEW.exposure_profile_id, v_profile_org, NEW.tracked_location_id, v_loc_org
            USING ERRCODE = 'check_violation';
    END IF;

    RETURN NEW;
END $$;

DROP TRIGGER IF EXISTS t8_tlp_org_coherence ON tracked_location_profiles;
CREATE TRIGGER t8_tlp_org_coherence
    BEFORE INSERT OR UPDATE ON tracked_location_profiles
    FOR EACH ROW EXECUTE FUNCTION trg_tracked_location_profile_org_coherence();
