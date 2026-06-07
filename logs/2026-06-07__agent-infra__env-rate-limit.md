# Log — agent-infra — 2026-06-07 — env-rate-limit

## 14:18 CEST — Exposer REFRESH_TTL_SECS et RATE_LIMIT_* (.env.example + docker-compose)

- **Agent / rôle** : agent-infra
- **Jalon / tâche** : point 3 de l'analyse du 2026-06-07 — traiter le signalement resté sans suite du lot auth-hardening (logs/2026-06-06__agent-back__auth-hardening.md, « Prochaine étape »).
- **Contexte** : `back/src/config.rs:59-62` consomme `REFRESH_TTL_SECS` (déf. 604800 = 7 j), `RATE_LIMIT_LOGIN_EMAIL_PER_MIN` (déf. 5), `RATE_LIMIT_LOGIN_IP_PER_MIN` (déf. 20) et `RATE_LIMIT_REFRESH_IP_PER_MIN` (déf. 30), mais ces variables n'étaient ni documentées dans `.env.example` ni forwardées par le bloc `environment:` (liste explicite) du service `back` — impossible de les régler via `.env` en compose.
- **Actions** :
  - `.env.example` : ajout des 4 variables avec leurs valeurs par défaut + commentaire (section back, après CORS).
  - `docker-compose.yml` (service `back`, bloc `environment:`) : forwarding des 4 variables avec défauts compose alignés sur ceux du code (`${VAR:-défaut}`), insérées après `JWT_ACCESS_TTL_SECONDS` pour garder le groupement auth.
  - **Extension de périmètre suite à la vérification adversariale** : `OPENAQ_MAX_PAGES` (consommé par `back/src/bin/ingest.rs`, défaut 50) souffrait du même problème côté service `ingest` → forwardé dans `ingest.environment` (`${OPENAQ_MAX_PAGES:-50}`) + les 4 réglages d'ingestion documentés en commentaire dans la section OpenAQ de `.env.example` (LOCATION_ID/DAYS/ORG_SLUG étaient déjà forwardés, juste non documentés).
- **Fichiers touchés** :
  - `.env.example` (modifié — section back : 5 lignes ; section OpenAQ : 2 lignes de commentaire)
  - `docker-compose.yml` (modifié — 4 lignes dans `back.environment`, 1 ligne dans `ingest.environment`)
  - `logs/2026-06-07__agent-infra__env-rate-limit.md` (créé — ce log)
- **Résultat** : OK — les 5 variables sont désormais réglables via `.env` ; en leur absence, double filet de défauts identiques (compose `:-` → code).
- **Vérifs** : noms et défauts vérifiés contre `back/src/config.rs:59-62` (env_u64) et `back/src/bin/ingest.rs` (OPENAQ_MAX_PAGES, défaut 50) ; YAML du compose re-parsé valide après modification ; vérification adversariale indépendante passée (noms caractère par caractère, défauts code/compose/.env.example cohérents, indentation conforme).
- **Prochaine étape** : aucune sur ce périmètre.
- **Action Git suggérée à l'humain** :
  > Exécuter le script `..\commit-lots-0607.cmd` (lot doc + lot infra), puis merger les 2 PR aux CI vertes.
