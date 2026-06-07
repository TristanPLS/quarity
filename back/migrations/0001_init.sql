-- =============================================================================
-- Quarity — Migration 0001 : schéma OLTP PostgreSQL 16 (GELÉ — ne plus modifier)
-- =============================================================================
-- Source de vérité métier (B2B/B2G) : organisations, users, RBAC, abonnements,
-- lieux suivis, règles d'alerte, profils d'exposition, faits d'alerte figés.
-- Les mesures OpenAQ (séries temporelles) vivent dans ClickHouse — voir
-- db/clickhouse/01_schema.sql et la frontière inter-bases dans docs/data-model.md.
--
-- Appliquée au démarrage du back par sqlx::migrate! (cf. back/src/main.rs).
-- Toute évolution du schéma = NOUVELLE migration back/migrations/000N_*.sql
-- (sqlx vérifie le checksum des migrations déjà appliquées : modifier ce fichier
-- après application casserait le boot).
--
-- Conventions :
--   * PK métier        : BIGINT GENERATED ALWAYS AS IDENTITY
--   * PK référentiels  : INT GENERATED ALWAYS AS IDENTITY (faible cardinalité)
--   * Horodatages      : TIMESTAMPTZ (UTC), created_at/updated_at par défaut now()
--   * Énums            : CHECK textuels (souplesse d'évolution) sauf RBAC (table roles)
--   * 3NF visée partout, dénormalisations de snapshot/confort explicitement tracées
--
-- Idempotente (IF NOT EXISTS partout, AUCUN DROP) : sur un volume Postgres
-- existant où les tables ont déjà été créées par l'ancien initdb.d, cette
-- migration passe en no-op et est simplement enregistrée dans _sqlx_migrations.
-- =============================================================================

-- --- Extensions ---------------------------------------------------------------
CREATE EXTENSION IF NOT EXISTS citext;     -- emails / slugs insensibles à la casse
CREATE EXTENSION IF NOT EXISTS pgcrypto;   -- digests pour le seed (hash de tokens)


-- =============================================================================
-- 1. TENANT & RBAC
-- =============================================================================

-- 1. organizations — tenant racine (isolation multi-tenant)
CREATE TABLE IF NOT EXISTS organizations (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name        TEXT        NOT NULL CHECK (length(trim(name)) > 0),
    slug        CITEXT      NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9-]{2,64}$'),
    segment     TEXT        CHECK (segment IN ('B2G','B2B','B2B2C')),
    deleted_at  TIMESTAMPTZ,                         -- soft-delete (protège les faits d'alerte, cf. alert_events)
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
COMMENT ON TABLE organizations IS 'Tenant racine. Suppression logique (deleted_at) recommandée : les alert_events référencent org_id en ON DELETE RESTRICT.';

-- 2. users — comptes (login Argon2id, US-14). Multi-org via memberships (aucun org_id ici → 3NF).
CREATE TABLE IF NOT EXISTS users (
    id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    email          CITEXT      NOT NULL UNIQUE CHECK (email ~ '^[^@]+@[^@]+\.[^@]+$'),
    password_hash  TEXT        NOT NULL CHECK (password_hash LIKE '$argon2id$%'),  -- US-14 : Argon2id imposé, jamais bcrypt/clair
    full_name      TEXT        NOT NULL,
    is_active      BOOLEAN     NOT NULL DEFAULT true,
    last_login_at  TIMESTAMPTZ,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 3. roles — référentiel RBAC (admin / gestionnaire / lecteur)
CREATE TABLE IF NOT EXISTS roles (
    id              INT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    code            TEXT    NOT NULL UNIQUE CHECK (code IN ('admin','gestionnaire','lecteur')),
    label           TEXT    NOT NULL,
    can_manage_org  BOOLEAN NOT NULL DEFAULT false,
    can_write       BOOLEAN NOT NULL DEFAULT false   -- US-09 c3 : lecteur=false → 403 sur mutation
);

-- 4. memberships — association n-aire user × org × role (US-09)
CREATE TABLE IF NOT EXISTS memberships (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    org_id      BIGINT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id     BIGINT NOT NULL REFERENCES users(id)         ON DELETE CASCADE,
    role_id     INT    NOT NULL REFERENCES roles(id)         ON DELETE RESTRICT,
    invited_by  BIGINT          REFERENCES users(id)         ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_membership_user_org UNIQUE (org_id, user_id)  -- un seul rôle par couple (user, org) — US-09 c2
);
CREATE INDEX IF NOT EXISTS idx_memberships_user ON memberships (user_id);  -- résolution rapide « mes orgs » (isolation US-09 c4)
CREATE INDEX IF NOT EXISTS idx_memberships_org  ON memberships (org_id);


-- =============================================================================
-- 2. BILLING / API
-- =============================================================================

-- 5. subscription_plans — catalogue (le quota vit ICI, jamais dupliqué sur l'abo → 3NF)
CREATE TABLE IF NOT EXISTS subscription_plans (
    id                     INT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    code                   TEXT    NOT NULL UNIQUE CHECK (code IN ('free','starter','pro','enterprise')),
    name                   TEXT    NOT NULL,
    monthly_request_quota  INTEGER NOT NULL CHECK (monthly_request_quota >= 0),  -- quota clé API (US-12). 0 = illimité (cf. enterprise)
    rate_limit_per_min     INTEGER NOT NULL CHECK (rate_limit_per_min >= 0),     -- token bucket Redis
    max_tracked_locations  INTEGER,                                              -- NULL = illimité
    max_members            INTEGER,
    price_cents            INTEGER NOT NULL DEFAULT 0,                            -- modélisé seulement (pas de Stripe — pitch §5)
    is_active              BOOLEAN NOT NULL DEFAULT true
);

-- 6. organization_subscriptions — abo d'une org (≤ 1 actif, décision 7)
CREATE TABLE IF NOT EXISTS organization_subscriptions (
    id                  BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    org_id              BIGINT NOT NULL REFERENCES organizations(id)      ON DELETE CASCADE,
    plan_id             INT    NOT NULL REFERENCES subscription_plans(id) ON DELETE RESTRICT,
    status              TEXT   NOT NULL CHECK (status IN ('active','trialing','past_due','canceled')),
    started_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    current_period_end  TIMESTAMPTZ,
    canceled_at         TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- décision 7 / US-12 c4 : au plus 1 abonnement actif par org, tout en gardant l'historique
CREATE UNIQUE INDEX IF NOT EXISTS uq_org_active_subscription
    ON organization_subscriptions (org_id) WHERE status = 'active';

-- 7. api_tokens — clés API hashées, lecture seule, révocables (US-11)
CREATE TABLE IF NOT EXISTS api_tokens (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    org_id        BIGINT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    created_by    BIGINT          REFERENCES users(id)         ON DELETE SET NULL,
    name          TEXT   NOT NULL,
    token_prefix  TEXT   NOT NULL,                       -- 8 1ers car. en clair pour l'affichage/lookup (ex 'qrt_a1b2')
    token_hash    TEXT   NOT NULL UNIQUE,                -- hash du secret ; JAMAIS le secret en clair (US-11 c2)
    scope         TEXT   NOT NULL DEFAULT 'read'
                         CHECK (scope IN ('read','read_write')),  -- US-11 : lecture seule émise ; 'read_write' = point d'extension
    last_used_at  TIMESTAMPTZ,
    expires_at    TIMESTAMPTZ,
    revoked_at    TIMESTAMPTZ,                           -- révocation immédiate → 401 (US-11 c3)
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_api_tokens_org    ON api_tokens (org_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_api_tokens_active ON api_tokens (token_hash) WHERE revoked_at IS NULL;  -- lookup auth des clés vives


-- =============================================================================
-- 3. RÉFÉRENTIEL AIR & OpenAQ
-- =============================================================================

-- 8. parameters — polluants (sert aussi d'allowlist anti-injection ClickHouse, US-06 c4)
CREATE TABLE IF NOT EXISTS parameters (
    id                   INT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    code                 TEXT NOT NULL UNIQUE CHECK (code IN ('pm25','pm10','no2','o3','so2','co')),
    display_name         TEXT NOT NULL,
    unit                 TEXT NOT NULL CHECK (unit IN ('µg/m³','mg/m³','ppm','ppb')),  -- unité canonique du polluant (référence unique)
    openaq_parameter_id  INTEGER UNIQUE,
    description          TEXT
);

-- 9. ref_locations — stations OpenAQ (décision 8 : PK surrogate + clé naturelle UNIQUE)
CREATE TABLE IF NOT EXISTS ref_locations (
    id                  BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    openaq_location_id  BIGINT  NOT NULL UNIQUE,                      -- clé naturelle OpenAQ (découplée de la PK)
    name                TEXT    NOT NULL,
    country             CHAR(2) NOT NULL,                            -- ISO-3166-1 alpha-2 (miroir LowCardinality(country) CH)
    city                TEXT,
    latitude            DOUBLE PRECISION NOT NULL CHECK (latitude  BETWEEN -90  AND 90),
    longitude           DOUBLE PRECISION NOT NULL CHECK (longitude BETWEEN -180 AND 180),
    timezone            TEXT,                                        -- IANA, ex 'Europe/Paris'
    is_active           BOOLEAN NOT NULL DEFAULT true,
    first_seen_at       TIMESTAMPTZ,
    last_seen_at        TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_ref_locations_country ON ref_locations (country);

-- 10. ref_sensors — capteurs OpenAQ (1 capteur = 1 station × 1 polluant)
CREATE TABLE IF NOT EXISTS ref_sensors (
    id                BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    openaq_sensor_id  BIGINT NOT NULL UNIQUE,                        -- clé naturelle OpenAQ
    ref_location_id   BIGINT NOT NULL REFERENCES ref_locations(id) ON DELETE CASCADE,
    parameter_id      INT    NOT NULL REFERENCES parameters(id)    ON DELETE RESTRICT,
    is_active         BOOLEAN NOT NULL DEFAULT true,
    last_value        NUMERIC(12,4),                                -- dénormalisation de CONFORT (vérité = ClickHouse)
    last_measured_at  TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_sensor_location_param UNIQUE (ref_location_id, parameter_id)
);
CREATE INDEX IF NOT EXISTS idx_ref_sensors_location ON ref_sensors (ref_location_id);

-- 11. aqi_categories — référentiel des 6 niveaux EPA (décision 6 + revue 3NF)
--     Verrouille couleur↔libellé↔catégorie par construction (identity.md). aqi_breakpoints y fait FK.
CREATE TABLE IF NOT EXISTS aqi_categories (
    category   SMALLINT PRIMARY KEY CHECK (category BETWEEN 1 AND 6),
    label      TEXT     NOT NULL,                                   -- libellé FR (accessibilité US-05)
    color_hex  CHAR(7)  NOT NULL CHECK (color_hex ~ '^#[0-9A-Fa-f]{6}$')  -- couleur EPA exacte (identity.md)
);

-- 12. aqi_breakpoints — paliers EPA par polluant × fenêtre (interpolation linéaire, décision 6)
CREATE TABLE IF NOT EXISTS aqi_breakpoints (
    id                INT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    parameter_id      INT      NOT NULL REFERENCES parameters(id)     ON DELETE CASCADE,
    category          SMALLINT NOT NULL REFERENCES aqi_categories(category) ON DELETE RESTRICT,
    averaging_period  TEXT     NOT NULL CHECK (averaging_period IN ('1h','8h','24h','annual')),
    unit              TEXT     NOT NULL CHECK (unit IN ('µg/m³','mg/m³','ppm','ppb')),  -- unité des conc_low/high (revue : verrou d'unité AQI)
    conc_low          NUMERIC(10,3) NOT NULL CHECK (conc_low >= 0),
    conc_high         NUMERIC(10,3) NOT NULL CHECK (conc_high > conc_low),
    aqi_low           INTEGER  NOT NULL CHECK (aqi_low  >= 0),
    aqi_high          INTEGER  NOT NULL CHECK (aqi_high > aqi_low),
    CONSTRAINT uq_breakpoint UNIQUE (parameter_id, averaging_period, category)
);
-- Formule (Jalon 3) : AQI = (aqi_high-aqi_low)/(conc_high-conc_low) * (C-conc_low) + aqi_low,
-- pour le palier tel que conc_low <= C <= conc_high sur (parameter, averaging_period), C exprimé en `unit`.
COMMENT ON TABLE aqi_breakpoints IS 'Paliers AQI EPA. unit = unité des paliers EPA (peut différer de l''unité OpenAQ canonique du polluant : O3/NO2 en ppb) ; toute comparaison mesure↔palier exige une conversion préalable — cf. docs/foundations.md décision D4.2.';


-- =============================================================================
-- 4. MÉTIER — LIEUX SUIVIS
-- =============================================================================

-- 13. tracked_locations — lieux suivis d'une org (US-01)
CREATE TABLE IF NOT EXISTS tracked_locations (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    org_id      BIGINT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name        TEXT   NOT NULL CHECK (length(trim(name)) > 0),
    description TEXT,
    is_active   BOOLEAN NOT NULL DEFAULT true,
    created_by  BIGINT          REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_tracked_location_org_name UNIQUE (org_id, name)
);
CREATE INDEX IF NOT EXISTS idx_tracked_locations_org ON tracked_locations (org_id);
-- Invariant « ≥ 1 station liée » (US-01 c2/c4, HTTP 422) : NON exprimable par FK → trigger/procédure Jalon 3 (voir §Triggers).

-- 14. tracked_location_stations — association n-n lieu ↔ stations OpenAQ (décision 4, US-01)
CREATE TABLE IF NOT EXISTS tracked_location_stations (
    id                   BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    tracked_location_id  BIGINT  NOT NULL REFERENCES tracked_locations(id) ON DELETE CASCADE,
    ref_location_id      BIGINT  NOT NULL REFERENCES ref_locations(id)     ON DELETE RESTRICT,
    is_primary           BOOLEAN NOT NULL DEFAULT false,  -- station de référence (revue : règle d'agrégation multi-stations)
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_tls UNIQUE (tracked_location_id, ref_location_id)
);
CREATE INDEX IF NOT EXISTS idx_tls_location ON tracked_location_stations (tracked_location_id);
CREATE INDEX IF NOT EXISTS idx_tls_station  ON tracked_location_stations (ref_location_id);
-- Au plus 1 station primaire par lieu :
CREATE UNIQUE INDEX IF NOT EXISTS uq_tls_primary ON tracked_location_stations (tracked_location_id) WHERE is_primary;
-- Règle d'agrégation multi-stations (alerte/AQI/moyennes) : MAX par polluant (principe de précaution) — voir docs/data-model.md.


-- =============================================================================
-- 5. ALERTING
-- =============================================================================

-- 15. alert_rules — règle de seuil par polluant (US-02). org_id dénormalisé pour l'isolation (revue couverture).
CREATE TABLE IF NOT EXISTS alert_rules (
    id                   BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    org_id               BIGINT NOT NULL REFERENCES organizations(id)    ON DELETE CASCADE,  -- isolation directe US-09/US-16, ranking US-10
    tracked_location_id  BIGINT NOT NULL REFERENCES tracked_locations(id) ON DELETE CASCADE,
    parameter_id         INT    NOT NULL REFERENCES parameters(id)        ON DELETE RESTRICT,
    comparator           TEXT   NOT NULL CHECK (comparator IN ('>','>=')),     -- US-02 c1
    threshold_value      NUMERIC(12,4) NOT NULL CHECK (threshold_value >= 0),  -- US-02 c2 (unité = parameters.unit, dérivée par jointure)
    severity             TEXT   NOT NULL DEFAULT 'warning' CHECK (severity IN ('info','warning','critical')),
    status               TEXT   NOT NULL DEFAULT 'active'  CHECK (status IN ('active','inactive')),  -- US-02 c3/c4
    name                 TEXT,
    created_by           BIGINT          REFERENCES users(id) ON DELETE SET NULL,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_alert_rule UNIQUE (tracked_location_id, parameter_id, comparator, threshold_value)
);
-- Seules les règles ACTIVES sont chargées dans Moka (US-02 c4, hot path) :
CREATE INDEX IF NOT EXISTS idx_alert_rules_active ON alert_rules (tracked_location_id, parameter_id) WHERE status = 'active';
CREATE INDEX IF NOT EXISTS idx_alert_rules_org    ON alert_rules (org_id);
-- Cohérence org_id = tracked_locations.org_id : trigger Jalon 3 (voir §Triggers).

-- 16. alert_rule_recipients — association n-aire règle × destinataire (US-15)
CREATE TABLE IF NOT EXISTS alert_rule_recipients (
    id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    alert_rule_id  BIGINT NOT NULL REFERENCES alert_rules(id) ON DELETE CASCADE,
    channel        TEXT   NOT NULL CHECK (channel IN ('email','websocket','webhook')),
    user_id        BIGINT          REFERENCES users(id) ON DELETE CASCADE,           -- destinataire interne
    email          CITEXT CHECK (email IS NULL OR email ~ '^[^@]+@[^@]+\.[^@]+$'),    -- destinataire externe
    webhook_url    TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_recipient_one_target CHECK (num_nonnulls(user_id, email, webhook_url) = 1)  -- exactement une cible
);
-- Anti-doublon par cible (index partiels, sinon NULLs distincts neutralisent l'unicité) :
CREATE UNIQUE INDEX IF NOT EXISTS uq_arr_user    ON alert_rule_recipients (alert_rule_id, user_id)     WHERE user_id     IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS uq_arr_email   ON alert_rule_recipients (alert_rule_id, email)       WHERE email       IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS uq_arr_webhook ON alert_rule_recipients (alert_rule_id, webhook_url) WHERE webhook_url IS NOT NULL;
-- Invariant « destinataire ∈ org de la règle » (US-15 c4, isolation) : trigger Jalon 3 (voir §Triggers).

-- 17. alert_events — SNAPSHOT figé immuable de la mesure déclencheuse (décision 5, US-03/US-10)
--     Les colonnes snapshot survivent à la purge TTL 90j de ClickHouse.
CREATE TABLE IF NOT EXISTS alert_events (
    id                  BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    alert_rule_id       BIGINT          REFERENCES alert_rules(id)      ON DELETE SET NULL,  -- l'event survit à la suppression de la règle
    org_id              BIGINT NOT NULL REFERENCES organizations(id)    ON DELETE RESTRICT,  -- revue : PAS de CASCADE (préserve l'audit)
    tracked_location_id BIGINT          REFERENCES tracked_locations(id) ON DELETE SET NULL,
    -- ---- snapshot immuable (copié au moment du match ; aucun FK vers les dimensions volatiles) ----
    ref_location_id     BIGINT NOT NULL,                 -- ref_locations.id figé (surrogate) — pas de FK (survit à la désactivation station)
    openaq_location_id  BIGINT NOT NULL,                 -- clé naturelle OpenAQ figée (cohérente CH measurements.location_id)
    openaq_sensor_id    BIGINT,                          -- capteur déclencheur figé (revue : défendabilité audit multi-capteurs)
    parameter_code      TEXT   NOT NULL,                 -- ex 'pm25'
    measured_value      NUMERIC(12,4) NOT NULL,
    unit                TEXT   NOT NULL,
    measured_at         TIMESTAMPTZ   NOT NULL,
    threshold_value     NUMERIC(12,4) NOT NULL,
    comparator          TEXT   NOT NULL CHECK (comparator IN ('>','>=')),
    severity            TEXT   NOT NULL CHECK (severity IN ('info','warning','critical')),
    -- ---- état mutable autorisé ----
    fired_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    is_read             BOOLEAN     NOT NULL DEFAULT false,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_alert_events_org_fired ON alert_events (org_id, fired_at DESC);
CREATE INDEX IF NOT EXISTS idx_alert_events_unread    ON alert_events (org_id, fired_at) WHERE is_read = false;   -- index partiel non-trivial (Jalon 3)
CREATE INDEX IF NOT EXISTS idx_alert_events_ranking   ON alert_events (org_id, tracked_location_id, severity, fired_at);  -- ranking US-10
COMMENT ON TABLE alert_events IS 'Fait historique figé (snapshot). Colonnes snapshot immuables : trigger BEFORE UPDATE Jalon 3. Seuls is_read sont mutables.';

-- 18. notification_deliveries — traçabilité des envois, découplée de l'event (US-15)
CREATE TABLE IF NOT EXISTS notification_deliveries (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    alert_event_id  BIGINT NOT NULL REFERENCES alert_events(id)          ON DELETE CASCADE,
    recipient_id    BIGINT          REFERENCES alert_rule_recipients(id) ON DELETE SET NULL,
    channel         TEXT   NOT NULL CHECK (channel IN ('email','websocket','webhook')),
    target          TEXT   NOT NULL,
    status          TEXT   NOT NULL DEFAULT 'pending'
                           CHECK (status IN ('pending','sent','failed','retrying')),  -- US-15 c2
    error_message   TEXT,
    attempts        SMALLINT NOT NULL DEFAULT 0,
    sent_at         TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_notif_event  ON notification_deliveries (alert_event_id);
CREATE INDEX IF NOT EXISTS idx_notif_status ON notification_deliveries (status) WHERE status IN ('pending','failed','retrying');


-- =============================================================================
-- 6. EXPOSITION (différenciateur produit)
-- =============================================================================

-- 19. exposure_profiles — profils de population sensible (US-04). org_id NULL = profil système partagé.
CREATE TABLE IF NOT EXISTS exposure_profiles (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    org_id      BIGINT REFERENCES organizations(id) ON DELETE CASCADE,   -- NULL = global/préfabriqué ; non-NULL = custom d'une org
    code        TEXT   NOT NULL CHECK (code IN ('enfants','asthmatiques','personnes_agees','sportifs','general')),
    name        TEXT   NOT NULL,
    description TEXT,
    is_system   BOOLEAN NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- NULLS NOT DISTINCT (PG15+) : empêche deux profils système (org_id NULL) de même code
    CONSTRAINT uq_exposure_profile_org_code UNIQUE NULLS NOT DISTINCT (org_id, code)
);

-- 20. exposure_thresholds — seuils adaptés par profil × polluant (US-04). Unité = parameters.unit (dérivée).
CREATE TABLE IF NOT EXISTS exposure_thresholds (
    id                   BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    exposure_profile_id  BIGINT NOT NULL REFERENCES exposure_profiles(id) ON DELETE CASCADE,
    parameter_id         INT    NOT NULL REFERENCES parameters(id)        ON DELETE RESTRICT,
    threshold_value      NUMERIC(12,4) NOT NULL CHECK (threshold_value >= 0),
    averaging_period     TEXT   NOT NULL DEFAULT '1h' CHECK (averaging_period IN ('1h','8h','24h','annual')),
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_exposure_threshold UNIQUE (exposure_profile_id, parameter_id, averaging_period)
);

-- 21. tracked_location_profiles — assoc. n-aire lieu × profil + plage horaire (décision 10, US-04)
CREATE TABLE IF NOT EXISTS tracked_location_profiles (
    id                   BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    tracked_location_id  BIGINT   NOT NULL REFERENCES tracked_locations(id) ON DELETE CASCADE,
    exposure_profile_id  BIGINT   NOT NULL REFERENCES exposure_profiles(id) ON DELETE RESTRICT,
    start_time           TIME     NOT NULL,
    end_time             TIME     NOT NULL,
    days_mask            SMALLINT NOT NULL CHECK (days_mask BETWEEN 1 AND 127),  -- bitmask : bit0=Lun … bit6=Dim ; ≥1 ⇒ jamais vide (US-04 c4)
    timezone             TEXT     NOT NULL DEFAULT 'Europe/Paris',
    is_active            BOOLEAN  NOT NULL DEFAULT true,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_tlp_time_window CHECK (end_time > start_time),                -- plage invalide refusée (US-04 c4)
    CONSTRAINT uq_tlp UNIQUE (tracked_location_id, exposure_profile_id)          -- Jalon 1 : 1 plage intra-journée par (lieu, profil)
);
CREATE INDEX IF NOT EXISTS idx_tlp_location ON tracked_location_profiles (tracked_location_id);

-- 22. exposure_results — cache des doses calculées (US-08). Snapshot de la fenêtre pour reproductibilité (revue).
CREATE TABLE IF NOT EXISTS exposure_results (
    id                           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    tracked_location_profile_id  BIGINT NOT NULL REFERENCES tracked_location_profiles(id) ON DELETE CASCADE,
    parameter_id                 INT    NOT NULL REFERENCES parameters(id)                ON DELETE RESTRICT,
    period_start                 DATE   NOT NULL,
    period_end                   DATE   NOT NULL CHECK (period_end >= period_start),
    threshold_value              NUMERIC(12,4) NOT NULL,           -- seuil appliqué figé (US-08 c3)
    -- snapshot de la fenêtre ayant servi au calcul (résultat exportable auto-portant) :
    window_start_time            TIME     NOT NULL,
    window_end_time              TIME     NOT NULL,
    window_days_mask             SMALLINT NOT NULL,
    timezone                     TEXT     NOT NULL,
    hours_over_threshold         NUMERIC(8,2) NOT NULL CHECK (hours_over_threshold >= 0),
    sample_count                 INTEGER NOT NULL DEFAULT 0,
    computed_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_exposure_result UNIQUE (tracked_location_profile_id, parameter_id, period_start, period_end)
);
COMMENT ON TABLE exposure_results IS 'Cache (décision 10). Vérité du calcul = ClickHouse (compute_exposure_dose, Jalon 3). Une modif de plage du tlp invalide ce cache.';


-- =============================================================================
-- 7. TRANSVERSE
-- =============================================================================

-- 23. audit_log — journal d'audit générique (trigger Jalon 3 sur alert_rules + auditabilité ESG US-10)
CREATE TABLE IF NOT EXISTS audit_log (
    id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    org_id         BIGINT REFERENCES organizations(id) ON DELETE CASCADE,
    actor_user_id  BIGINT REFERENCES users(id)         ON DELETE SET NULL,
    entity_type    TEXT   NOT NULL,
    entity_id      BIGINT,
    action         TEXT   NOT NULL CHECK (action IN ('create','update','delete','activate','deactivate')),
    diff           JSONB,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_audit_org_created ON audit_log (org_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_entity      ON audit_log (entity_type, entity_id);


-- =============================================================================
-- 8. INVARIANTS HORS-SCHÉMA → TRIGGERS & PROCÉDURES (à livrer au JALON 3)
-- =============================================================================
-- Ces règles métier ne sont pas exprimables par contrainte déclarative ; elles
-- sont documentées ici et implémentées au Jalon 3 (cf. docs/data-model.md §Invariants).
--
--  T1  tracked_location_stations : refuser la suppression de la dernière station
--      d'un lieu ACTIF (gardien de l'invariant « ≥ 1 station », US-01 c2/c4).
--  T2  alert_rules : forcer org_id = tracked_locations.org_id (cohérence d'isolation).
--  T3  alert_rule_recipients : le user destinataire doit avoir un membership dans
--      l'org de la règle (US-15 c4 — règle d'isolation cross-tenant).
--  T4  alert_events : trigger BEFORE UPDATE rejetant toute modification des colonnes
--      snapshot (immuabilité du fait d'audit, décision 5). Seul is_read reste mutable.
--  T5  audit auto sur alert_rules (INSERT/UPDATE/activate/deactivate → audit_log) — roadmap l.130.
--  T6  compteur dénormalisé d'alertes non-lues par org (roadmap l.130).
--
--  Procédures : create_tracked_location_with_rules (point d'entrée garantissant ≥1 station + règles),
--               archive_old_alert_events, compute_exposure_dose (interroge ClickHouse → exposure_results).
-- =============================================================================
