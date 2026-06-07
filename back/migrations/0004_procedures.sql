-- =============================================================================
-- Quarity — Migration 0004 : procédures P1–P3 (Jalon 3 — backlog B3)
-- =============================================================================
--   P1 create_tracked_location_with_rules : point d'entrée transactionnel qui
--      garantit l'invariant « ≥ 1 station » À LA CRÉATION (T1 ne protège que
--      le retrait) + crée les règles associées en un seul geste (US-01).
--   P2 archive_old_alert_events : déplace les events LUS et anciens vers
--      alert_events_archive (l'audit ne supprime jamais, il déplace).
--   P3 compute_exposure_dose : moitié TRANSACTIONNELLE du calcul de dose —
--      l'agrégat fenêtré vit côté ClickHouse (exécuté par le back, B9) ; ici on
--      fige seuil + fenêtre au moment du calcul et on upsert le cache (US-08).
-- Idempotente (CREATE OR REPLACE / IF NOT EXISTS) — convention 0001.
-- Erreurs métier : préfixe 'QRT_Pn:' + ERRCODE 'check_violation' (→ 422 API).
-- =============================================================================

-- -----------------------------------------------------------------------------
-- Table d'archive des faits d'alerte (support de P2).
-- Mêmes colonnes qu'alert_events, SANS identity (l'id d'origine est préservé)
-- et SANS FK (fait figé : il doit survivre à la disparition de tout référent).
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS alert_events_archive (
    id                  BIGINT PRIMARY KEY,            -- id d'origine (traçabilité)
    alert_rule_id       BIGINT,
    org_id              BIGINT        NOT NULL,
    tracked_location_id BIGINT,
    ref_location_id     BIGINT        NOT NULL,
    openaq_location_id  BIGINT        NOT NULL,
    openaq_sensor_id    BIGINT,
    parameter_code      TEXT          NOT NULL,
    measured_value      NUMERIC(12,4) NOT NULL,
    unit                TEXT          NOT NULL,
    measured_at         TIMESTAMPTZ   NOT NULL,
    threshold_value     NUMERIC(12,4) NOT NULL,
    comparator          TEXT          NOT NULL,
    severity            TEXT          NOT NULL,
    fired_at            TIMESTAMPTZ   NOT NULL,
    is_read             BOOLEAN       NOT NULL,
    created_at          TIMESTAMPTZ   NOT NULL,
    archived_at         TIMESTAMPTZ   NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_aea_org_fired ON alert_events_archive (org_id, fired_at DESC);
COMMENT ON TABLE alert_events_archive IS
  'Archive des alert_events (P2). Faits figés sans FK : défendables en audit même après suppression des référents. Alimentée exclusivement par archive_old_alert_events.';

-- -----------------------------------------------------------------------------
-- P1 — create_tracked_location_with_rules
-- Stations par clé naturelle OpenAQ (la 1re du tableau devient primaire).
-- Règles en JSONB : [{"parameter":"pm25","comparator":">","threshold":15.0,
--                     "severity":"warning","name":"…"}, …]
-- Tout échec (station inconnue, polluant inconnu, tableau vide) annule TOUT
-- (corps de procédure = atomique dans la transaction appelante).
-- -----------------------------------------------------------------------------
CREATE OR REPLACE PROCEDURE create_tracked_location_with_rules(
    p_org_id               BIGINT,
    p_name                 TEXT,
    p_description          TEXT,
    p_created_by           BIGINT,
    p_openaq_location_ids  BIGINT[],
    p_rules                JSONB  DEFAULT '[]'::jsonb,
    INOUT p_tracked_location_id BIGINT DEFAULT NULL
)
LANGUAGE plpgsql AS $$
DECLARE
    v_expected INT;
    v_inserted INT;
BEGIN
    v_expected := COALESCE(cardinality(p_openaq_location_ids), 0);
    IF v_expected = 0 THEN
        RAISE EXCEPTION 'QRT_P1: un lieu suivi exige au moins une station (US-01 c2)'
            USING ERRCODE = 'check_violation';
    END IF;

    -- Doublons refusés EXPLICITEMENT (revue) : sinon la contrainte uq_tls lèverait
    -- une unique_violation (23505) que le back mapperait en 500 au lieu de 422,
    -- avec un message trompeur.
    IF (SELECT count(DISTINCT s) FROM unnest(p_openaq_location_ids) AS s) <> v_expected THEN
        RAISE EXCEPTION 'QRT_P1: stations en double dans p_openaq_location_ids (%)', p_openaq_location_ids
            USING ERRCODE = 'check_violation';
    END IF;

    INSERT INTO tracked_locations (org_id, name, description, created_by)
    VALUES (p_org_id, p_name, p_description, p_created_by)
    RETURNING id INTO p_tracked_location_id;

    -- Stations : résolution par clé naturelle, 1re du tableau = primaire.
    INSERT INTO tracked_location_stations (tracked_location_id, ref_location_id, is_primary)
    SELECT p_tracked_location_id, rl.id, (s.ord = 1)
      FROM unnest(p_openaq_location_ids) WITH ORDINALITY AS s(openaq_id, ord)
      JOIN ref_locations rl ON rl.openaq_location_id = s.openaq_id;

    GET DIAGNOSTICS v_inserted = ROW_COUNT;
    IF v_inserted <> v_expected THEN
        RAISE EXCEPTION 'QRT_P1: station(s) OpenAQ inconnue(s) dans % (%/% résolues) — ingérer le référentiel d''abord',
            p_openaq_location_ids, v_inserted, v_expected
            USING ERRCODE = 'check_violation';
    END IF;

    -- Règles d'alerte optionnelles (org_id contrôlé par T2 par construction).
    v_expected := jsonb_array_length(COALESCE(p_rules, '[]'::jsonb));
    IF v_expected > 0 THEN
        INSERT INTO alert_rules (org_id, tracked_location_id, parameter_id, comparator,
                                 threshold_value, severity, name, created_by)
        SELECT p_org_id,
               p_tracked_location_id,
               prm.id,
               r->>'comparator',
               (r->>'threshold')::NUMERIC,
               COALESCE(r->>'severity', 'warning'),
               NULLIF(r->>'name', ''),
               p_created_by
          FROM jsonb_array_elements(p_rules) AS r
          JOIN parameters prm ON prm.code = r->>'parameter';

        GET DIAGNOSTICS v_inserted = ROW_COUNT;
        IF v_inserted <> v_expected THEN
            RAISE EXCEPTION 'QRT_P1: polluant inconnu dans p_rules (%/% règles créées) — codes valides : cf. parameters',
                v_inserted, v_expected
                USING ERRCODE = 'check_violation';
        END IF;
    END IF;
END $$;

COMMENT ON PROCEDURE create_tracked_location_with_rules IS
  'P1 (US-01) : crée lieu + stations (1re = primaire) + règles, atomiquement. Garantit >= 1 station à la création (T1 protège ensuite le retrait). CALL ... puis lire p_tracked_location_id.';

-- -----------------------------------------------------------------------------
-- P2 — archive_old_alert_events
-- Déplace vers l'archive les events LUS dont fired_at < now() - p_older_than_days.
-- Les non-lus ne sont JAMAIS archivés (une alerte non traitée reste visible).
-- DELETE → T6 décrémenterait le compteur : sans objet ici (events lus), mais le
-- trigger gère le cas par construction si la politique évoluait.
-- -----------------------------------------------------------------------------
CREATE OR REPLACE PROCEDURE archive_old_alert_events(
    p_older_than_days INTEGER,
    INOUT p_archived_count BIGINT DEFAULT 0
)
LANGUAGE plpgsql AS $$
BEGIN
    IF p_older_than_days IS NULL OR p_older_than_days < 0 THEN
        RAISE EXCEPTION 'QRT_P2: p_older_than_days doit être un entier >= 0 (reçu : %)', p_older_than_days
            USING ERRCODE = 'check_violation';
    END IF;

    WITH moved AS (
        DELETE FROM alert_events ae
         WHERE ae.is_read IS TRUE
           AND ae.fired_at < now() - make_interval(days => p_older_than_days)
        RETURNING ae.id, ae.alert_rule_id, ae.org_id, ae.tracked_location_id,
                  ae.ref_location_id, ae.openaq_location_id, ae.openaq_sensor_id,
                  ae.parameter_code, ae.measured_value, ae.unit, ae.measured_at,
                  ae.threshold_value, ae.comparator, ae.severity, ae.fired_at,
                  ae.is_read, ae.created_at
    )
    INSERT INTO alert_events_archive (id, alert_rule_id, org_id, tracked_location_id,
                                      ref_location_id, openaq_location_id, openaq_sensor_id,
                                      parameter_code, measured_value, unit, measured_at,
                                      threshold_value, comparator, severity, fired_at,
                                      is_read, created_at)
    SELECT m.id, m.alert_rule_id, m.org_id, m.tracked_location_id,
           m.ref_location_id, m.openaq_location_id, m.openaq_sensor_id,
           m.parameter_code, m.measured_value, m.unit, m.measured_at,
           m.threshold_value, m.comparator, m.severity, m.fired_at,
           m.is_read, m.created_at
      FROM moved m;

    GET DIAGNOSTICS p_archived_count = ROW_COUNT;
END $$;

COMMENT ON PROCEDURE archive_old_alert_events IS
  'P2 : déplace les alert_events LUS plus vieux que N jours vers alert_events_archive (jamais de suppression sèche). Renvoie le nombre déplacé via p_archived_count.';

-- -----------------------------------------------------------------------------
-- P3 — compute_exposure_dose (moitié Postgres)
-- Le back calcule hours_over_threshold/sample_count côté ClickHouse (fenêtre
-- horaire du tlp, règle MAX multi-stations — cf. data-model §D.5) puis appelle
-- cette procédure, qui fige le seuil applicable et le SNAPSHOT de la fenêtre
-- (reproductibilité US-08 c3) et upsert le cache exposure_results.
-- -----------------------------------------------------------------------------
CREATE OR REPLACE PROCEDURE compute_exposure_dose(
    p_tracked_location_profile_id BIGINT,
    p_parameter_code              TEXT,
    p_period_start                DATE,
    p_period_end                  DATE,
    p_hours_over_threshold        NUMERIC,
    p_sample_count                INTEGER,
    p_averaging_period            TEXT   DEFAULT '1h',
    INOUT p_exposure_result_id    BIGINT DEFAULT NULL
)
LANGUAGE plpgsql AS $$
DECLARE
    v_param_id   INT;
    v_threshold  NUMERIC(12,4);
    v_start_time TIME;
    v_end_time   TIME;
    v_days_mask  SMALLINT;
    v_timezone   TEXT;
    v_is_active  BOOLEAN;
BEGIN
    IF p_hours_over_threshold IS NULL OR p_hours_over_threshold < 0 THEN
        RAISE EXCEPTION 'QRT_P3: p_hours_over_threshold doit être >= 0 (reçu : %)', p_hours_over_threshold
            USING ERRCODE = 'check_violation';
    END IF;

    SELECT prm.id, et.threshold_value, tlp.start_time, tlp.end_time, tlp.days_mask, tlp.timezone,
           tlp.is_active
      INTO v_param_id, v_threshold, v_start_time, v_end_time, v_days_mask, v_timezone,
           v_is_active
      FROM tracked_location_profiles tlp
      JOIN parameters prm          ON prm.code = p_parameter_code
      JOIN exposure_thresholds et  ON et.exposure_profile_id = tlp.exposure_profile_id
                                  AND et.parameter_id = prm.id
                                  AND et.averaging_period = p_averaging_period
     WHERE tlp.id = p_tracked_location_profile_id;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'QRT_P3: pas de seuil % (%) pour le profil du tlp % — vérifier exposure_thresholds',
            p_parameter_code, p_averaging_period, p_tracked_location_profile_id
            USING ERRCODE = 'check_violation';
    END IF;

    -- Défense en profondeur (revue) : pas de calcul de dose pour une association
    -- lieu × profil désactivée — le back ne devrait pas appeler, la base refuse.
    IF v_is_active IS FALSE THEN
        RAISE EXCEPTION 'QRT_P3: le tlp % est désactivé (is_active = false) — pas de calcul de dose',
            p_tracked_location_profile_id
            USING ERRCODE = 'check_violation';
    END IF;

    INSERT INTO exposure_results (tracked_location_profile_id, parameter_id,
                                  period_start, period_end, threshold_value,
                                  window_start_time, window_end_time, window_days_mask,
                                  timezone, hours_over_threshold, sample_count)
    VALUES (p_tracked_location_profile_id, v_param_id,
            p_period_start, p_period_end, v_threshold,
            v_start_time, v_end_time, v_days_mask,
            v_timezone, p_hours_over_threshold, COALESCE(p_sample_count, 0))
    ON CONFLICT ON CONSTRAINT uq_exposure_result DO UPDATE
        SET threshold_value      = EXCLUDED.threshold_value,
            window_start_time    = EXCLUDED.window_start_time,
            window_end_time      = EXCLUDED.window_end_time,
            window_days_mask     = EXCLUDED.window_days_mask,
            timezone             = EXCLUDED.timezone,
            hours_over_threshold = EXCLUDED.hours_over_threshold,
            sample_count         = EXCLUDED.sample_count,
            computed_at          = now()
    RETURNING id INTO p_exposure_result_id;
END $$;

COMMENT ON PROCEDURE compute_exposure_dose IS
  'P3 (US-08) : moitié transactionnelle du calcul de dose. Fige seuil + fenêtre (snapshot reproductible) et upsert exposure_results. L''agrégat fenêtré vient de ClickHouse (back, Jalon 3 B9).';
