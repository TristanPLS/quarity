# Log — agent-infra — 2026-06-04 — jalon2-ingestion-openaq

## 22:29 CEST — Script d'ingestion OpenAQ → ClickHouse + liaison org

- **Agent / rôle** : agent-infra
- **Jalon / tâche** : Jalon 2 — walking skeleton, ingestion des mesures réelles OpenAQ
- **Contexte** : remplacer le seed de démo par de vraies mesures OpenAQ, pour atteindre « un user loggé voit des mesures OpenAQ réelles » (pitch).
- **Actions** :
  - Exploration de l'API OpenAQ v3 avec la clé (forme des réponses `/v3/locations/{id}`, `/v3/sensors/{id}/hours` : `value`, `parameter.{name,units}`, `period.datetimeFrom.utc`). Station de référence repérée : #4085 NICE PROMENADE (FR), capteurs pm25 #9648, pm10 #9649, no2 #9859 — cohérente avec le seed Riviera.
  - Binaire Rust `back/src/bin/ingest.rs` (réutilise reqwest/sqlx/serde du crate back ; ajout de `chrono`) :
    1. récupère une station OpenAQ + ses capteurs (allowlist pm25/pm10/no2/o3/so2/co — 'no' ignoré),
    2. récupère les mesures horaires des N derniers jours (défaut 7),
    3. INSERT batch dans `quarity.measurements` (ClickHouse, FORMAT JSONEachRow ; conversion `…T…Z` → `… .000`),
    4. **lie la station à une org** en Postgres (upsert `ref_locations` + `ref_sensors` + `tracked_locations` + `tracked_location_stations`) pour qu'elle soit interrogeable depuis le front avec l'isolation multi-tenant.
  - Empaquetage : `back/Dockerfile` copie aussi le binaire (`quarity-ingest`) ; service `ingest` (profile `ingest`) dans `docker-compose.yml` → `docker compose --profile ingest run --rm ingest [location_id]`. `OPENAQ_API_KEY` lu depuis `.env` (jamais en dur).
- **Fichiers touchés** :
  - `app/back/src/bin/ingest.rs` (créé)
  - `app/back/Cargo.toml` (modifié — dépendance `chrono`)
  - `app/back/Dockerfile` (modifié — copie du binaire `quarity-ingest`)
  - `app/docker-compose.yml` (modifié — tag `image: quarity-back:local` + service `ingest` en profile)
  - `logs/2026-06-04__agent-infra__jalon2-ingestion-openaq.md` (créé — hors dépôt app/)
- **Résultat** : OK — ingestion réelle validée de bout en bout.
- **Vérifs** (clé OpenAQ réelle, ports hauts) :
  - `ingest` (station 4085, 7 jours) → **287 mesures réelles** insérées (pm25:74, no2:139, pm10:74).
  - ClickHouse : pm25 (74 pts, 3.7–17.3 µg/m³), no2 (139), pm10 (74).
  - Postgres : org « agglo-riviera » suit « NICE PROMENADE (OpenAQ) ».
  - **END-TO-END** : login sophie → `GET /api/measurements?location_id=4085&parameter=pm25` → **count=74 vrais points** (valeurs/timestamps réels), isolation multi-tenant respectée.
- **Prochaine étape** : ordonnancement (cron/scheduler pour ingestion périodique au lieu d'un one-shot) + backfill via l'archive S3 OpenAQ (roadmap) ; idempotence déjà couverte par `ReplacingMergeTree`. Côté produit : durcissement (CORS strict, rate-limit, OpenAPI) puis Jalon 3 (alertes temps réel WebSocket, vues/triggers BDD).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Le script d'ingestion fait partie du lot « walking skeleton » (working tree, branche back/front).
  > Sécurité : NE PAS committer .env (clé OpenAQ). Seul le code (qui lit OPENAQ_API_KEY depuis l'env) est versionné.
  >   git add back/src/bin/ingest.rs back/Cargo.toml back/Cargo.lock back/Dockerfile docker-compose.yml
  >   git commit -m "feat(infra): ingestion OpenAQ -> ClickHouse + liaison org (binaire + service compose profile)"
  >   git push
  > Puis : intégrer au lot Jalon 2 (PR walking skeleton) ou PR dédiée vers dev.
  > ─────────────────────────────────────────────
