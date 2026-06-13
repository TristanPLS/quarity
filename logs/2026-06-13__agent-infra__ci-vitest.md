# Log — agent-infra — 2026-06-13 — ci-vitest

## 12:25 CEST — La CI lance enfin les tests Vitest du front

- **Agent / rôle** : agent-infra
- **Jalon / tâche** : Jalon 4 (CI/CD) — constat A1 des audits.
- **Contexte** : le job `front` de la CI ne faisait que `typecheck` + `build`. Les 14 tests Vitest (`types.test.ts`, `ui.test.tsx`) existaient et passaient en local, mais **ne tournaient jamais en CI** → une régression front (helpers d'erreur, AQI level, badges) serait passée au vert sans alerte.
- **Actions** :
  - `.github/workflows/ci.yml` : ajout de l'étape `npm run test:run` (Vitest) dans le job front, après `typecheck` et avant `build`. Nom du job mis à jour (`typecheck · tests (vitest) · build`).
- **Fichiers touchés** :
  - `.github/workflows/ci.yml` (modifié)
- **Résultat** : OK.
- **Vérifs** : `npm run test:run` (front) en local → **2 fichiers, 14 tests passés** (types.test.ts 11 + ui.test.tsx 3).
- **Prochaine étape** : reste de la traîne audit — A4 intégrité AQI PG↔CH, A5 trigger T8, A3/P3 robustesse dose, A6 ESLint (PR dédiée), + volet process (resync backlog/roadmap, README final, tag v1.0).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Script : quarity/commit-fix-ci-vitest.cmd
  > Branche cible : fix/ci-front-vitest (depuis origin/dev)
  > fetch + switch -c, git add ci.yml + ce log, commit ASCII, push, gh pr create vers dev.
  > Puis : attendre la CI verte (le nouveau step inclus), squash merge.
  > ─────────────────────────────────────────────
