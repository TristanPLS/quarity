# Log — agent-doc — 2026-06-07 — resync-backlog-roadmap

## 14:18 CEST — Resynchronisation backlog/roadmap après les merges #14–#16 + pointeurs migrations

- **Agent / rôle** : agent-doc
- **Jalon / tâche** : point 2 de l'analyse du 2026-06-07 (logs/2026-06-07__review__analyse-projet.md) — la doc d'avancement avait un cran de retard sur `dev`.
- **Contexte** : les lots auth-hardening (PR #14), ingest-robustesse (PR #15) et migrations sqlx (PR #16) sont mergés, mais `backlog.md` (daté du 06/06) marquait encore A4 « à faire », ignorait la mission migrations et annonçait 12 tests e2e (15 réels). `roadmap.md:50` gardait un reliquat « 1 review » contredisant le process solo.
- **Actions** :
  - `docs/backlog.md` :
    - Ligne « Dernière mise à jour » → 2026-06-07 (lots #14/#15/#16, critiques n°1 et n°2 résolues).
    - Jalon 1 : référence schéma → `back/migrations/0001_init.sql` (gelé, appliqué au boot — PR #16 ; `db/sql/01_schema.sql` = pointeur).
    - Jalon 2 : « 12 tests e2e » → « 15 tests e2e (12 le 05/06 + 3 au durcissement auth) » ; ligne docker-compose réactualisée (profils `seed`/`ingest`, initdb.d ClickHouse seul, schéma Postgres via migrations au boot).
    - Nouvelle sous-section « Post-revue du 06/06 — durcissement & socle (mergé le 2026-06-07) » : 3 entrées (PR #14, #15, #16) avec renvois vers leurs logs ; attribution précise des critiques (n°1 + volet schéma de la n°2 → #16 ; volet ingestion de la n°2 → #15).
    - A4 coché ✅ (rate-limit fenêtre fixe login email+IP / refresh IP, PR #14) avec reste explicite → B9 (token bucket généralisé + quota par clé API).
    - A6 annoté : pagination/retry ✅ (PR #15), idempotence préexistante confirmée ; restent scheduler + backfill S3.
    - Repères techniques : nouvelle procédure de boot (schéma via migrations sqlx au démarrage du back) + commande seed `docker compose --profile seed run --rm seed`.
  - `roadmap.md:50` : « PR obligatoire, 1 review » → « PR obligatoire + CI verte — 0 review humaine » (process solo acté le 06/06).
  - `db/sql/02_seed.sql` (en-tête) : « Prérequis : 01_schema.sql appliqué » → migrations sqlx (`back/migrations/0001_init.sql`) + commande d'exécution du seed — follow-up explicitement demandé par le lot migrations (logs/2026-06-06__agent-bdd__migrations-sqlx.md, entrée 00:03).
- **Fichiers touchés** :
  - `docs/backlog.md` (modifié — 8 retouches)
  - `roadmap.md` (modifié — 1 ligne)
  - `db/sql/02_seed.sql` (modifié — en-tête, commentaire uniquement)
  - `logs/2026-06-07__agent-doc__resync-backlog-roadmap.md` (créé — ce log)
- **Résultat** : OK — doc d'avancement réalignée sur l'état réel de `dev` (`9043db0`).
- **Vérifs** : chaque affirmation ajoutée provient de l'analyse croisée du 2026-06-07 (vérifiée fichier par fichier) ; nombre de tests e2e recompté ; numéros de PR confirmés via l'historique de `dev` ; passe de vérification adversariale indépendante effectuée (a relevé la sur-attribution de la critique n°2 à la PR #16 et la ligne docker-compose périmée — toutes deux corrigées avant commit).
- **Prochaine étape** : A1 (vérif visuelle front + captures + CSP nginx), A2 (OpenAPI/utoipa) ; nettoyage mineur restant (bloc front commenté du compose, FRONT_PORT dupliqué dans .env.example, back/run.log, .gitkeep, stash du 04/06 à inspecter).
- **Action Git suggérée à l'humain** :
  > Exécuter le script `..\commit-lots-0607.cmd` (lot doc + lot infra), puis merger les 2 PR aux CI vertes.
