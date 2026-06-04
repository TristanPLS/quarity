-- =============================================================================
-- Quarity — Seed de démonstration PostgreSQL  (Jalon 1)
-- =============================================================================
-- Objectif : rendre testable CHAQUE user story "Must" + chaque vue/trigger/
-- procédure du Jalon 3 + l'isolation multi-tenant (3 orgs).
-- Les FK sont résolues par SOUS-REQUÊTE sur clés naturelles (slug/email/code/…),
-- car les PK sont GENERATED ALWAYS AS IDENTITY (pas d'ID en dur).
-- Prérequis : 01_schema.sql appliqué. Rejouable (TRUNCATE en tête).
-- =============================================================================

BEGIN;

TRUNCATE
    audit_log, exposure_results, tracked_location_profiles, exposure_thresholds,
    exposure_profiles, notification_deliveries, alert_events, alert_rule_recipients,
    alert_rules, tracked_location_stations, tracked_locations, aqi_breakpoints,
    aqi_categories, ref_sensors, ref_locations, parameters, api_tokens,
    organization_subscriptions, subscription_plans, memberships, roles, users, organizations
RESTART IDENTITY CASCADE;

-- --- Référentiels indépendants du tenant -------------------------------------

-- roles (US-09)
INSERT INTO roles (code, label, can_manage_org, can_write) VALUES
    ('admin',        'Administrateur', true,  true),
    ('gestionnaire', 'Gestionnaire',   false, true),
    ('lecteur',      'Lecteur',        false, false);

-- parameters (unité canonique OpenAQ par polluant)
INSERT INTO parameters (code, display_name, unit, openaq_parameter_id) VALUES
    ('pm25', 'PM2.5', 'µg/m³', 2),
    ('pm10', 'PM10',  'µg/m³', 1),
    ('no2',  'NO₂',   'µg/m³', 7),
    ('o3',   'O₃',    'µg/m³', 10),
    ('so2',  'SO₂',   'µg/m³', 9),
    ('co',   'CO',    'mg/m³', 8);

-- subscription_plans (US-12 ; quota=0 ⇒ illimité côté applicatif)
INSERT INTO subscription_plans (code, name, monthly_request_quota, rate_limit_per_min, max_tracked_locations, max_members, price_cents) VALUES
    ('free',       'Free',         1000,    30, 3,    3,    0),
    ('starter',    'Starter',     50000,   120, 25,   10,   4900),
    ('pro',        'Pro',        500000,   600, 200,  50,  29900),
    ('enterprise', 'Enterprise',      0,  2000, NULL, NULL, 99900);

-- aqi_categories — 6 niveaux EPA, couleurs/libellés EXACTS d'identity.md (verrou couleur↔catégorie)
INSERT INTO aqi_categories (category, label, color_hex) VALUES
    (1, 'Bon',                              '#00E400'),
    (2, 'Modéré',                           '#FFFF00'),
    (3, 'Mauvais pour groupes sensibles',   '#FF7E00'),
    (4, 'Mauvais',                          '#FF0000'),
    (5, 'Très mauvais',                     '#8F3F97'),
    (6, 'Dangereux',                        '#7E0023');

-- aqi_breakpoints — barème EPA par polluant × fenêtre. `unit` = unité de référence de l'AQI
-- (peut différer de l'unité OpenAQ brute → conversion au calcul, Jalon 3).
--   PM2.5 / 24h (µg/m³, barème EPA 2024)
INSERT INTO aqi_breakpoints (parameter_id, category, averaging_period, unit, conc_low, conc_high, aqi_low, aqi_high)
SELECT p.id, v.cat, '24h', 'µg/m³', v.cl, v.ch, v.al, v.ah
FROM parameters p, (VALUES
    (1,   0.0,   9.0,   0,  50),
    (2,   9.1,  35.4,  51, 100),
    (3,  35.5,  55.4, 101, 150),
    (4,  55.5, 125.4, 151, 200),
    (5, 125.5, 225.4, 201, 300),
    (6, 225.5, 325.4, 301, 500)
) AS v(cat, cl, ch, al, ah)
WHERE p.code = 'pm25';
--   PM10 / 24h (µg/m³)
INSERT INTO aqi_breakpoints (parameter_id, category, averaging_period, unit, conc_low, conc_high, aqi_low, aqi_high)
SELECT p.id, v.cat, '24h', 'µg/m³', v.cl, v.ch, v.al, v.ah
FROM parameters p, (VALUES
    (1,   0,  54,   0,  50),
    (2,  55, 154,  51, 100),
    (3, 155, 254, 101, 150),
    (4, 255, 354, 151, 200),
    (5, 355, 424, 201, 300),
    (6, 425, 604, 301, 500)
) AS v(cat, cl, ch, al, ah)
WHERE p.code = 'pm10';
--   O3 / 8h (ppb — unité EPA, ≠ µg/m³ OpenAQ : illustre le verrou d'unité ; cat5/6 8h = approximation démo)
INSERT INTO aqi_breakpoints (parameter_id, category, averaging_period, unit, conc_low, conc_high, aqi_low, aqi_high)
SELECT p.id, v.cat, '8h', 'ppb', v.cl, v.ch, v.al, v.ah
FROM parameters p, (VALUES
    (1,   0,  54,   0,  50),
    (2,  55,  70,  51, 100),
    (3,  71,  85, 101, 150),
    (4,  86, 105, 151, 200),
    (5, 106, 200, 201, 300),
    (6, 201, 504, 301, 500)
) AS v(cat, cl, ch, al, ah)
WHERE p.code = 'o3';
--   NO2 / 1h (ppb — unité EPA)
INSERT INTO aqi_breakpoints (parameter_id, category, averaging_period, unit, conc_low, conc_high, aqi_low, aqi_high)
SELECT p.id, v.cat, '1h', 'ppb', v.cl, v.ch, v.al, v.ah
FROM parameters p, (VALUES
    (1,    0,   53,   0,  50),
    (2,   54,  100,  51, 100),
    (3,  101,  360, 101, 150),
    (4,  361,  649, 151, 200),
    (5,  650, 1249, 201, 300),
    (6, 1250, 2049, 301, 500)
) AS v(cat, cl, ch, al, ah)
WHERE p.code = 'no2';

-- exposure_profiles système (org_id NULL, is_system) + seuils adaptés (US-04)
INSERT INTO exposure_profiles (org_id, code, name, description, is_system) VALUES
    (NULL, 'enfants',         'Enfants',          'Population sensible : enfants (seuils OMS abaissés).', true),
    (NULL, 'asthmatiques',    'Asthmatiques',     'Personnes asthmatiques / pathologies respiratoires.',  true),
    (NULL, 'personnes_agees', 'Personnes âgées',  'Population âgée sensible.',                             true),
    (NULL, 'sportifs',        'Sportifs',         'Effort physique en extérieur.',                        true),
    (NULL, 'general',         'Population générale','Seuils réglementaires standards.',                    true);

INSERT INTO exposure_thresholds (exposure_profile_id, parameter_id, threshold_value, averaging_period)
SELECT ep.id, p.id, v.thr, v.ap
FROM exposure_profiles ep
JOIN (VALUES
    ('enfants',      'pm25', 15.0, '1h'),
    ('enfants',      'no2',  40.0, '1h'),
    ('enfants',      'o3',   100.0,'8h'),
    ('asthmatiques', 'pm25', 10.0, '1h'),
    ('asthmatiques', 'o3',   80.0, '8h'),
    ('personnes_agees','pm25',12.0,'24h')
) AS v(profile_code, param_code, thr, ap) ON ep.code = v.profile_code
JOIN parameters p ON p.code = v.param_code
WHERE ep.is_system;

-- --- Référentiel OpenAQ (stations + capteurs) --------------------------------
INSERT INTO ref_locations (openaq_location_id, name, country, city, latitude, longitude, timezone) VALUES
    (1001, 'Nice Promenade',        'FR', 'Nice',    43.6951,  7.2659, 'Europe/Paris'),
    (1002, 'Nice Centre',           'FR', 'Nice',    43.7010,  7.2680, 'Europe/Paris'),
    (1003, 'Cannes Croisette',      'FR', 'Cannes',  43.5513,  7.0174, 'Europe/Paris'),
    (1004, 'Antibes',               'FR', 'Antibes', 43.5808,  7.1250, 'Europe/Paris'),
    (2001, 'Lyon Part-Dieu',        'FR', 'Lyon',    45.7606,  4.8579, 'Europe/Paris'),
    (3001, 'Antwerpen Centrum',     'BE', 'Anvers',  51.2194,  4.4025, 'Europe/Brussels'),
    (4001, 'Berlin Mitte',          'DE', 'Berlin',  52.5200, 13.4050, 'Europe/Berlin');

-- 1 capteur par polluant suivi (pm25/no2/o3) et par station
INSERT INTO ref_sensors (openaq_sensor_id, ref_location_id, parameter_id)
SELECT (rl.openaq_location_id * 10 + p.id), rl.id, p.id
FROM ref_locations rl
JOIN parameters p ON p.code IN ('pm25','no2','o3');

-- --- Tenants (3 orgs — isolation multi-tenant US-09) -------------------------
INSERT INTO organizations (name, slug, segment) VALUES
    ('Agglo Riviera', 'agglo-riviera', 'B2G'),
    ('GroupeIndus SA','groupeindus',   'B2B'),
    ('CityAir App',   'cityair',       'B2B2C');

-- users — hash Argon2id RÉEL ; tous les comptes de démo ont le mot de passe : Quarity2026!
-- (régénérable via : quarity-back hash '<motdepasse>')
INSERT INTO users (email, password_hash, full_name) VALUES
    ('sophie@agglo-riviera.fr', '$argon2id$v=19$m=19456,t=2,p=1$MTz7aIPtJvTueF+dakcuXQ$NprTF6h0zutVJ9u3MkycUPqZevDEbix+ip/zpgyqFBk', 'Sophie Marchand'),
    ('karim@agglo-riviera.fr',  '$argon2id$v=19$m=19456,t=2,p=1$MTz7aIPtJvTueF+dakcuXQ$NprTF6h0zutVJ9u3MkycUPqZevDEbix+ip/zpgyqFBk', 'Karim Benali'),
    ('lecteur@agglo-riviera.fr','$argon2id$v=19$m=19456,t=2,p=1$MTz7aIPtJvTueF+dakcuXQ$NprTF6h0zutVJ9u3MkycUPqZevDEbix+ip/zpgyqFBk', 'Lucie Lecteur'),
    ('thomas@groupeindus.com',  '$argon2id$v=19$m=19456,t=2,p=1$MTz7aIPtJvTueF+dakcuXQ$NprTF6h0zutVJ9u3MkycUPqZevDEbix+ip/zpgyqFBk', 'Thomas Nguyen'),
    ('audit@groupeindus.com',   '$argon2id$v=19$m=19456,t=2,p=1$MTz7aIPtJvTueF+dakcuXQ$NprTF6h0zutVJ9u3MkycUPqZevDEbix+ip/zpgyqFBk', 'Aude Auditeur'),
    ('lea@cityair.app',         '$argon2id$v=19$m=19456,t=2,p=1$MTz7aIPtJvTueF+dakcuXQ$NprTF6h0zutVJ9u3MkycUPqZevDEbix+ip/zpgyqFBk', 'Léa Dubois');

-- memberships (triplet user × org × role ; + 1 user multi-org pour prouver l'asso n-aire)
INSERT INTO memberships (org_id, user_id, role_id)
SELECT o.id, u.id, r.id
FROM (VALUES
    ('agglo-riviera', 'sophie@agglo-riviera.fr',  'admin'),
    ('agglo-riviera', 'karim@agglo-riviera.fr',   'gestionnaire'),
    ('agglo-riviera', 'lecteur@agglo-riviera.fr', 'lecteur'),
    ('groupeindus',   'thomas@groupeindus.com',   'admin'),
    ('groupeindus',   'audit@groupeindus.com',    'lecteur'),
    ('cityair',       'lea@cityair.app',          'admin'),
    -- user multi-org : Thomas est aussi lecteur de l'Agglo Riviera
    ('agglo-riviera', 'thomas@groupeindus.com',   'lecteur')
) AS v(org_slug, email, role_code)
JOIN organizations o ON o.slug = v.org_slug
JOIN users u         ON u.email = v.email
JOIN roles r         ON r.code = v.role_code;

-- organization_subscriptions (Org A pro, B enterprise, C free) + 1 abo annulé (prouve l'index partiel)
INSERT INTO organization_subscriptions (org_id, plan_id, status, current_period_end)
SELECT o.id, sp.id, v.status, now() + interval '30 days'
FROM (VALUES
    ('agglo-riviera', 'pro',        'active'),
    ('groupeindus',   'enterprise', 'active'),
    ('cityair',       'free',       'active'),
    ('cityair',       'starter',    'canceled')   -- historique non-actif : autorisé par l'index partiel UNIQUE
) AS v(org_slug, plan_code, status)
JOIN organizations o        ON o.slug = v.org_slug
JOIN subscription_plans sp  ON sp.code = v.plan_code;

-- api_tokens (Org C : 1 active + 1 révoquée pour tester le 401)
INSERT INTO api_tokens (org_id, created_by, name, token_prefix, token_hash, scope, revoked_at)
SELECT o.id, u.id, v.name, v.prefix, encode(digest(v.secret,'sha256'),'hex'), 'read', v.revoked
FROM (VALUES
    ('cityair', 'lea@cityair.app', 'Clé prod CityAir',    'qrt_lea1', 'demo-secret-lea-active',  NULL::timestamptz),
    ('cityair', 'lea@cityair.app', 'Clé de test révoquée','qrt_lea0', 'demo-secret-lea-revoked', now() - interval '2 days')
) AS v(org_slug, email, name, prefix, secret, revoked)
JOIN organizations o ON o.slug = v.org_slug
JOIN users u         ON u.email = v.email;

-- --- Données métier (lieux, stations, règles, profils) -----------------------
INSERT INTO tracked_locations (org_id, name, description, created_by)
SELECT o.id, v.name, v.descr, u.id
FROM (VALUES
    ('agglo-riviera', 'École Jules-Ferry', 'Groupe scolaire — surveillance enfants',  'karim@agglo-riviera.fr'),
    ('agglo-riviera', 'Centre-ville',      'Hyper-centre, multi-stations',            'sophie@agglo-riviera.fr'),
    ('agglo-riviera', 'Axe A8',            'Échangeur autoroutier',                   'sophie@agglo-riviera.fr'),
    ('groupeindus',   'Site Lyon',         'Site industriel Lyon Part-Dieu',          'thomas@groupeindus.com'),
    ('groupeindus',   'Site Anvers',       'Site industriel Anvers (multi-pays)',     'thomas@groupeindus.com')
) AS v(org_slug, name, descr, email)
JOIN organizations o ON o.slug = v.org_slug
JOIN users u         ON u.email = v.email;

-- tracked_location_stations : « Centre-ville » agrège 2 stations (prouve le n-n), 1 primaire
INSERT INTO tracked_location_stations (tracked_location_id, ref_location_id, is_primary)
SELECT tl.id, rl.id, v.is_primary
FROM (VALUES
    ('École Jules-Ferry', 1001, true),
    ('Centre-ville',      1002, true),
    ('Centre-ville',      1001, false),   -- 2e station → agrégation multi-stations (règle MAX)
    ('Axe A8',            1004, true),
    ('Site Lyon',         2001, true),
    ('Site Anvers',       3001, true)
) AS v(loc_name, openaq_loc, is_primary)
JOIN tracked_locations tl ON tl.name = v.loc_name
JOIN ref_locations rl     ON rl.openaq_location_id = v.openaq_loc;

-- alert_rules (org_id dénormalisé pour l'isolation ; 1 inactive pour prouver le filtre Moka)
INSERT INTO alert_rules (org_id, tracked_location_id, parameter_id, comparator, threshold_value, severity, status, name, created_by)
SELECT tl.org_id, tl.id, p.id, v.cmp, v.thr, v.sev, v.status, v.name, u.id
FROM (VALUES
    ('École Jules-Ferry', 'pm25', '>',  15.0, 'warning',  'active',   'Seuil enfants PM2.5',     'karim@agglo-riviera.fr'),
    ('Centre-ville',      'no2',  '>=', 40.0, 'critical', 'active',   'Seuil NO2 réglementaire', 'sophie@agglo-riviera.fr'),
    ('Axe A8',            'o3',   '>',  120.0,'critical', 'inactive', 'Seuil O3 (désactivé)',    'sophie@agglo-riviera.fr'),
    ('Site Lyon',         'pm25', '>',  25.0, 'critical', 'active',   'PM2.5 site Lyon',         'thomas@groupeindus.com'),
    ('Site Anvers',       'pm25', '>',  25.0, 'warning',  'active',   'PM2.5 site Anvers',       'thomas@groupeindus.com')
) AS v(loc_name, param_code, cmp, thr, sev, status, name, email)
JOIN tracked_locations tl ON tl.name = v.loc_name
JOIN parameters p         ON p.code = v.param_code
JOIN users u              ON u.email = v.email;

-- alert_rule_recipients (teste le CHECK « exactement une cible »)
INSERT INTO alert_rule_recipients (alert_rule_id, channel, user_id, email)
SELECT ar.id, v.channel,
       (SELECT id FROM users WHERE email = v.user_email),
       v.ext_email
FROM (VALUES
    ('Seuil enfants PM2.5',     'websocket', 'karim@agglo-riviera.fr', NULL),
    ('Seuil enfants PM2.5',     'email',      NULL,                    'parents@ecole-julesferry.fr'),
    ('Seuil NO2 réglementaire', 'email',     'sophie@agglo-riviera.fr',NULL)
) AS v(rule_name, channel, user_email, ext_email)
JOIN alert_rules ar ON ar.name = v.rule_name;

-- alert_events : faits figés (snapshot complet), répartis Org A/B, sévérités mixées, sur le dernier trimestre
INSERT INTO alert_events (alert_rule_id, org_id, tracked_location_id, ref_location_id, openaq_location_id,
                          openaq_sensor_id, parameter_code, measured_value, unit, measured_at,
                          threshold_value, comparator, severity, fired_at, is_read)
SELECT ar.id, tl.org_id, tl.id, rl.id, rl.openaq_location_id, rs.openaq_sensor_id,
       v.pcode, v.mval, p.unit, v.mat::timestamptz, ar.threshold_value, ar.comparator, ar.severity, v.mat::timestamptz, v.is_read
FROM (VALUES
    ('Seuil enfants PM2.5',     'École Jules-Ferry', 1001, 'pm25', 22.5, '2026-05-12 09:00+02', false),
    ('Seuil enfants PM2.5',     'École Jules-Ferry', 1001, 'pm25', 31.0, '2026-05-28 08:00+02', false),
    ('Seuil NO2 réglementaire', 'Centre-ville',      1002, 'no2',  52.0, '2026-04-03 18:00+02', true),
    ('PM2.5 site Lyon',         'Site Lyon',         2001, 'pm25', 40.0, '2026-05-20 14:00+02', false),
    ('PM2.5 site Lyon',         'Site Lyon',         2001, 'pm25', 28.0, '2026-06-01 07:00+02', true),
    ('PM2.5 site Anvers',       'Site Anvers',       3001, 'pm25', 33.0, '2026-05-15 11:00+02', false)
) AS v(rule_name, loc_name, openaq_loc, pcode, mval, mat, is_read)
JOIN alert_rules ar       ON ar.name = v.rule_name
JOIN tracked_locations tl ON tl.name = v.loc_name
JOIN ref_locations rl     ON rl.openaq_location_id = v.openaq_loc
JOIN parameters p         ON p.code = v.pcode
JOIN ref_sensors rs       ON rs.ref_location_id = rl.id AND rs.parameter_id = p.id;
-- NB : « Site Anvers » a un event ; « Axe A8 » n'en a AUCUN → le ranking US-10 doit l'afficher à 0.

-- notification_deliveries (1 sent + 1 failed → découplage US-15)
INSERT INTO notification_deliveries (alert_event_id, recipient_id, channel, target, status, sent_at, attempts)
SELECT ae.id, arr.id, arr.channel, COALESCE(u.email::text, arr.email::text, arr.webhook_url), v.status,
       CASE WHEN v.status = 'sent' THEN ae.fired_at END, v.attempts
FROM alert_events ae
JOIN alert_rules ar           ON ar.id = ae.alert_rule_id
JOIN alert_rule_recipients arr ON arr.alert_rule_id = ar.id
LEFT JOIN users u             ON u.id = arr.user_id
JOIN (VALUES
    ('École Jules-Ferry', 'websocket', 'sent',   1),
    ('École Jules-Ferry', 'email',     'failed', 3)
) AS v(loc_name, channel, status, attempts)
  ON arr.channel = v.channel
JOIN tracked_locations tl ON tl.id = ae.tracked_location_id AND tl.name = v.loc_name
WHERE ae.parameter_code = 'pm25';

-- tracked_location_profiles : École Jules-Ferry × enfants, 8h-17h Lun-Ven (différenciateur US-04)
INSERT INTO tracked_location_profiles (tracked_location_id, exposure_profile_id, start_time, end_time, days_mask, timezone)
SELECT tl.id, ep.id, v.st::time, v.et::time, v.days, v.tz
FROM (VALUES
    ('École Jules-Ferry', 'enfants',      '08:00', '17:00', 31, 'Europe/Paris'),  -- 31 = Lun→Ven
    ('Site Anvers',       'asthmatiques', '08:00', '20:00', 127,'Europe/Brussels')
) AS v(loc_name, profile_code, st, et, days, tz)
JOIN tracked_locations tl ON tl.name = v.loc_name
JOIN exposure_profiles ep ON ep.code = v.profile_code AND ep.is_system;

-- exposure_results : 1 dose pré-calculée (snapshot de fenêtre pour reproductibilité — US-08)
INSERT INTO exposure_results (tracked_location_profile_id, parameter_id, period_start, period_end,
                              threshold_value, window_start_time, window_end_time, window_days_mask, timezone,
                              hours_over_threshold, sample_count)
SELECT tlp.id, p.id, DATE '2026-05-01', DATE '2026-05-31', et.threshold_value,
       tlp.start_time, tlp.end_time, tlp.days_mask, tlp.timezone, 3.0, 220
FROM tracked_location_profiles tlp
JOIN tracked_locations tl   ON tl.id = tlp.tracked_location_id AND tl.name = 'École Jules-Ferry'
JOIN exposure_profiles ep   ON ep.id = tlp.exposure_profile_id
JOIN parameters p           ON p.code = 'pm25'
JOIN exposure_thresholds et ON et.exposure_profile_id = ep.id AND et.parameter_id = p.id;

-- audit_log (alimente le trigger Jalon 3 + timeline ESG)
INSERT INTO audit_log (org_id, actor_user_id, entity_type, entity_id, action, diff)
SELECT ar.org_id, ar.created_by, 'alert_rule', ar.id, v.action, v.diff::jsonb
FROM (VALUES
    ('Seuil enfants PM2.5', 'create',     '{"threshold_value": 15.0}'),
    ('Seuil O3 (désactivé)','deactivate', '{"status": {"from": "active", "to": "inactive"}}')
) AS v(rule_name, action, diff)
JOIN alert_rules ar ON ar.name = v.rule_name;

COMMIT;

-- --- Contrôles rapides (décommenter pour vérifier après exécution) -----------
-- SELECT count(*) AS n_tables_seedees FROM (
--   SELECT 1 FROM organizations UNION ALL SELECT 1 FROM users UNION ALL SELECT 1 FROM alert_events) t;
-- SELECT o.name, count(ae.*) AS events FROM organizations o
--   LEFT JOIN alert_events ae ON ae.org_id = o.id GROUP BY o.name ORDER BY 1;
