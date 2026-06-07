# Log — agent-bdd — 2026-06-06 — migrations-sqlx

## 23:55 CEST — Migrations sqlx (schéma gelé en 0001) + COMMENT unités AQI corrigé

- **Agent / rôle** : agent-bdd
- **Jalon / tâche** : pré-requis B1–B5 du backlog — critiques n°1 (DROP CASCADE / pas de feature migrate) et n°2 (COMMENT `aqi_breakpoints.unit` contradictoire, décision D4.2 de `docs/foundations.md`) de la revue.
- **Contexte** : `db/sql/01_schema.sql` commençait par des `DROP TABLE … CASCADE` (dangereux en initdb.d / rejeu) et sqlx n'avait pas la feature `migrate` ; le COMMENT de `aqi_breakpoints.unit` affirmait à tort que `unit` doit matcher l'unité OpenAQ canonique (le seed insère les paliers O3/NO2 en ppb, unité EPA).
- **Actions** :
  - Création de `back/migrations/0001_init.sql` : schéma Postgres complet (23 tables), **gelé**, SANS le bloc `DROP … CASCADE`, rendu **idempotent** (`CREATE TABLE IF NOT EXISTS`, `CREATE [UNIQUE] INDEX IF NOT EXISTS`, `CREATE EXTENSION IF NOT EXISTS` ; aucun `CREATE TYPE`/`TRIGGER`/`VIEW` à protéger ; les `COMMENT ON TABLE` sont idempotents par nature). Sur un volume Postgres existant (tables créées par l'ancien initdb.d), 0001 passe en no-op et est enregistrée dans `_sqlx_migrations` → **pas besoin de `down -v`**.
  - COMMENT `aqi_breakpoints` corrigé (dans 0001) : « unit = unité des paliers EPA (peut différer de l'unité OpenAQ canonique du polluant : O3/NO2 en ppb) ; toute comparaison mesure↔palier exige une conversion préalable — cf. docs/foundations.md décision D4.2 ».
  - `back/Cargo.toml` : feature `migrate` ajoutée à sqlx (→ `back/Cargo.lock` mis à jour automatiquement par cargo).
  - `back/src/main.rs` : `sqlx::migrate!("./migrations").run(&state.pg)` exécuté juste après `AppState::connect` et AVANT `serve` ; échec = erreur fatale avec contexte « application des migrations Postgres (back/migrations) — démarrage refusé ».
  - `db/sql/01_schema.sql` : remplacé par un court pointeur (« source de vérité = back/migrations/ ») — fichier conservé pour ne pas casser les références documentaires.
  - `docker-compose.yml` : montages initdb.d Postgres (01_schema + 02_seed) retirés ; ajout d'un service one-shot `seed` (image `postgres:16-alpine`, `profiles: ["seed"]`, `depends_on postgres healthy`, `psql -v ON_ERROR_STOP=1 -f /seed/02_seed.sql` monté `:ro`, variables `PG*` depuis l'env). initdb.d ClickHouse intact.
  - `.github/workflows/ci.yml` : chargement du schéma Postgres via `psql -f back/migrations/0001_init.sql` (chemins relatifs à la racine du checkout — pas de `working-directory` sur ce step) ; seed `02_seed.sql` inchangé.
  - `docs/data-model.md` : note en tête de la section B (MLD Postgres) — schéma appliqué via les migrations sqlx au boot du back.
- **Fichiers touchés** :
  - `back/migrations/0001_init.sql` (créé)
  - `back/Cargo.toml` (modifié)
  - `back/Cargo.lock` (modifié — effet mécanique de la feature ajoutée)
  - `back/src/main.rs` (modifié)
  - `db/sql/01_schema.sql` (modifié — devenu pointeur)
  - `docker-compose.yml` (modifié)
  - `.github/workflows/ci.yml` (modifié)
  - `docs/data-model.md` (modifié)
  - `logs/2026-06-06__agent-bdd__migrations-sqlx.md` (créé)
- **Résultat** : OK — avec **1 point bloquant hors périmètre** à traiter par l'humain (voir ci-dessous).
- **Vérifs** : depuis `back/` : `cargo fmt` puis `cargo fmt --all -- --check` → exit 0 ; `cargo check` → OK ; `cargo clippy --all-targets -- -D warnings` → exit 0, 0 warning ; `cargo test --lib` → 5 passed, 0 failed. YAML `docker-compose.yml` + `ci.yml` parsés OK (python yaml). Tests e2e et docker NON lancés (charte — la CI s'en charge).
- **⚠ Point bloquant pour l'humain (fichier NON autorisé dans ce lot)** :
  - `back/Dockerfile` ne copie que `Cargo.toml` et `src/` dans le builder. La macro `sqlx::migrate!` lit `back/migrations/` **à la compilation** → le `docker build` (job CI « docker » + `docker compose build back`) échouera tant qu'on n'ajoute pas, après `COPY src ./src` :
    `COPY migrations ./migrations`
  - À faire par toi (idéalement dans le même commit) ou par le lot infra. Le `.dockerignore` n'exclut pas `migrations/`, rien d'autre à changer.
- **Nouvel ordre de boot (à documenter/connaître)** :
  1. `docker compose up -d` → Postgres démarre **vide** (plus d'initdb.d), le back applique `back/migrations/0001_init.sql` au démarrage (table `_sqlx_migrations` créée/complétée), puis sert.
  2. Seed démo (optionnel, rejouable — `TRUNCATE … RESTART IDENTITY CASCADE` en tête) : `docker compose --profile seed run --rm seed`.
  3. Volume `pgdata` existant (tables déjà créées par l'ancien initdb.d) : 0001 est idempotente → no-op enregistrée, **aucun `docker compose down -v` requis**.
  - En CI, le schéma est chargé par `psql -f back/migrations/0001_init.sql` (les tests d'intégration n'exécutent pas `main`), le seed reste `psql -f db/sql/02_seed.sql`.
- **Prochaine étape** : ajouter `COPY migrations ./migrations` au `back/Dockerfile` (humain/lot infra), puis missions B1–B5 du backlog (chaque évolution de schéma = nouvelle migration `back/migrations/000N_*.sql`, ne jamais retoucher 0001 : sqlx vérifie les checksums).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Branche cible : feature/bdd-migrations-sqlx   (jamais master/dev en direct)
  > 1) git add back/migrations/0001_init.sql back/Cargo.toml back/Cargo.lock back/src/main.rs db/sql/01_schema.sql docker-compose.yml .github/workflows/ci.yml docs/data-model.md logs/2026-06-06__agent-bdd__migrations-sqlx.md
  > 2) git commit -m "feat(bdd): migrations sqlx (schéma gelé en 0001) + COMMENT unités AQI corrigé"
  > 3) git push -u origin feature/bdd-migrations-sqlx
  > Puis : ouvrir une PR vers `dev`, attendre la CI verte, squash merge.
  > ⚠ Avant la PR : ajouter `COPY migrations ./migrations` dans back/Dockerfile (hors périmètre de ce lot), sinon le job CI « docker » échoue.
  > ─────────────────────────────────────────────

## 00:03 CEST — Correctifs post-revue : COPY migrations dans le Dockerfile + lien DDL de data-model.md + format du log

- **Agent / rôle** : agent-bdd
- **Jalon / tâche** : reprise du lot « migrations » — correction des 4 problèmes relevés par le vérificateur (1 critique, 3 mineurs). Entrée écrite le 2026-06-07 juste après minuit (même session, on append au fichier de la veille conformément à `logs/README.md`).
- **Contexte** : le lot était non mergeable en l'état — `docker build ./back` échouait (« migrations directory not found ») car le builder ne copiait pas `migrations/`, lu **à la compilation** par `sqlx::migrate!`.
- **Actions** :
  - **Critique — `back/Dockerfile`** : ajout de `COPY migrations ./migrations` après `COPY src ./src` dans le stage builder (+ commentaire expliquant pourquoi). Fichier hors liste autorisée du lot d'origine, mais explicitement ajouté au périmètre par le `fix_hint` du vérificateur (seul moyen de rendre le lot mergeable). Le `back/.dockerignore` (`target/`, `.env`, `**/*.rs.bk`) n'exclut pas `migrations/` → rien d'autre à changer. Contexte de build = `./back` (docker-compose.yml lignes 108/134 et job CI « docker »).
  - **Mineur — `back/Cargo.lock`** : aucun changement de fichier ; décision actée conformément au `fix_hint` → le lockfile **reste dans le commit** (conséquence mécanique de la feature `migrate` + `cargo check` exigé ; le rejeter laisserait l'arbre incohérent). Si un autre lot parallèle touche aussi `Cargo.lock`, **merger ce lot en premier**.
  - **Mineur — format du log** : en-tête de l'entrée précédente corrigé « ## 23:55 — » → « ## 23:55 CEST — » (fuseau imposé par `logs/README.md`).
  - **Mineur — références doc** : dans `docs/data-model.md` (fichier autorisé), le lien « DDL exécutable » de l'en-tête pointe désormais vers `back/migrations/0001_init.sql` (au lieu de `db/sql/01_schema.sql`, devenu simple pointeur) ; le lien seed `db/sql/02_seed.sql` est inchangé (toujours exécutable via le service `seed` et la CI). Le commentaire d'en-tête de `db/sql/02_seed.sql` (« Prérequis : 01_schema.sql appliqué ») reste **hors périmètre** → follow-up lot agent-doc.
- **Fichiers touchés** :
  - `back/Dockerfile` (modifié — 2 lignes ajoutées dans le builder)
  - `docs/data-model.md` (modifié — lien « DDL exécutable » de l'en-tête)
  - `logs/2026-06-06__agent-bdd__migrations-sqlx.md` (modifié — en-tête 23:55 + cette entrée)
- **Résultat** : OK — le lot est désormais mergeable (le job CI « docker » et `docker compose build back` trouveront `migrations/`).
- **Vérifs** : depuis `back/` : `cargo fmt` puis `cargo fmt --all -- --check` → exit 0 ; `cargo check` → OK ; `cargo clippy --all-targets -- -D warnings` → exit 0, 0 warning ; `cargo test --lib` → 5 passed, 0 failed. Docker NON lancé (charte — la CI s'en charge) : le correctif Dockerfile sera validé par le job CI « docker ».
- **Prochaine étape** : commit du lot complet (bloc ci-dessous, qui **remplace** celui de l'entrée 23:55) ; follow-up agent-doc pour l'en-tête de `db/sql/02_seed.sql` ; puis missions B1–B5 du backlog (toute évolution de schéma = nouvelle migration `back/migrations/000N_*.sql`, ne jamais retoucher 0001).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Branche cible : feature/bdd-migrations-sqlx   (jamais master/dev en direct)
  > 1) git add back/migrations/0001_init.sql back/Cargo.toml back/Cargo.lock back/src/main.rs back/Dockerfile db/sql/01_schema.sql docker-compose.yml .github/workflows/ci.yml docs/data-model.md logs/2026-06-06__agent-bdd__migrations-sqlx.md
  > 2) git commit -m "feat(bdd): migrations sqlx (schéma gelé en 0001) + COMMENT unités AQI corrigé"
  > 3) git push -u origin feature/bdd-migrations-sqlx
  > Puis : ouvrir une PR vers `dev`, attendre la CI verte, squash merge.
  > ─────────────────────────────────────────────

## 00:40 CEST — Micro-correctif post-revue (coordinateur) : référence §8 dans data-model.md

- **Agent / rôle** : coordinateur
- **Jalon / tâche** : même lot migrations (correctif issu de la re-vérification adversariale)
- **Contexte** : la re-vérification a relevé une référence résiduelle « tracées dans `01_schema.sql §8` » (data-model.md §E) alors que le §8 vit désormais dans `back/migrations/0001_init.sql`.
- **Actions** :
  - `docs/data-model.md` ligne 157 : référence mise à jour vers `back/migrations/0001_init.sql §8`.
- **Fichiers touchés** :
  - `docs/data-model.md` (modifié — 1 ligne ; fichier déjà dans le git add du lot)
- **Résultat** : OK
- **Vérifs** : grep « 01_schema.sql §8 » → 0 occurrence restante.
- **Prochaine étape** : aucune — le bloc Git de l'entrée 00:03 reste valable tel quel (la liste de fichiers est inchangée).
- **Action Git suggérée à l'humain** :
  > Aucune nouvelle — utiliser le bloc de l'entrée 00:03 ci-dessus.
