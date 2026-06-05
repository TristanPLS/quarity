# Log — agent-infra — 2026-06-04 — jalon2-socle-db

## 16:40 CEST — Walking skeleton : socle bases de données (docker-compose)

- **Agent / rôle** : agent-infra
- **Jalon / tâche** : Jalon 2 — walking skeleton, fondement bases de données (Postgres + ClickHouse + Redis)
- **Contexte** : démarrer le walking skeleton (« non négociable » du roadmap) par son socle DB, en s'appuyant sur les schémas validés au Jalon 1. Objectif : `docker compose up -d` lève les 3 bases, healthy, avec schémas + seeds auto-chargés.
- **Actions** :
  - `docker-compose.yml` : 3 services DB (postgres:16-alpine, clickhouse/clickhouse-server:24.8-alpine, redis:7-alpine) avec healthchecks, volumes nommés (pgdata/clickhouse_data/redis_data), `ulimit nofile` ClickHouse, ports paramétrables.
  - Auto-chargement des schémas au 1er boot via `/docker-entrypoint-initdb.d` (mounts en lecture seule de `db/sql/01_schema.sql`+`02_seed.sql` et `db/clickhouse/01_schema.sql`+`02_seed.sql`).
  - Services `back`/`front` préparés en commentaire avec le pattern `depends_on: condition: service_healthy` (à activer quand leur code existera).
  - `.env.example` : aucun secret en dur, clé OpenAQ en variable, identifiants DB/Redis paramétrés, blocs back/front commentés.
  - Bug détecté et corrigé à la validation : le healthcheck Redis référençait `$REDIS_PASSWORD` non exposé au conteneur (mot de passe vide → `unhealthy` alors que Redis répondait PONG). Ajout de `environment: REDIS_PASSWORD` au service redis.
- **Fichiers touchés** :
  - `app/docker-compose.yml` (créé)
  - `app/.env.example` (créé)
  - `app/.env` (créé localement pour la validation — ⚠ gitignoré, NON versionné, valeurs `change_me` à remplacer)
  - `logs/2026-06-04__agent-infra__jalon2-socle-db.md` (créé — ⚠ hors du dépôt git app/)
- **Résultat** : OK — stack validé sur Docker réel.
- **Vérifs** (`docker compose up -d` puis inspection des healthchecks + chargement) :
  - `docker compose config` → valide.
  - Postgres : `healthy` ; schéma auto-chargé = 23 tables, 3 orgs, 6 alert_events.
  - ClickHouse : `healthy` ; schéma auto-chargé = 5 objets, mesures chargées (doublon collapsé par merge).
  - Redis : `healthy` (après correctif) ; `PING` → `PONG`.
  - Teardown `docker compose down -v` OK (volumes supprimés, état propre).
- **Prochaine étape** : Jalon 2 — services applicatifs : Dockerfile back (Rust/Axum) + endpoint walking skeleton `GET /api/measurements` (query ClickHouse) + auth JWT minimale (Postgres/Argon2id + refresh Redis) ; Dockerfile front (React/Vite) + écran login + série temporelle ; script d'ingestion OpenAQ. Décommenter alors les services back/front du compose.
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Branche cible : feature/infra-jalon2-socle-db     (partir de `dev`, jamais master/dev en direct)
  >
  > # .env est gitignoré → NE PAS l'ajouter. On versionne uniquement docker-compose.yml + .env.example.
  > # Le log ci-dessus (logs/) est hors du dépôt app/ tant que la structure n'est pas corrigée.
  >
  > 1) git switch dev && git switch -c feature/infra-jalon2-socle-db
  > 2) git add app/docker-compose.yml app/.env.example
  > 3) git commit -m "feat(infra): socle DB walking skeleton (docker-compose Postgres+ClickHouse+Redis, healthchecks, auto-load schémas)"
  > 4) git push -u origin feature/infra-jalon2-socle-db
  > Puis : ouvrir une PR vers `dev`, 1 reviewer, squash merge.
  > ─────────────────────────────────────────────
