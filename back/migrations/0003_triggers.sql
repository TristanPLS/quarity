-- =============================================================================
-- Quarity — Migration 0003 : triggers T1–T6 (Jalon 3 — backlog B2)
-- =============================================================================
-- Implémente les invariants « hors-schéma » documentés dans 0001_init.sql §8 et
-- docs/data-model.md §E : règles métier non exprimables par contrainte déclarative.
--
-- Conventions :
--   * Une fonction par invariant (trg_*), idempotence par CREATE OR REPLACE +
--     DROP TRIGGER IF EXISTS avant chaque CREATE TRIGGER.
--   * Violation = RAISE EXCEPTION avec message préfixé 'QRT_Tn:' et
--     ERRCODE 'check_violation' (23514) → le back mappe vers 422/409.
--   * L'acteur applicatif est lu depuis la GUC de session `quarity.actor_user_id`
--     (posée par le back via set_config(..., true) — repli : created_by).
-- =============================================================================

-- -----------------------------------------------------------------------------
-- T6 (préalable) — colonne dénormalisée : compteur d'alertes non-lues par org.
-- 0001 est GELÉE : l'ajout de colonne passe par cette migration. Initialisée
-- depuis l'existant (recalcul idempotent), maintenue ensuite par trigger.
-- -----------------------------------------------------------------------------
ALTER TABLE organizations
    ADD COLUMN IF NOT EXISTS unread_alert_count INTEGER NOT NULL DEFAULT 0;

-- CHECK en contrainte NOMMÉE et conditionnelle (revue) : un simple CHECK inline
-- dans le ADD COLUMN IF NOT EXISTS ne serait PAS rétro-appliqué si la colonne
-- préexistait sans lui. Le test couvre aussi le nom auto-généré d'une
-- application antérieure de cette migration (variante inline).
DO $$ BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint c
         WHERE c.conrelid = 'organizations'::regclass
           AND c.contype  = 'c'
           AND c.conname IN ('chk_org_unread_nonneg', 'organizations_unread_alert_count_check')
    ) THEN
        ALTER TABLE organizations
            ADD CONSTRAINT chk_org_unread_nonneg CHECK (unread_alert_count >= 0);
    END IF;
END $$;

-- Recalibrage : réutilisable à tout moment (restore partiel, TRUNCATE manuel
-- d'alert_events, COPY massif…) — TRUNCATE ne déclenche PAS les triggers ROW.
CREATE OR REPLACE FUNCTION recompute_unread_alert_counts() RETURNS void
LANGUAGE sql AS $$
    -- Sous-requête corrélée : une org sans event non-lu revient bien à 0.
    UPDATE organizations o
       SET unread_alert_count = (SELECT count(*) FROM alert_events ae
                                  WHERE ae.org_id = o.id AND ae.is_read IS FALSE)
     WHERE o.unread_alert_count IS DISTINCT FROM
           (SELECT count(*) FROM alert_events ae
             WHERE ae.org_id = o.id AND ae.is_read IS FALSE);
$$;

SELECT recompute_unread_alert_counts();  -- initialisation depuis l'existant (idempotent)

COMMENT ON COLUMN organizations.unread_alert_count IS
  'Dénormalisation T6 (Jalon 3) : nb d''alert_events non-lues. Maintenu par trigger sur alert_events — ne JAMAIS écrire directement. Après un TRUNCATE manuel d''alert_events, un restore partiel ou un COPY massif (aucun ne déclenche les triggers ROW) : SELECT recompute_unread_alert_counts(). Limite MVP connue (revue) : chaque event écrit verrouille la ligne org (hot row) — OK à l''échelle actuelle, à passer en agrégation différée si la boucle de matching (B7) génère des rafales > ~10 events/s par org.';

-- -----------------------------------------------------------------------------
-- T1 — tracked_location_stations : refuser de retirer la DERNIÈRE station d'un
-- lieu ACTIF (invariant « ≥ 1 station », US-01 c2/c4 → 422 côté API).
-- Cas couverts : DELETE direct et UPDATE déplaçant la ligne vers un autre lieu.
-- Cas autorisé : suppression en cascade depuis tracked_locations (le parent
-- n'existe déjà plus au moment où la cascade touche les enfants → lookup NULL).
-- Concurrence : verrou FOR UPDATE sur le lieu — deux suppressions simultanées
-- des deux dernières stations se sérialisent au lieu de passer toutes les deux ;
-- l'ordre d'arrivée décide laquelle passe (la seconde reçoit QRT_T1) — c'est la
-- sémantique voulue, l'invariant prime sur l'équité d'ordre.
-- -----------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION trg_tls_guard_last_station() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    v_is_active BOOLEAN;
    v_others    BIGINT;
BEGIN
    -- UPDATE ne nous concerne que si la ligne change de lieu.
    IF TG_OP = 'UPDATE' AND NEW.tracked_location_id = OLD.tracked_location_id THEN
        RETURN NEW;
    END IF;

    SELECT tl.is_active INTO v_is_active
      FROM tracked_locations tl
     WHERE tl.id = OLD.tracked_location_id
       FOR UPDATE;  -- sérialise les retraits concurrents sur un même lieu

    -- Parent absent (cascade en cours) ou lieu inactif : retrait libre.
    IF v_is_active IS NULL OR v_is_active IS FALSE THEN
        RETURN CASE TG_OP WHEN 'DELETE' THEN OLD ELSE NEW END;
    END IF;

    SELECT count(*) INTO v_others
      FROM tracked_location_stations tls
     WHERE tls.tracked_location_id = OLD.tracked_location_id
       AND tls.id <> OLD.id;

    IF v_others = 0 THEN
        RAISE EXCEPTION 'QRT_T1: impossible de retirer la dernière station du lieu actif % (invariant US-01 : un lieu actif garde >= 1 station)',
            OLD.tracked_location_id
            USING ERRCODE = 'check_violation';
    END IF;

    RETURN CASE TG_OP WHEN 'DELETE' THEN OLD ELSE NEW END;
END $$;

DROP TRIGGER IF EXISTS t1_guard_last_station ON tracked_location_stations;
CREATE TRIGGER t1_guard_last_station
    BEFORE DELETE OR UPDATE OF tracked_location_id ON tracked_location_stations
    FOR EACH ROW EXECUTE FUNCTION trg_tls_guard_last_station();

-- -----------------------------------------------------------------------------
-- T2 — alert_rules : org_id DOIT égaler tracked_locations.org_id (US-09/16).
-- Choix : REJET explicite (pas de correction silencieuse) — un mismatch signale
-- soit un bug du back, soit une tentative cross-tenant ; le masquer serait pire.
-- Anti-TOCTOU (revue) : FOR SHARE sur le lieu — son org_id ne peut pas changer
-- entre la vérification et le commit de l'insertion.
-- -----------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION trg_alert_rules_org_coherence() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    v_loc_org BIGINT;
BEGIN
    SELECT tl.org_id INTO v_loc_org
      FROM tracked_locations tl
     WHERE tl.id = NEW.tracked_location_id
       FOR SHARE;

    -- Lieu inexistant : on laisse la FK produire son erreur canonique.
    IF v_loc_org IS NULL THEN
        RETURN NEW;
    END IF;

    IF NEW.org_id IS DISTINCT FROM v_loc_org THEN
        RAISE EXCEPTION 'QRT_T2: alert_rules.org_id (%) != org du lieu suivi % (%) — violation d''isolation multi-tenant',
            NEW.org_id, NEW.tracked_location_id, v_loc_org
            USING ERRCODE = 'check_violation';
    END IF;

    RETURN NEW;
END $$;

DROP TRIGGER IF EXISTS t2_org_coherence ON alert_rules;
CREATE TRIGGER t2_org_coherence
    BEFORE INSERT OR UPDATE OF org_id, tracked_location_id ON alert_rules
    FOR EACH ROW EXECUTE FUNCTION trg_alert_rules_org_coherence();

-- -----------------------------------------------------------------------------
-- T3 — alert_rule_recipients : un destinataire INTERNE (user_id) doit être
-- membre de l'org de la règle (US-15 c4). Les cibles externes (email/webhook)
-- restent libres par conception (parents d'école, systèmes tiers).
-- Anti-TOCTOU (revue) : FOR SHARE sur la membership — sa suppression concurrente
-- est bloquée jusqu'au commit de l'insertion (aucune FK ne re-garantit après).
-- -----------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION trg_arr_recipient_in_org() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    v_rule_org    BIGINT;
    v_membership  BIGINT;
BEGIN
    IF NEW.user_id IS NULL THEN
        RETURN NEW;  -- cible externe : rien à vérifier
    END IF;

    SELECT ar.org_id INTO v_rule_org
      FROM alert_rules ar
     WHERE ar.id = NEW.alert_rule_id;

    IF v_rule_org IS NULL THEN
        RETURN NEW;  -- règle inexistante : la FK tranchera
    END IF;

    SELECT m.id INTO v_membership
      FROM memberships m
     WHERE m.org_id = v_rule_org
       AND m.user_id = NEW.user_id
       FOR SHARE;

    IF v_membership IS NULL THEN
        RAISE EXCEPTION 'QRT_T3: le user % n''est pas membre de l''org % de la règle % (destinataire cross-tenant refusé)',
            NEW.user_id, v_rule_org, NEW.alert_rule_id
            USING ERRCODE = 'check_violation';
    END IF;

    RETURN NEW;
END $$;

DROP TRIGGER IF EXISTS t3_recipient_in_org ON alert_rule_recipients;
CREATE TRIGGER t3_recipient_in_org
    BEFORE INSERT OR UPDATE OF user_id, alert_rule_id ON alert_rule_recipients
    FOR EACH ROW EXECUTE FUNCTION trg_arr_recipient_in_org();

-- -----------------------------------------------------------------------------
-- T4 — alert_events : IMMUABILITÉ du snapshot (décision 5, US-03/10).
-- Comparaison jsonb OLD/NEW privée des colonnes mutables : couvre TOUTES les
-- autres colonnes, y compris celles qu'une migration future ajouterait
-- (fail-closed). Colonnes mutables :
--   * is_read — librement (lecture d'une alerte) ;
--   * alert_rule_id / tracked_location_id — UNIQUEMENT vers NULL : ce sont les
--     FK en ON DELETE SET NULL (« l'event survit à la suppression du référent »),
--     et un SET NULL référentiel déclenche ce trigger d'UPDATE. Re-pointer vers
--     une AUTRE valeur resterait une falsification → refusé.
-- -----------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION trg_alert_events_immutable() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF (to_jsonb(NEW) - 'is_read' - 'alert_rule_id' - 'tracked_location_id')
       IS DISTINCT FROM
       (to_jsonb(OLD) - 'is_read' - 'alert_rule_id' - 'tracked_location_id') THEN
        RAISE EXCEPTION 'QRT_T4: alert_events est un fait d''audit immuable — seul is_read est mutable (event %)',
            OLD.id
            USING ERRCODE = 'check_violation';
    END IF;

    IF NEW.alert_rule_id IS DISTINCT FROM OLD.alert_rule_id
       AND NEW.alert_rule_id IS NOT NULL THEN
        RAISE EXCEPTION 'QRT_T4: alert_events.alert_rule_id ne peut évoluer que vers NULL (cascade SET NULL) — event %',
            OLD.id
            USING ERRCODE = 'check_violation';
    END IF;

    IF NEW.tracked_location_id IS DISTINCT FROM OLD.tracked_location_id
       AND NEW.tracked_location_id IS NOT NULL THEN
        RAISE EXCEPTION 'QRT_T4: alert_events.tracked_location_id ne peut évoluer que vers NULL (cascade SET NULL) — event %',
            OLD.id
            USING ERRCODE = 'check_violation';
    END IF;

    RETURN NEW;
END $$;

DROP TRIGGER IF EXISTS t4_snapshot_immutable ON alert_events;
CREATE TRIGGER t4_snapshot_immutable
    BEFORE UPDATE ON alert_events
    FOR EACH ROW EXECUTE FUNCTION trg_alert_events_immutable();

-- -----------------------------------------------------------------------------
-- T5 — audit automatique d'alert_rules → audit_log (roadmap l.130, ESG US-10).
-- Actions : create / update / activate / deactivate / delete.
-- diff : INSERT/DELETE = ligne complète ; UPDATE = {col: {from, to}} des seuls
-- champs modifiés (updated_at exclu — bruit). Update sans changement réel : pas
-- de ligne d'audit. Acteur = GUC quarity.actor_user_id, repli created_by.
-- -----------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION trg_alert_rules_audit() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    v_actor  BIGINT := NULLIF(current_setting('quarity.actor_user_id', true), '')::BIGINT;
    v_action TEXT;
    v_diff   JSONB;
BEGIN
    IF TG_OP = 'INSERT' THEN
        INSERT INTO audit_log (org_id, actor_user_id, entity_type, entity_id, action, diff)
        VALUES (NEW.org_id, COALESCE(v_actor, NEW.created_by), 'alert_rule', NEW.id, 'create', to_jsonb(NEW));
        RETURN NEW;

    ELSIF TG_OP = 'UPDATE' THEN
        SELECT jsonb_object_agg(n.key, jsonb_build_object('from', o.value, 'to', n.value))
          INTO v_diff
          FROM jsonb_each(to_jsonb(OLD)) o
          JOIN jsonb_each(to_jsonb(NEW)) n USING (key)
         WHERE o.value IS DISTINCT FROM n.value
           AND n.key <> 'updated_at';

        IF v_diff IS NULL THEN
            RETURN NEW;  -- update sans changement réel : pas de bruit d'audit
        END IF;

        v_action := CASE
            WHEN OLD.status = 'active'   AND NEW.status = 'inactive' THEN 'deactivate'
            WHEN OLD.status = 'inactive' AND NEW.status = 'active'   THEN 'activate'
            ELSE 'update'
        END;

        INSERT INTO audit_log (org_id, actor_user_id, entity_type, entity_id, action, diff)
        VALUES (NEW.org_id, v_actor, 'alert_rule', NEW.id, v_action, v_diff);
        RETURN NEW;

    ELSE  -- DELETE
        INSERT INTO audit_log (org_id, actor_user_id, entity_type, entity_id, action, diff)
        VALUES (OLD.org_id, v_actor, 'alert_rule', OLD.id, 'delete', to_jsonb(OLD));
        RETURN OLD;
    END IF;
END $$;

DROP TRIGGER IF EXISTS t5_audit ON alert_rules;
CREATE TRIGGER t5_audit
    AFTER INSERT OR UPDATE OR DELETE ON alert_rules
    FOR EACH ROW EXECUTE FUNCTION trg_alert_rules_audit();

-- -----------------------------------------------------------------------------
-- T6 — maintenance du compteur organizations.unread_alert_count.
-- AFTER trigger : ne court qu'après que T4 (BEFORE) a validé l'update.
-- org_id est immuable (garanti par T4) → pas de cas « event change d'org ».
-- GREATEST(0, …) : filet de sécurité (le CHECK refuserait un négatif).
-- -----------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION trg_alert_events_unread_counter() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.is_read IS FALSE THEN
            UPDATE organizations SET unread_alert_count = unread_alert_count + 1
             WHERE id = NEW.org_id;
        END IF;
        RETURN NEW;

    ELSIF TG_OP = 'UPDATE' THEN
        IF OLD.is_read IS FALSE AND NEW.is_read IS TRUE THEN
            UPDATE organizations SET unread_alert_count = GREATEST(0, unread_alert_count - 1)
             WHERE id = NEW.org_id;
        ELSIF OLD.is_read IS TRUE AND NEW.is_read IS FALSE THEN
            UPDATE organizations SET unread_alert_count = unread_alert_count + 1
             WHERE id = NEW.org_id;
        END IF;
        RETURN NEW;

    ELSE  -- DELETE (archivage P2, purge)
        IF OLD.is_read IS FALSE THEN
            UPDATE organizations SET unread_alert_count = GREATEST(0, unread_alert_count - 1)
             WHERE id = OLD.org_id;
        END IF;
        RETURN OLD;
    END IF;
END $$;

DROP TRIGGER IF EXISTS t6_unread_counter ON alert_events;
CREATE TRIGGER t6_unread_counter
    AFTER INSERT OR UPDATE OF is_read OR DELETE ON alert_events
    FOR EACH ROW EXECUTE FUNCTION trg_alert_events_unread_counter();
