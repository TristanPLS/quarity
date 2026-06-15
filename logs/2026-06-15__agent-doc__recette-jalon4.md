# Log — agent-doc — 2026-06-15 — recette Jalon 4 (Vague 3 : 7a / C1)

## CEST — Recette fonctionnelle attendu/obtenu (plan-finition-v1, cluster 7)

- **Agent / rôle** : agent-doc
- **Jalon / tâche** : finition v1.0 — Vague 3, PR `docs/recette-jalon4` (7a = backlog C1). Doc pure, zéro risque CI.
- **Contexte** : le Jalon 4 exige une **recette** (tableau de tests fonctionnels manuels attendu vs obtenu). Manquait.
- **Actions** :
  - `docs/recette.md` (créé) : recette structurée en **10 domaines** (auth/sécu, RBAC+isolation, lieux, règles+temps réel, dashboard/AQI/carte, profils/seuils, expositions/dose, API publique/quotas, santé/exploitation, +périmètre/conclusion). ~40 scénarios `attendu/obtenu`, chacun avec code HTTP attendu + preuve (capture `docs/captures/` OU test automatisé).
  - Rattaché aux **16 user stories** (critères d'acceptation) et au **backlog** (statut livré). Section **« Périmètre & exclusions »** honnête : US-10 (ranking, vue BDD sans UI), US-15 (destinataires/`notification_deliveries`, push éphémère non câblé), US-07 (moyennes glissantes validées en BDD sans endpoint front) = reportés ; cluster 8 → v1.1.
  - Section **reproduction** : stack compose, seed, ingest 4085, compte `sophie@agglo-riviera.fr`.
  - `docs/backlog.md` : C1 coché.
- **Fichiers touchés** :
  - `docs/recette.md` (créé)
  - `docs/backlog.md` (C1 → [x])
- **Vérifs** :
  - **Toutes les références vérifiées existantes** : 13 fichiers de tests (`back/tests/*.rs`) + **18 captures** (`docs/captures/*.png`) — zéro lien cassé (script de contrôle).
  - Doc pure → aucun impact CI/code.
- **Prochaine étape Vague 3** : `perf/c2-benchmarks-matching` (7b) · `chore/ci-supply-chain-audit` (7c) · 5e capture Swagger → puis release 7d.
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Script : quarity/commit-docs-recette-jalon4.cmd
  > Branche cible : docs/recette-jalon4 (depuis origin/dev)
  > fetch + switch -c, git add docs/recette.md + docs/backlog.md + ce log, commit ASCII, push, gh pr create vers dev.
  > Puis : attendre la CI verte, squash merge, maj-dev.cmd.
  > ─────────────────────────────────────────────
