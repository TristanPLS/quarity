# Log — agent-infra — 2026-06-15 — mem_limits + ports loopback compose (Vague 1 : 5b/5c)

## CEST — Limites mémoire + ports DB en loopback (plan-finition-v1, cluster 5)

- **Agent / rôle** : agent-infra
- **Jalon / tâche** : finition v1.0 — Vague 1, PR `infra/compose-mem-limits` (5b + 5c, mêmes fichiers).
- **Contexte** : aucune limite mémoire sur les conteneurs (un service pouvait affamer l'hôte) ; les ports des bases étaient exposés sur toutes les interfaces (`0.0.0.0`) alors qu'ils ne servent qu'au tooling dev (l'inter-service passe par le réseau compose).
- **Actions** (`docker-compose.yml`) :
  - **5b — mem_limits** (`deploy.resources.limits.memory`, honoré par compose v2) : ClickHouse **4g** (CH 24.8 lit la limite **cgroup** et se borne via `max_server_memory_usage_to_ram_ratio`=0.9 → couplage automatique, pas de config.d), Postgres **1g**, Redis **512m** (PAS de politique d'éviction : Redis porte refresh-tokens / rate-limit / sessions ; `--appendonly` persiste → restart = reload AOF). Pas de limite sur back/front (services applicatifs).
  - **5c — ports DB en loopback** : `127.0.0.1:…` sur postgres/clickhouse/redis uniquement. L'inter-service (réseau par défaut du projet) est **inchangé** ; seul l'accès depuis l'hôte est restreint à localhost. Ports back/front laissés en `0.0.0.0` (la démo les sert). Réseau dédié non ajouté : le réseau par défaut du projet isole déjà.
- **Fichiers touchés** :
  - `docker-compose.yml` (modifié — 3 services : postgres, clickhouse, redis)
- **Vérifs** (la CI ne lance PAS `compose up` → vérif manuelle) :
  - `docker compose config` → **valide** (YAML + interpolation `.env`).
  - Rendu (JSON) confirmé : CH mem=4 GiB ports `127.0.0.1:8123/9000` ; PG mem=1 GiB port `127.0.0.1:55432→5432` ; Redis mem=512 MiB port `127.0.0.1:6379` ; back/front en `0.0.0.0` sans limite.
- **Prochaine étape Vague 1** : série front (mêmes `*.module.css`, jamais en parallèle) `refactor/front-dedup-state-ui` (2a) → `refactor/front-selectfield` (2d).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Script : quarity/commit-infra-compose-mem-limits.cmd
  > Branche cible : infra/compose-mem-limits (depuis origin/dev)
  > fetch + switch -c, git add docker-compose.yml + ce log, commit ASCII, push, gh pr create vers dev.
  > Puis : attendre la CI verte (docker build inchangé), squash merge, maj-dev.cmd.
  > ─────────────────────────────────────────────
