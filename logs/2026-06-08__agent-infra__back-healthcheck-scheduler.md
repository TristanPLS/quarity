# Log — agent-infra — 2026-06-08 — back-healthcheck-scheduler

## 23:36 CEST — Healthcheck Docker du back + ingest-scheduler en service_healthy

- **Agent / rôle** : agent-infra
- **Jalon / tâche** : durcissement post-analyse (rapport multi-agents du 2026-06-08) — mineur infra : fermer la course du 1er tick du scheduler contre l'application des migrations.
- **Contexte** : le service `back` n'avait AUCUN healthcheck Docker alors que `/health` existe, si bien que `ingest-scheduler` dépendait de `back` en `service_started` (conteneur lancé ≠ migrations appliquées). Le 1er tick (immédiat) pouvait s'exécuter avant que le schéma Postgres soit prêt. Point clé vérifié dans `main.rs` : les migrations sqlx sont appliquées AVANT `axum::serve`, donc *« /health répond »* ⟹ *« migrations appliquées »* — un healthcheck HTTP sur `/health` ferme exactement cette course.
- **Actions** :
  - **`back/Dockerfile`** : ajout de `curl` à l'`apt-get install` du stage runtime (`debian:bookworm-slim` n'a ni curl ni wget). Sert UNIQUEMENT au healthcheck Docker du service back ; commenté.
  - **`docker-compose.yml`** : healthcheck sur le service `back` — `["CMD", "curl", "-fsS", "http://localhost:8080/health"]`, interval 10s / timeout 5s / retries 5 / `start_period: 30s` (laisse le temps aux migrations). `/health` renvoie toujours 200 (statut ok/degraded dans le corps) → un 200 prouve « serveur prêt = migrations OK » ; les dépendances ont leurs propres healthchecks via `depends_on`.
  - **`ingest-scheduler`** : `depends_on.back` passé de `service_started` à **`service_healthy`** ; commentaire réécrit en conséquence (rappel maintenu : l'org agglo-riviera n'existe qu'après le seed `--profile seed`, un tick antérieur au seed échoue proprement et est retenté — pas de crash).
- **Fichiers touchés** :
  - `back/Dockerfile` (modifié — `curl` runtime)
  - `docker-compose.yml` (modifié — healthcheck back + scheduler service_healthy + commentaire)
  - `logs/2026-06-08__agent-infra__back-healthcheck-scheduler.md` (créé — ce log)
- **Résultat** : OK — compose valide.
- **Vérifs** :
  - `docker compose config -q` → **OK** (0 erreur ; le healthcheck back et la condition `service_healthy` parsent, variables `.env` résolues).
  - **Build image** (apt `curl`) → couvert par le job CI « Docker — build images back & front ». Comportement runtime (healthcheck vert + scheduler qui attend `back` healthy + pas de crash-loop) à confirmer en local au prochain `docker compose up -d --build` (`docker compose ps` doit montrer `quarity_back` en `(healthy)` avant le démarrage de `quarity_ingest_scheduler`).
- **Prochaine étape** : exécuter l'action git ci-dessous ; PR vers `dev`, CI verte, squash merge.
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Branche cible : fix/infra-back-healthcheck   (depuis origin/dev — jamais master/dev en direct)
  > Script prêt : `..\commit-infra-back-healthcheck.cmd`
  > 1) git fetch origin
  > 2) git switch -c fix/infra-back-healthcheck --no-track origin/dev
  > 3) git add back/Dockerfile docker-compose.yml logs/2026-06-08__agent-infra__back-healthcheck-scheduler.md
  > 4) git commit -m "fix(infra): healthcheck Docker du back (/health) + ingest-scheduler en service_healthy"
  > 5) git push -u origin fix/infra-back-healthcheck
  > Puis : ouvrir une PR vers `dev`, attendre la CI verte, squash merge.
  > ─────────────────────────────────────────────
