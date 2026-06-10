# Quarity - Roadmap produit

**Stack** : Rust/Axum · React+Vite · Postgres 16 · ClickHouse 24.x · Redis 7 · Moka (cache L1 Rust) · Docker Compose
**Source de données** : OpenAQ (mesures de qualité de l'air mondiale — capteurs, fréquence ~horaire)
**Cible** : collectivités, autorités sanitaires, établissements (écoles, hôpitaux), grands comptes (ESG/QSE), applis citoyennes — **B2B/B2G**

> 📌 **Avancement** : ce document est le *plan*. L'avancement réel fait foi dans [docs/backlog.md](docs/backlog.md).

---

## Pitchs techniques à connaître (justification d'architecture)

### Pourquoi ClickHouse (vs Cassandra/Scylla, dans la famille colonnes)

1. **Pattern de requêtes** = agrégations sur dimensions arbitraires (date × ville × polluant × capteur). Cassandra exige de connaître les partition keys à l'avance ; ClickHouse agrège nativement sur n'importe quelle colonne grâce au stockage column-wise.
2. **Compression** : sur colonnes basse-cardinalité (`parameter`, `country`, `location_id`), ClickHouse compresse 10-50× mieux que Cassandra (`LowCardinality` + codecs `DoubleDelta`/`Gorilla`/`ZSTD`). Les valeurs de capteurs (flottants lisses) compriment idéalement avec `Gorilla`.
3. **Modèle d'écriture** : OpenAQ pousse des mesures par batches périodiques, pas des millions d'updates/seconde sur clé connue. Cassandra est sur-dimensionnée ; ClickHouse correspond exactement au profil batch + analytics ad-hoc.

### Pourquoi ClickHouse compte comme NoSQL

- **Stockage physique column-wise** (un fichier par colonne, MergeTree append-only) — inverse exact d'un B-tree row-oriented type Postgres.
- **Garanties BASE** : pas de FK appliquées, pas d'UPDATE/DELETE transactionnels (mutations asynchrones), `ReplacingMergeTree` en eventual consistency.
- **CAP-AP** : sharding/réplication natifs orientés scale-out horizontal.

SQL n'est qu'une interface (Cassandra a CQL, Hive a HiveQL). La distinction wide-column vs columnar reste à l'intérieur de la famille colonnes.

### Pourquoi le polyglotte (et pas une seule base)

- **Columnar (ClickHouse) = cas d'école des séries temporelles denses** : partitionnement par temps, downsampling, rollups (moyennes horaires/journalières), fenêtres glissantes réglementaires. C'est exactement le besoin des mesures de capteurs.
- **Document** (Mongo…) : inadapté aux séries temporelles denses (pas de compression columnar, agrégations lentes sur gros volumes).
- **Clé-valeur** (pur Redis comme store) : aucune agrégation temporelle native.
- **Graphe** (Neo4j…) : aucun sens ici, les mesures ne forment pas un réseau de relations.
- **Relationnel (Postgres)** : indispensable pour le **métier** (orgs, users, règles, profils d'exposition, abonnements) — contraintes fortes, transactions ACID, 3NF.

### Pourquoi Moka **et** Redis (pas l'un ou l'autre)

- **Moka (L1, in-process)** : les règles d'alerte (`alert_rules`) activées sont compilées en mémoire et rechargées à chaque batch d'ingestion. La boucle de matching évalue chaque mesure contre les règles concernées par lookup mémoire pur, **sous 5 ms**. Pas de réseau.
- **Redis (L2, distribué)** : sessions JWT, refresh tokens, rate-limit (token bucket par IP + user, et quota par clé API), **pub/sub pour push WebSocket** (un dépassement de seuil → publication sur channel → broadcast aux clients connectés). Ce que Moka ne peut pas faire.

Le hot path qui bénéficie **spécifiquement** de Moka : la boucle de matching de seuils (relue à chaque batch sur potentiellement des milliers de règles × mesures, doit rester sous 5 ms par mesure).

---

## Jalon 0 — Cadrage (setup)

**Avant toute ligne de code applicatif.**

- [X] Rôles répartis : lead BDD (Tristan), lead back, lead front, lead conception/UX-doc
- [X] Stack arrêtée et écrite dans le README
- [X] Repo Git créé : branches `master` (protégée), `dev` (protégée), conventions `feature/*`, `fix/*`, `chore/*`, `docs/*` *(protections appliquées le 2026-06-06, ajustées le jour même au process solo : PR obligatoire + CI verte — 0 review humaine, pas de force-push)*
- [X] `.gitignore` propre (pas de `.env`, `target/`, `node_modules/`, données OpenAQ téléchargées)
- [X] Conventional Commits documenté (README + CONTRIBUTING + templates PR/issue)
- [X] Règle : pas de commit direct sur `master` ni `dev`, PR obligatoire avec CI verte *(process solo acté le 2026-06-06 — la CI remplace la review humaine)*
- [X] Board ouvert (GitHub Projects), colonnes : Backlog / À faire / En cours / À valider / Problématique / Terminé — https://github.com/users/TristanPLS/projects/1
- [X] Pitch produit rédigé (`docs/pitch.md`)
- [X] Nom du produit + identité visuelle de base (`docs/identity.md`)
- [X] **`AGENTS.md` rédigé** (charte agents : zéro action Git, logging dans `logs/`)
- [X] **Dossier `logs/` initialisé** (`logs/README.md` : convention + format)

**Livrable** : repo initialisé, board peuplé, pitch + identité + charte agents écrits.

---

## Jalon 1 — Modélisation

**La phase la plus rentable du projet.**

### Personas & user stories

- [X] 4-5 personas (responsable environnement/santé d'une collectivité, direction d'école/périscolaire, gestionnaire hospitalier/épidémio, analyste ESG, dev appli citoyenne)
- [X] 12-15 user stories format `En tant que [rôle], je veux [action] afin de [bénéfice]` avec critères d'acceptation
- [X] Priorisation MoSCoW (Must / Should / Could / Won't)

### Postgres (MCD → MLD → script)

Tables minimales : `organizations`, `users`, `roles`, `memberships`, `subscription_plans`, `organization_subscriptions`, `api_tokens`, `parameters`, `ref_locations`, `ref_sensors`, `aqi_breakpoints`, `tracked_locations`, `alert_rules`, `alert_rule_recipients`, `alert_events`, `notification_deliveries`, `exposure_profiles`, `exposure_thresholds`, `tracked_location_profiles`.

- [X] MCD propre en 3NF (au moins)
- [X] Au moins une association n-aire gérée proprement (`memberships` user × org × rôle ; `tracked_location_profiles` lieu × profil d'exposition)
- [X] Cardinalités vérifiées deux fois
- [X] `db/sql/01_schema.sql` + `db/sql/02_seed.sql` versionnés
- [X] Le seed contient assez de données pour tester chaque vue/trigger/procédure

### ClickHouse

- [X] Table `measurements` partitionnée par mois (`PARTITION BY toYYYYMM(measured_at)`)
- [X] `ORDER BY (location_id, parameter, measured_at)`
- [X] `LowCardinality(String)` sur `parameter` / `country` / `location_id` (gain compression énorme)
- [X] Codecs explicites : `CODEC(DoubleDelta, ZSTD)` sur timestamps, `CODEC(Gorilla, ZSTD)` sur les valeurs
- [X] 1-2 `MATERIALIZED VIEW` pour rollups (`measurements_hourly`, `measurements_daily` en `AggregatingMergeTree` — downsampling)
- [X] Index `MinMax` sur date, `bloom_filter` sur `location_id`
- [X] Plan de TTL : mesures brutes > 90 jours purgées, rollups conservés 2-5 ans

### Frontière inter-bases (à documenter)

- Postgres = source de vérité OLTP (users, orgs, lieux suivis, règles, profils d'exposition, abos)
- ClickHouse = source de vérité analytics (mesures OpenAQ, time-series)
- Alerte déclenchée = écriture dans `alert_events` (Postgres) + push Redis pub/sub
- **Dénormalisation contrôlée** (cf. docs/data-model.md §D.2). ClickHouse n'a aucune FK vers Postgres, seulement les clés naturelles OpenAQ (`location_id`/`sensor_id`).

### Wireframes

- [ ] 6-7 écrans basse-fi : login, dashboard (carte + AQI), lieux suivis, règles d'alerte, détail d'un lieu (séries temporelles), profils d'exposition, profil/org

**Livrable** : MCD + MLD validés, scripts SQL qui tournent en local, schéma ClickHouse écrit, wireframes faits.

---

## Jalon 2 — Walking skeleton (MVP)

**Le tuyau de bout en bout AVANT d'étoffer.** Tant que ça ne marche pas, personne ne travaille sur autre chose. **Non négociable.**

- [X] `docker-compose.yml` à 5 services : back, postgres, clickhouse, redis, front
- [X] Healthchecks sur les 3 DB + `depends_on` avec `condition: service_healthy`
- [X] Volumes nommés pour la persistance
- [X] `.env.example` propre, aucun secret en dur (clé API OpenAQ en variable)
- [ ] **Script d'ingestion OpenAQ** (Rust ou Python) : interroge l'API v3 (`/v3/locations`, `/v3/measurements`) + backfill via l'archive S3 open-data, batch insert ClickHouse via driver natif. Ingérer quelques jours pour démarrer. *(API v3 ✅ livré ; backfill S3 restant → backlog A6)*
- [X] **Endpoint walking skeleton** : `GET /api/measurements?location_id=…&parameter=pm25&from=…&to=…` → query ClickHouse → JSON paginé
- [X] **Auth JWT** minimale : login (Postgres user lookup + Argon2id) + refresh token (Redis)
- [X] **Écran front minimal** : login + série temporelle d'un lieu filtrable
- [X] **Doc API auto-générée** accessible à `/api/docs` (utoipa pour Axum) *(livrée le 2026-06-07 — backlog A2)*
- [ ] Test en clonant le repo sur un autre poste : `docker compose up` doit suffire

**Livrable** : un user loggé voit des mesures OpenAQ réelles. C'est moche mais ça prouve que l'archi tient.

---

## Jalon 3 — Profondeur fonctionnelle

Travail en parallèle sur 4 axes.

### Axe BDD

**Postgres** :

- [X] 2 vues métier (`org_active_zones_view`, `org_alert_stats_view`) *(2026-06-07 — migration 0002, backlog B1)*
- [X] 2 triggers — **6 livrés, T1–T6** (audit auto `alert_rules`, compteur non-lus, + invariants isolation/immuabilité) *(2026-06-07 — migration 0003, backlog B2)*
- [X] 3 procédures stockées (`create_tracked_location_with_rules`, `archive_old_alert_events`, `compute_exposure_dose`) *(2026-06-07 — migration 0004, backlog B3)*
- [X] 1 transaction explicite avec ROLLBACK testé *(cas retenu : P1 lieu+stations+règles — back/tests/db.rs)*
- [X] 1 index non-trivial (index partiel `alert_events(org_id, fired_at) WHERE is_read = false`) *(livré dès 0001_init.sql:306)*
- [X] **Requête métier complexe** : 4 jointures + GROUP BY + HAVING + 1 CTE — fichier `db/sql/queries/complex_business_query.sql`. Cas : top 10 orgs par volume d'alertes critiques sur le dernier trimestre, avec plan d'abo + ratio lu/non-lu. *(2026-06-07 — backlog B4)*

**ClickHouse** :

- [X] `MATERIALIZED VIEW` de rollup maintenue en continu (jour × ville × polluant) *(en place dès le Jalon 1 : `mv_measurements_hourly`/`_daily` — granularité **station** : la « ville » n'existe pas dans le schéma ClickHouse, l'agrégation par lieu vit côté Postgres via `tracked_location_stations` ; validées et consommées par B5 — Q4)*
- [X] **Moyennes glissantes réglementaires** via window functions : O₃ max journalier de la moyenne 8 h, PM2.5 moyenne 24 h, NO₂ moyenne annuelle — fichier `db/clickhouse/queries/rolling_regulatory.sql` *(2026-06-07 — backlog B5 : Q1 pm25/pm10 24 h + o3 8 h + no2 1 h avec couverture EPA 75 %, Q3 O₃ max journalier 8 h, Q4 NO₂ moyenne annuelle — depuis `measurements_daily`, les bruts ne vivant que 90 j)*
- [X] Calcul **AQI** (concentration → catégorie) via table de breakpoints *(2026-06-07 — backlog B5 : Q2 `aqi_snapshot`, miroir verbatim des `aqi_breakpoints` Postgres, troncature + interpolation EPA, conversion ppb figée D4.2, polluant dominant)*
- [X] Table avec TTL purgeant les mesures brutes > 90 jours *(en place dès le Jalon 1 — `01_schema.sql` : TTL 90 j, `ttl_only_drop_parts=1`)*
- [ ] Une requête d'agrégation lourde benchmark : gain ClickHouse vs Postgres équivalent

### Axe back

**16+ endpoints REST** :

- [X] CRUD sur 4 ressources : `users`, `organizations`, `tracked_locations`, `alert_rules` (= **20 endpoints**) *(2026-06-10 — backlog B6, PR #34 : 5 handlers × 4 ressources, 55 tests e2e)*
- [X] Auth : `POST /auth/login`, `POST /auth/refresh`, `POST /auth/logout`, `GET /auth/me` *(register écarté — choix B2B/B2G assumé : les comptes sont provisionnés par un admin d'org via `POST /api/users` + RBAC, pas d'auto-inscription)*
- [ ] Transverses : `GET /api/measurements/timeseries` (agg ClickHouse) ✅ *J2*, `POST /api/alert-rules/{id}/run` (force-check — hot path Moka) ✅ *B7* ; **restants** : `GET /api/locations` (recherche stations OpenAQ), `GET /api/locations/{id}/aqi`, `GET /api/rankings`, `POST /api/exposure/compute`
- [X] Codes HTTP corrects (200, 201, 204, 400, 401, 403, 404, 409, 422, 429, 500) *(2026-06-10 — B6 : codes Postgres mappés 23505→409, 23514→422)*
- [X] Pagination + filtrage + tri sur tous les listings *(2026-06-10 — B6 : uniforme sur les 4 listings)*
- [X] Doc OpenAPI auto-générée (utoipa) *(livrée le 2026-06-07 — backlog A2, étendue aux 20 endpoints B6)*

**Cache & temps réel** :

- [X] Moka : règles d'alerte compilées, rechargées à chaque batch (TTL léger > intervalle) *(2026-06-10 — backlog B7, PR #35 : boucle de matching de seuils, index `RuleIndex` rechargé via cache Moka L1, idempotence base migration 0006)*
- [ ] Redis pub/sub : channel par org, push WebSocket quand une mesure dépasse un seuil *(restant — backlog B8)*
- [X] Redis : rate-limit middleware (token bucket sur IP + user) + quota par clé API *(socle livré — rate-limit login email+IP sur Redis ; quota par clé API à étendre en B9)*

**Sécurité** :

- [X] Argon2id sur les mots de passe (pas bcrypt) *(+ vérification factice à temps constant pour les emails inconnus — anti-énumération temporelle)*
- [X] Validation `serde` + crate `validator` sur tous les inputs (seuils, plages de dates, coordonnées, intervalles autorisés) *(socle posé le 2026-06-07 — backlog A5 ; étendu aux endpoints B6 le 2026-06-10)*
- [X] Paramètres préparés partout (anti-injection SQL ET ClickHouse — allowlist stricte des valeurs d'`interval`/`agg`/`parameter`)
- [X] CORS strict (allowlist des origins front)
- [X] Headers via tower-http : CSP, X-Frame-Options, HSTS
- [X] Pas de secret dans le repo, tout en env vars *(clés API citoyennes hashées en base — prévu B9, hors périmètre Jalon 3 actuel)*

### Axe front

- [ ] Charte graphique propre (voir `docs/identity.md`) → page design system
- [ ] Écrans : dashboard (carte + AQI), lieux suivis, règles d'alerte, détail d'un lieu (séries temporelles + moyennes glissantes + jauge AQI + calendar-heatmap), profils d'exposition, analytics/comparaison, profil/org
- [ ] Responsive mobile + desktop
- [ ] WebSocket client pour les alertes temps réel
- [ ] **Interdit** : Bootstrap default ou Tailwind starter « tel quel » — personnalise.

### Axe doc

- [ ] **Captures en continu** : à chaque feature finie, screenshot dans `docs/captures/` (board, Swagger, DBeaver, clickhouse-client, app)
- [ ] Documentation produit au fil de l'eau (pitch, identité, README à jour)
- [ ] Revue d'itération régulière documentée

---

## Jalon 4 — Durcissement & V1

- [ ] Recette : tableau de tests fonctionnels manuels, scénario par scénario, attendu vs obtenu
- [ ] README final : prérequis, lancement `docker compose up`, variables d'env, comptes de démo, lien doc API
- [ ] Performance : benchmarks ingestion + requêtes ClickHouse, latence matching
- [ ] CI/CD (lint + tests + build), tests sur les chemins critiques
- [ ] Déploiement (au moins un environnement de démo)
- [ ] Tag Git `v1.0`

---

## Règles de discipline (à tenir tout du long)

1. **Captures en temps réel** — screenshot à chaque feature finie, dans `docs/captures/`. Pas la veille.
2. **Pas de commit sur `master` ni `dev`** — toujours par PR avec CI verte obligatoire (process solo : la CI tient le rôle de reviewer). (Exécuté par l'humain ; les agents préparent et signalent — voir `AGENTS.md`.)
3. **Revue d'itération régulière** — ce qui avance, ce qui bloque, ce qui glisse. Si quelqu'un cale, c'est détecté tôt.
4. **Pas de fichier flottant sur Discord** — tout dans le board ou le repo.
5. **Les agents loggent toutes leurs actions** dans `logs/` (voir `AGENTS.md` §6). C'est la trace d'audit du projet.
6. **Walking skeleton non négociable au Jalon 2** — pas de feature riche tant que le tuyau bout-à-bout ne marche pas.

---

## Anti-patterns à éviter

- ❌ Prendre MongoDB par défaut sans réflexion — ClickHouse justifié, Postgres discipliné.
- ❌ Stocker des passwords en clair, des tokens dans le code, des clés API (OpenAQ ou citoyennes) dans Git.
- ❌ Faire 16 endpoints qui sont tous des `SELECT *` basiques — mutations métier, transactions testées.
- ❌ Laisser un agent committer/pusher à ta place — un agent prépare et signale, il ne touche jamais à Git.
- ❌ Travailler tous sur `master`/`dev` sans review.
