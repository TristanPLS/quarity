# Log — agent-back — 2026-06-04 — jalon2-auth-measurements

## 17:36 CEST — Back walking skeleton : auth JWT + endpoint mesures (Rust/Axum)

- **Agent / rôle** : agent-back
- **Jalon / tâche** : Jalon 2 — walking skeleton, API back (auth + lecture ClickHouse)
- **Contexte** : livrer le tuyau serveur du walking skeleton (login → JWT → requête mesures avec isolation multi-tenant), branché sur le socle DB (compose).
- **Actions** :
  - Atelier de design (4 agents Context7) pour figer les versions de crates ACTUELLES et compatibles, puis écriture du crate.
  - Crate `quarity-back` (Rust/Axum 0.8) : `main` (bootstrap + arrêt gracieux + helper `hash`), `config` (env), `state` (pools Postgres/Redis/ClickHouse), `error` (AppError → JSON), `security` (Argon2id, JWT HS256, extractor `AuthUser`), `db` (sqlx runtime), `ch` (ClickHouse via HTTP + allowlist), `redis_store` (refresh tokens), routes `health`/`auth`(login,refresh,logout,me)/`measurements`.
  - Sécurité : Argon2id (vérif + helper de génération), JWT court (~15 min) + refresh token Redis (rotation au refresh, révocation au logout), isolation multi-tenant dérivée de l'`org_id` du JWT (vérif `org_owns_location` avant toute lecture ClickHouse), allowlist stricte des polluants (anti-injection), CORS + TraceLayer (tower-http).
  - Versions résolues : axum 0.8.9, sqlx 0.8.6 (requêtes runtime, pas de macros compile-time), redis 0.27.6, reqwest 0.12.28, jsonwebtoken 9.3.1, argon2 0.5. Compilation **réussie au 1er essai** (le pinning de l'atelier a payé).
  - Dockerfile multi-stage (rust:slim → debian-slim) ; service `back` activé dans `docker-compose.yml` (connexion aux DBs par nom de service, `depends_on: service_healthy`) ; `.env.example` complété (JWT_SECRET + vars back).
  - Seed mis à jour : vrais hashs Argon2id pour les comptes de démo (mot de passe `Quarity2026!`), afin que le login fonctionne réellement.
  - 3 bugs détectés UNIQUEMENT à l'exécution et corrigés : (1) le back tapait le Postgres NATIF de l'hôte sur :5432 (message d'erreur localisé non-UTF-8) → test sur ports hauts ; (2) collision d'alias ClickHouse `toString(measured_at) AS measured_at` (String vs DateTime64) + `UInt64` quotés en JSON → SQL corrigé + `output_format_json_quote_64bit_integers=0` ; (3) `SELECT 1` (INT4) décodé en `i64` (INT8) dans `org_owns_location` → `SELECT EXISTS(...)` → bool.
- **Fichiers touchés** :
  - `app/back/Cargo.toml`, `app/back/Cargo.lock`, `app/back/src/*` (main, config, state, error, security, db, ch, redis_store, routes/*), `app/back/Dockerfile`, `app/back/.dockerignore` (créés)
  - `app/docker-compose.yml` (modifié — service `back` activé)
  - `app/.env.example` (modifié — JWT_SECRET + vars back)
  - `app/db/sql/02_seed.sql` (modifié — vrais hashs Argon2id pour la démo)
  - `logs/2026-06-04__agent-back__jalon2-auth-measurements.md` (créé — hors dépôt app/)
- **Résultat** : OK — compile + tourne ; chaîne complète validée sur Postgres 16 / ClickHouse 24.8 / Redis 7 réels.
- **Vérifs** (back local contre les DBs compose, ports hauts) — 9/9 :
  - `/health` → `{postgres:true, clickhouse:true, redis:true, status:ok}`.
  - login (sophie/Quarity2026!) → 200, JWT (org_id=1, role admin), expires_in=900.
  - `/api/auth/me` → identité correcte ; refresh → rotation OK.
  - `/api/measurements` 1001/pm25 → **count=5**, valeurs dédupliquées (value=31 @ 28/05).
  - Isolation org B (2001) → **403** ; mauvais paramètre → **400** ; sans token → **401** ; mauvais mdp → **401**.
- **Prochaine étape** : ⚠ résoudre le séquencement de branches (la branche infra/back a été coupée d'un `dev` sans la bdd → schémas absents du working tree, reconstruits localement depuis Git pour le test). Merger bdd → dev, puis aligner infra/back sur dev. Ensuite : front (login + série temporelle) + script d'ingestion OpenAQ ; durcissement (CORS strict, rate-limit Redis, headers sécurité, OpenAPI/utoipa).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > ⚠ PRÉREQUIS — la branche courante (feature/infra-jalon2-socle-db) a été branchée d'un `dev`
  >   qui ne contient PAS encore la bdd (schémas). À régler AVANT de committer le back :
  >   1) Merger la PR feature/bdd-jalon1-modelisation → dev (apporte db/sql, db/clickhouse, docs).
  >   2) Récupérer la bdd dans la branche de travail : git merge origin/dev   (résoudre l'éventuel conflit sur 02_seed.sql en gardant la version "vrai hash").
  >
  > Puis committer le back (branche cible : feature/back-jalon2-auth-measurements, depuis un dev à jour) :
  >   git add app/back/Cargo.toml app/back/Cargo.lock app/back/Dockerfile app/back/.dockerignore app/back/src
  >   git add app/docker-compose.yml app/.env.example app/db/sql/02_seed.sql
  >   git commit -m "feat(back): walking skeleton — auth JWT (Argon2id+refresh Redis) + GET /api/measurements (ClickHouse, isolation multi-tenant)"
  >   git push -u origin feature/back-jalon2-auth-measurements
  > Puis : PR vers dev, 1 reviewer, squash merge.   (.env est gitignoré → ne pas l'ajouter)
  > ─────────────────────────────────────────────
