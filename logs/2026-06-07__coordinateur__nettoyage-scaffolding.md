# Log — coordinateur — 2026-06-07 — nettoyage-scaffolding

## 15:19 CEST — Nettoyage des restes de scaffolding + purge du stash du 04/06

- **Agent / rôle** : coordinateur
- **Jalon / tâche** : point 4 de l'analyse du 2026-06-07 (logs/2026-06-07__review__analyse-projet.md) — hygiène du dépôt, aucun changement de comportement.
- **Contexte** : restes du Jalon 2 identifiés par l'analyse et contre-vérifiés : bloc front commenté redondant en fin de docker-compose.yml, bloc « Front (Jalon 2) [décommenter…] » de .env.example dupliquant `FRONT_PORT` (déjà actif 2 lignes plus haut), `back/run.log` vide (untracked, déjà couvert par `.gitignore:42 *.log`), 3 `.gitkeep` résiduels dans des dossiers désormais peuplés, et un stash du 04/06 jamais purgé.
- **Actions** :
  - `docker-compose.yml` : suppression du bloc front commenté (7 lignes, reliquat d'avant l'activation du service réel).
  - `.env.example` : suppression du bloc « Front (Jalon 2) » (3 lignes — `FRONT_PORT` dupliqué + `VITE_API_BASE_URL` documentée côté `front/.env.example`).
  - `back/run.log` : supprimé du disque (untracked + ignoré + vide — aucune action git nécessaire).
  - `.gitkeep` : retrait de `back/.gitkeep`, `front/.gitkeep`, `logs/.gitkeep` (dossiers peuplés — via `git rm` dans le script). **Conservés** : `docs/captures/`, `db/sql/queries/`, `db/clickhouse/queries/`, `infra/`, `scripts/` (dossiers vides en attente de A1/B4/B5).
  - Stash `stash@{0}` (« On feature/infra-jalon2-socle-db: back-jalon2 ») : **inspecté avant purge** (`git stash show -p`) — instantané intermédiaire de l'activation du service back dans compose/.env.example, entièrement remplacé par les versions mergées sur `dev`. Purge via le script (`git stash drop`).
- **Fichiers touchés** :
  - `docker-compose.yml` (modifié — 7 lignes supprimées)
  - `.env.example` (modifié — 3 lignes supprimées)
  - `back/run.log` (supprimé du disque — hors git)
  - `back/.gitkeep`, `front/.gitkeep`, `logs/.gitkeep` (suppression préparée via `git rm` dans le script)
  - `logs/2026-06-07__coordinateur__nettoyage-scaffolding.md` (créé — ce log)
- **Résultat** : préparé — exécution par l'humain.
- **Vérifs** : `run.log` confirmé untracked (`git ls-files`) et ignoré ; les 8 `.gitkeep` inventoriés et triés (3 résiduels / 5 légitimes) ; contenu du stash lu intégralement avant de proposer la purge ; YAML du compose re-parsé valide après suppression du bloc.
- **Prochaine étape** : A1 (vérif visuelle front + captures dans `docs/captures/` + CSP nginx), puis A2 (OpenAPI/utoipa).
- **Action Git suggérée à l'humain** :
  > Exécuter le script `..\nettoyage-0607.cmd` (branche `chore/repo-nettoyage-0607` + git rm des 3 .gitkeep + commit + push + PR + `git stash drop` du stash périmé), puis merger à la CI verte.
