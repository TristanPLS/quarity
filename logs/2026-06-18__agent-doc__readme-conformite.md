# Log — agent-doc — 2026-06-18 — README conformité (comptes démo + env + liens)

## CEST — Comblement des 3 manques README relevés par l'audit conformité

- **Agent / rôle** : agent-doc (doc pure, aucune ligne de code touchée)
- **Jalon / tâche** : conformité sujet — Livrable L1 (README), PR `docs/readme-conformite`. Comble les 3 écarts README de l'audit ([[quarity-conformite-sujet-tp]]) : comptes démo incomplets, variables d'env non listées, pas de lien vers le dossier de conception. Suite directe des captures L4 (PR #82 mergée).
- **Contexte** : base synchronisée (`dev` = `origin/dev` @ `ca4980a`, README identique). Comptes/rôles relus en base (seed réel) et variables relevées dans `.env.example`.
- **Actions** (`README.md` uniquement) :
  - **Comptes de démo** : table étendue de 1 à **3 comptes** d'agglo-riviera (admin `sophie@`, gestionnaire `karim@`, lecteur `lecteur@` — mutation → `403 read_only_role`), tous mdp `Quarity2026!` ; + note sur les 2 autres orgs seedées (`lea@cityair.app`, `thomas@groupeindus.com`) pour démontrer l'isolation multi-tenant.
  - **Section `## Configuration (.env)`** : nouvelle table des ~18 variables essentielles (JWT_SECRET obligatoire, OPENAQ_API_KEY, identifiants PG/CH/Redis, TTL jetons, rate-limit, CORS, ports, cadences matching/ingestion) avec rôle + défaut ; renvoi à `.env.example` pour les réglages avancés (`WS_*`, `MATCHING_*`, `OPENAQ_*`).
  - **Liens** : ajout du bloc « Dossier de conception (sources) » (personas, user stories, modèle de données MCD/MLD, fondations, pitch, recette — consolidées dans le PDF L2) + « Preuves de fonctionnement (captures L4) » → `docs/captures/README.md`.
- **Fichiers touchés** : `README.md` (modifié, +30/-4) · ce log (créé).
- **Vérifs** : tous les liens internes pointent vers des fichiers existants (contrôle `test -e` sur les 9 cibles) ; comptes/rôles vérifiés contre le seed réel ; variables/défauts conformes à `.env.example` et `docker-compose.yml`. Empreinte working tree : 1 modifié + 1 log ; parasite `step` exclu.
- **Reste conformité** : **dossier de conception PDF (L2)** = dernier livrable et seul risque éliminatoire (hors `app/`, à produire en bureautique au sommaire imposé 11 sections). Puis release 7d (FF `dev→master` + tag `v1.0.0`).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Script : quarity/commit-docs-readme-conformite.cmd
  > Branche cible : docs/readme-conformite (depuis origin/dev)
  > fetch + switch -c, git add README.md + ce log, commit ASCII, push, gh pr create vers dev.
  > Puis : CI verte (docs-only), squash merge, maj-dev.cmd.
  > ─────────────────────────────────────────────
