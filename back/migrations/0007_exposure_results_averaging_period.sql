-- =============================================================================
-- Migration 0007 — exposure_results : discriminer les doses par periode de
-- moyennage (correctif de CORRECTION, B9a-3 / US-08).
--
-- DEFAUT CORRIGE : `POST .../compute-dose` calcule une dose par seuil du profil
-- (1h / 8h / 24h ...), mais `uq_exposure_result` ne portait que
-- (tracked_location_profile_id, parameter_id, period_start, period_end) — SANS
-- `averaging_period`. Pour un meme polluant declare sur plusieurs fenetres
-- (ex. pm25 1h ET pm25 24h), la 2e dose ECRASAIT la 1re a l'upsert
-- (ON CONFLICT DO UPDATE de compute_exposure_dose) → resultat silencieusement
-- FAUX (une seule ligne par polluant/periode au lieu d'une par fenetre).
--
-- CORRECTIF : ajouter la colonne `averaging_period` a `exposure_results`,
-- l'inclure dans la cle d'unicite (une ligne PAR fenetre), et l'ecrire dans la
-- procedure `compute_exposure_dose` (qui recevait deja `p_averaging_period`,
-- mais ne le stockait pas — il ne servait qu'a re-resoudre le seuil).
--
-- exposure_results est un CACHE recalculable (cf. COMMENT 0001 : « une modif de
-- plage du tlp invalide ce cache »). Les lignes pre-existantes (potentiellement
-- corrompues par la collision) sont retro-etiquetees '1h' puis recalculees au
-- prochain compute-dose. Migration idempotente.
-- =============================================================================

-- 1) Colonne discriminante. DEFAULT '1h' uniquement pour retro-remplir l'existant,
--    puis retire : tout INSERT doit desormais fournir averaging_period explicitement
--    (sinon un oubli redeviendrait silencieusement '1h' et recollisionnerait).
ALTER TABLE exposure_results
    ADD COLUMN IF NOT EXISTS averaging_period TEXT NOT NULL DEFAULT '1h'
        CHECK (averaging_period IN ('1h', '8h', '24h', 'annual'));
ALTER TABLE exposure_results
    ALTER COLUMN averaging_period DROP DEFAULT;

-- 2) Nouvelle cle d'unicite incluant la fenetre de moyennage. Les lignes
--    retro-etiquetees '1h' ne peuvent pas se dupliquer : l'ancienne contrainte
--    garantissait deja l'unicite sur (tlp, parameter, period_start, period_end).
ALTER TABLE exposure_results DROP CONSTRAINT IF EXISTS uq_exposure_result;
ALTER TABLE exposure_results
    ADD CONSTRAINT uq_exposure_result
    UNIQUE (tracked_location_profile_id, parameter_id, averaging_period, period_start, period_end);

-- 3) Procedure : ecrire `averaging_period` (= la fenetre re-resolue cote seuil) et
--    cibler la nouvelle contrainte. Corps identique a 0004 par ailleurs.
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
        RAISE EXCEPTION 'QRT_P3: p_hours_over_threshold doit etre >= 0 (recu : %)', p_hours_over_threshold
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
        RAISE EXCEPTION 'QRT_P3: pas de seuil % (%) pour le profil du tlp % — verifier exposure_thresholds',
            p_parameter_code, p_averaging_period, p_tracked_location_profile_id
            USING ERRCODE = 'check_violation';
    END IF;

    -- Defense en profondeur (revue) : pas de calcul de dose pour une association
    -- lieu x profil desactivee — le back ne devrait pas appeler, la base refuse.
    IF v_is_active IS FALSE THEN
        RAISE EXCEPTION 'QRT_P3: le tlp % est desactive (is_active = false) — pas de calcul de dose',
            p_tracked_location_profile_id
            USING ERRCODE = 'check_violation';
    END IF;

    INSERT INTO exposure_results (tracked_location_profile_id, parameter_id,
                                  averaging_period, period_start, period_end, threshold_value,
                                  window_start_time, window_end_time, window_days_mask,
                                  timezone, hours_over_threshold, sample_count)
    VALUES (p_tracked_location_profile_id, v_param_id,
            p_averaging_period, p_period_start, p_period_end, v_threshold,
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

COMMENT ON COLUMN exposure_results.averaging_period IS
  'Fenetre de moyennage du seuil applique (1h/8h/24h). Discriminant de uq_exposure_result depuis 0007 : un meme polluant sur plusieurs fenetres = autant de lignes distinctes.';
