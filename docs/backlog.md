# Quarity — État d'avancement & prochaines missions

> Vue d'ensemble vivante : ce qui est **fait/validé**, la **dette connue**, et les **missions à venir** par priorité.
> Complète [`roadmap.md`](../roadmap.md) (le plan) avec l'avancement réel. Trace d'audit détaillée : [`logs/`](../logs/).
> Dernière mise à jour : 2026-06-04 (fin du walking skeleton).

---

## ✅ Fait & validé

### Jalon 0 — Cadrage
README, AGENTS.md, CONTRIBUTING.md, roadmap, pitch, identité, logs. *(Repo Git + branches `dev` créées en cours de route.)*

### Jalon 1 — Modélisation *(branche `feature/bdd-jalon1-modelisation`)*
- Personas (5), user stories (16) + **MoSCoW** — `docs/personas.md`, `docs/user-stories.md`.
- **MCD/MLD + frontière inter-bases** — `docs/data-model.md`.
- **Postgres 16** : 23 tables 3NF, FK, index partiels — `db/sql/01_schema.sql` + `02_seed.sql` (**exécutés/validés** sur Postgres réel).
- **ClickHouse 24.8** : `measurements` ReplacingMergeTree + 2 rollups MV + TTL — `db/clickhouse/01_schema.sql` + `02_seed.sql` (**validés**, dédup prouvée).

### Jalon 2 — Walking skeleton *(branches `feature/infra-*`, `feature/back-*`, front/ingest dans le working tree)*
- **`docker-compose.yml`** : 5 services (postgres, clickhouse, redis, back, front) + healthchecks + `depends_on: service_healthy` + auto-chargement des schémas. `.env.example` propre.
- **Back Rust/Axum** (`back/`) : auth JWT (Argon2id + refresh Redis), `GET /api/measurements` (ClickHouse, **isolation multi-tenant**, allowlist anti-injection), `/health`. Compile + 9/9 tests e2e.
- **Front React/Vite** (`front/`) : login + dashboard série temporelle (recharts), charte `identity.md`, nginx + proxy `/api`. Build + typecheck OK, stack 5 services validé.
- **Ingestion OpenAQ** (`back/src/bin/ingest.rs`) : station réelle → ClickHouse + liaison org. Service compose `--profile ingest`. **287 mesures réelles** validées end-to-end (NICE PROMENADE #4085).

➡️ **La chaîne complète fonctionne** : `OpenAQ → ingest → ClickHouse+Postgres → back (JWT+isolation) → nginx → front`.

---

## ⚠️ Dette / points connus à régler

| # | Sujet | Détail |
|---|---|---|
| D1 | **Structure du dépôt** | `README/AGENTS/CONTRIBUTING/roadmap/logs` sont à la racine `quarity/`, **hors** du dépôt git (`app/`) → non versionnés. Liens README vers `docs/pitch.md` cassés (réels sous `app/docs/`). **À corriger** (remonter dans `app/` ou déplacer le `.git`). |
| D2 | **Port Postgres natif** | Un Postgres natif occupe `localhost:5432` sur le poste de dev → lancer avec ports hauts (`POSTGRES_PORT=55432 …`). À documenter pour les devs. |
| D3 | **Secrets** | `.env` jamais committé (gitignoré). **Clé OpenAQ de dev à régénérer** (exposée en session). `JWT_SECRET` à générer (`openssl rand -hex 32`). |
| D4 | **Seed = hash démo partagé** | Tous les comptes démo ont le mot de passe `Quarity2026!`. À ne pas reproduire en prod. |
| D5 | **CORS permissif** | Le back autorise `Any` (skeleton) → allowlist stricte des origins à mettre (cf. missions). |

---

## 🎯 Prochaines missions (par priorité)

### A. Finir le Jalon 2 / durcissement (court terme)
- [ ] **A1** Vérif visuelle du front (navigateur) + 1ʳᵉˢ **captures** dans `docs/captures/` (board, Swagger, app).
- [ ] **A2** Doc **OpenAPI `/api/docs`** (utoipa + utoipa-swagger-ui `vendored`) — différée volontairement, à ajouter.
- [ ] **A3** **CORS strict** (allowlist origins front) + **headers sécurité** (CSP, HSTS, X-Frame) via tower-http.
- [ ] **A4** **Rate-limit Redis** (token bucket IP + user) + quota par clé API.
- [ ] **A5** Validation des inputs au boundary (crate `validator`).
- [ ] **A6** Ingestion : **ordonnancement périodique** (cron/scheduler) + **backfill S3** + pagination/retry/idempotence robustes.
- [ ] **A7** **CI minimale** : clippy + fmt + `cargo test` + `npm run build` + `docker build` ; ≥ 1 test d'intégration ; **test d'isolation cross-tenant**. (Roadmap la met au Jalon 4 — la remonter ici réduit la dette.)

### B. Jalon 3 — Profondeur fonctionnelle
**BDD** (cf. `docs/data-model.md §E`) :
- [ ] **B1** 2 vues métier (`org_active_zones_view`, `org_alert_stats_view`).
- [ ] **B2** Triggers T1–T6 : ≥1 station/lieu, cohérence `org_id` sur `alert_rules`, destinataire ∈ org, **immuabilité `alert_events`**, audit `alert_rules`, compteur non-lus.
- [ ] **B3** 3 procédures (`create_tracked_location_with_rules`, `archive_old_alert_events`, `compute_exposure_dose`) + 1 transaction avec ROLLBACK testé.
- [ ] **B4** Requête complexe (`db/sql/queries/complex_business_query.sql`).
- [ ] **B5** ClickHouse : MV rollup maintenue, **moyennes glissantes réglementaires** (`db/clickhouse/queries/rolling_regulatory.sql`), calcul **AQI** via breakpoints.

**Back** :
- [ ] **B6** CRUD complet (16+ endpoints) users/orgs/tracked_locations/alert_rules + codes HTTP + pagination/tri/filtre.
- [ ] **B7** **Boucle de matching de seuils** (cache Moka L1) → `alert_events`.
- [ ] **B8** Redis **pub/sub + WebSocket** alertes temps réel.
- [ ] **B9** Profils d'exposition + calcul de dose ; API publique + clés API + quotas.

**Front** :
- [ ] **B10** Dashboard **carte + jauge AQI** (le composant `AqiBadge` est déjà prêt).
- [ ] **B11** CRUD lieux suivis & règles d'alerte ; **client WebSocket** alertes ; profils d'exposition ; responsive.

**Sécurité/conformité** (cf. rapport d'analyse, à instruire **avant** d'aller plus loin sur les données perso) :
- [ ] **B12** Mini **threat model** + classification des données ; invariant **multi-tenant** testé.
- [ ] **B13** **RGPD** (registre, rétention des données perso ≠ TTL mesures, DPA B2G) ; matrice **RBAC** + middleware ; **journal d'audit** applicatif.

### C. Jalon 4 — Durcissement & V1
- [ ] **C1** Recette : tableau de tests fonctionnels manuels (attendu vs obtenu).
- [ ] **C2** Benchmarks perf (matching < 5 ms, timeseries < 200 ms p95) **avec hypothèses de charge** + protocole.
- [ ] **C3** CI/CD complète + **sécurité supply-chain** (`cargo audit`, `npm audit`, lockfiles committés).
- [ ] **C4** Déploiement (≥ 1 environnement de démo) ; README final ; **tag `v1.0`**.

### D. Angles morts de fondation (à trancher tôt — cf. rapport d'analyse)
> 📄 Décisions instruites dans [`foundations.md`](foundations.md) (proposées + à confirmer juridiquement) — 2026-06-05.
- [ ] **D1** **Droit d'usage OpenAQ** : licence/attribution pour un produit B2B/B2G **payant**.
- [ ] **D2** **Qualité des données** OpenAQ : trous, valeurs aberrantes, unités hétérogènes (ppb vs µg/m³) → nettoyage/validation avant alertes sanitaires.
- [ ] **D3** **Responsabilité** des alertes sanitaires : disclaimer, « non certifié », SLA d'exactitude.

---

## Repères techniques (pour démarrer vite)
- Lancer : `docker compose up -d --build` (ports hauts si Postgres natif, cf. D2) ; ingérer : `docker compose --profile ingest run --rm ingest [location_id]`.
- Comptes démo : `sophie@agglo-riviera.fr` / `Quarity2026!` (org A) · station réelle ingérée : `location_id=4085` (NICE PROMENADE).
- Versions clés : axum 0.8, sqlx 0.8, redis 0.27, reqwest 0.12 · React 18.3, Vite 6, react-router 7.9, recharts 2.15.
- Décisions de modélisation tracées : `docs/data-model.md §F`. Logs d'agents : `logs/2026-06-04__*`.
