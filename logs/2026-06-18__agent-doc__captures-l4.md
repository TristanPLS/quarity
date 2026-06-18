# Log — agent-doc — 2026-06-18 — captures L4 (preuves SGBD/requêtes/git/4xx)

## CEST — Production des captures obligatoires manquantes (Livrable L4)

- **Agent / rôle** : agent-doc (capture, lecture seule sur le code)
- **Jalon / tâche** : conformité sujet — Livrable L4, PR `docs/captures-l4`. Comble les captures « obligatoires » manquantes relevées par l'audit conformité ([[quarity-conformite-sujet-tp]]) : vue SGBD SQL, vue NoSQL, ≥3 requêtes SQL + ≥3 NoSQL exécutées, git graph, éventail 4xx. Décision Tristan : après 4b, attaquer les captures.
- **Contexte** : la stack a été levée (`docker compose up -d`, images en cache, volumes persistés) — 5 services healthy, bases déjà peuplées (PG 3 orgs/6 users/5 lieux/5 règles/6 alertes ; ClickHouse 2971 mesures stations 1001/1002/2001/3001/4085 ; Redis). **Aucune donnée de test persistante créée** (les cas 403/409/422 ne mutent rien ; les compteurs rate-limit 429 expirent en 60 s).
- **Méthode** : requêtes lancées en direct via `docker exec` (psql / clickhouse-client / redis-cli) — **sortie 100 % authentique des vrais clients**, jamais d'env secret affiché (lecture via l'env interne des conteneurs). Transcripts texte → PNG « fenêtre terminal » (thème sombre) rendus par `smoke-shots/render.js` (puppeteer-core + Edge headless, même pipeline que les captures B11b/c). Outils gardés hors dépôt (`smoke-shots/`, `smoke-shots/l4/`).
- **Requêtes SQL (PostgreSQL)** : (1) `db/sql/queries/complex_business_query.sql` canonique — CTE + 4 JOINs + GROUP BY + HAVING + agrégat `FILTER` (top orgs par alertes critiques) ; (2) lecture de la vue métier `org_alert_stats_view` ; (3) fonctions fenêtre `RANK() OVER (PARTITION …)` + `SUM() OVER`.
- **Requêtes NoSQL (ClickHouse)** : (1) `argMax(value, ingested_at)` = dédup des re-ingestions ; (2) `avgMerge(avg_state)` sur la vue matérialisée `AggregatingMergeTree` `measurements_daily` ; (3) moyenne glissante 24 h réglementaire par fonction fenêtre. + vue Redis (refresh-tokens + compteurs anti-brute-force, `GET`/`TTL`) = NoSQL clé-valeur au-delà de get/set.
- **Éventail 4xx** : 401 (sans Bearer), 403 (jeton lecteur sur mutation), 404 (id inexistant), 409 (règle en doublon, re-POST sans insertion), 422 (`threshold_value` mal typé), 429 (6 logins échoués/email = anti-brute-force 5/min). Corps d'erreur normalisés réels du back live (:8080). Codes confirmés par dry-run avant capture.
- **Git graph** : `git log --graph --oneline --all --decorate` — escalier branches/merges (1 branche/PR, squash → `dev`), 94 commits / 75 branches distantes / 78 PR mergées / tag `v1.0.0-rc.1`.
- **Fichiers créés** (PR) :
  - `docs/captures/2026-06-18__{sgbd-postgres,sgbd-clickhouse,sgbd-redis,requetes-sql,requetes-nosql,git-graph,api-4xx}.png` (7)
  - `docs/captures/README.md` — index des 25 captures mappées aux exigences L4.
- **Vérifs** : 6 codes 4xx confirmés en dry-run ; 6 requêtes (3 SQL + 3 NoSQL) renvoient des résultats réels non vides ; rendu PNG inspecté (alignement des tableaux box-drawing OK, lisible). Empreinte working tree : 7 PNG + 1 README + ce log ; **zéro parasite de rendu** ; parasite préexistant `step` exclu.
- **Reste conformité** (ordre Tristan) : **README** (compte démo standard `lecteur@agglo-riviera.fr`/`Quarity2026!` + variables d'env + lien dossier) → **dossier de conception PDF (L2)** (seul risque éliminatoire, hors `app/`) → release 7d.
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Script : quarity/commit-docs-captures-l4.cmd
  > Branche cible : docs/captures-l4 (depuis origin/dev)
  > fetch + switch -c, git add (7 PNG + README captures + ce log), commit ASCII, push, gh pr create vers dev.
  > Puis : CI verte (docs-only), squash merge, maj-dev.cmd.
  > ─────────────────────────────────────────────
