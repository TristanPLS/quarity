# Log — review — 2026-06-07 — analyse-projet

## 13:30 CEST — Analyse globale du projet (6 agents : docs, back, front, data/infra, journal + cohérence croisée)

- **Agent / rôle** : review (coordinateur de 5 agents lecteurs + 1 vérificateur adversarial)
- **Jalon / tâche** : transverse — état des lieux complet à la demande de Tristan
- **Contexte** : analyse en lecture seule (zéro modification de code, zéro action git). Chaque sous-système lu par un agent dédié, puis une passe de cohérence croisée a contre-vérifié les rapports dans les fichiers et l'historique git (lecture seule).
- **Actions** :
  - Lecture exhaustive : docs produit, back Rust (src + tests), front React, db/compose/ingestion, les 18 logs du journal.
  - Vérification croisée : backlog/roadmap vs code mergé sur `dev`, journal vs état git réel, schémas DB vs requêtes du back, contrats front vs endpoints back.
- **Constats majeurs** (détail dans la synthèse remise à Tristan) :
  1. **Le rattrapage du 07/06 a bien été exécuté** mais aucun log ne le trace : commits `93067a4`/`1b06edf`/`53693ce` présents, PR #13/#14/#15 squash-mergées dans `dev` (`eedd62a`), script `..\rattrapage-lots.cmd` supprimé. Le journal annonce donc à tort une action urgente encore bloquante. — Ce log clôt ce point.
  2. **Lot « migrations sqlx + COMMENT unités AQI » invisible** : la branche `feature/bdd-migrations-sqlx` (`1b06edf`) est poussée mais NON mergée ; son log de mission n'existe que sur cette branche. Elle répond aux critiques n°1 et n°2 de la revue du 06/06. Basée sur `2664ab7` (avant les merges #13–#15) → rebase nécessaire avant PR.
  3. **backlog.md périmé** : A4 (rate-limit) livré non coché (nuance : fenêtre fixe email/IP, pas de token bucket ni quota par clé API) ; A6 partiellement livré (pagination/retry/idempotence mergés, restent scheduler + backfill S3) ; « 12 tests e2e » → 15 réels ; mission migrations-sqlx absente du backlog.
  4. **Signalement jamais traité** : `REFRESH_TTL_SECS` et `RATE_LIMIT_*` absents de `.env.example` alors que `config.rs` les consomme (demandé par le log auth-hardening du 06/06).
  5. **Mineurs vérifiés** : `roadmap.md:50` reliquat « 1 review » contredisant le process solo ; incohérence COMMENT/seed unités AQI toujours sur `dev` (corrigée seulement sur la branche non mergée) ; `quarity.` en dur dans ch.rs/ingest.rs ; scaffolding résiduel (bloc front commenté du compose, FRONT_PORT dupliqué dans .env.example, back/run.log, .gitkeep) ; un stash du 04/06 (`refs/stash`, a8d4640) jamais purgé — à inspecter avant suppression.
  - Cohérences confirmées : schémas Postgres/ClickHouse ↔ requêtes du back alignés ; les 5 routes appelées par le front existent côté back avec types alignés champ à champ ; Dockerfile back renomme bien le binaire `ingest` en `quarity-ingest`.
- **Fichiers touchés** :
  - `logs/2026-06-07__review__analyse-projet.md` (créé — ce log)
  - `../commit-log-analyse.cmd` (créé — hors dépôt, jetable après exécution : branche + commit + push + PR de ce log)
- **Résultat** : analyse remise à Tristan ; aucun code modifié.
- **Vérifs** : chaque incohérence signalée a été contre-vérifiée dans les fichiers et les objets git (lecture seule) par l'agent de cohérence ; le faux positif du rapport front (« compteur Points divergent du graphe ») a été infirmé et retiré.
- **Prochaine étape** (ordre suggéré) :
  1. Rebaser `feature/bdd-migrations-sqlx` sur `dev`, ouvrir la PR, tester l'impact volume existant (retrait initdb.d), merger à la CI verte.
  2. Resynchroniser `docs/backlog.md` (A4/A6 annotés, 15 tests, mission migrations) + corriger `roadmap.md:50`.
  3. Ajouter `REFRESH_TTL_SECS` et `RATE_LIMIT_*` à `.env.example`.
  4. Nettoyage mineur (compose, .env.example, run.log, .gitkeep, stash après inspection).
  5. Reprendre le fil : A1 (vérif visuelle + captures + CSP), A2 (OpenAPI/utoipa).
- **Action Git suggérée à l'humain** :
  > Exécuter le script `..\commit-log-analyse.cmd` (branche `docs/review-analyse-2026-06-07` depuis `dev` + commit + push + PR), puis merger à la CI verte et supprimer le script.
