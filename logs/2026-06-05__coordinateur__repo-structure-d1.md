# Log — coordinateur — 2026-06-05 — repo-structure-d1

## 12:09 CEST — Rapatriement des méta-fichiers dans le dépôt (dette D1)

- **Agent / rôle** : coordinateur (axe repo)
- **Jalon / tâche** : Durcissement post-Jalon 2 — recommandation prioritaire #1 (corriger la dette D1 du backlog)
- **Contexte** : le `.git` est dans `app/`, donc `README.md`, `roadmap.md` et `logs/` (à la racine `quarity/`) n'étaient pas versionnés et les liens du README vers `docs/` étaient cassés. Décision produit (Tristan) : versionner README + roadmap + logs, mais **garder `AGENTS.md` et `CONTRIBUTING.md` hors du dépôt GitHub** (charte interne non publiée).
- **Actions** :
  - Déplacé `README.md`, `roadmap.md` et le dossier `logs/` de `quarity/` vers `quarity/app/` (désormais dans l'arbre git).
  - Laissé `AGENTS.md` et `CONTRIBUTING.md` à la racine `quarity/`, **hors** du dépôt `app/` → exclusion garantie (vérifié : `git ls-files --error-unmatch ../AGENTS.md` → « outside repository »). Aucune entrée `.gitignore` nécessaire.
  - Corrigé les liens devenus morts ou décalés :
    - `README.md` : retiré les liens `[AGENTS.md]` / `[CONTRIBUTING.md]` (remplacés par une mention « interne, non publiée ») ; les liens vers `docs/pitch.md`, `roadmap.md`, `logs/` sont à présent **valides** (ils étaient cassés avant le déplacement).
    - `logs/README.md` : déréférencé `[`AGENTS.md`](../AGENTS.md)` → mention « charte agents (interne, hors dépôt) ».
    - `docs/backlog.md` : `../../roadmap.md` → `../roadmap.md` ; `../../logs/` → `../logs/`.
    - `docs/data-model.md` : `../../roadmap.md` → `../roadmap.md`.
- **Fichiers touchés** :
  - `README.md` (déplacé vers `app/` + modifié)
  - `roadmap.md` (déplacé vers `app/`)
  - `logs/` (déplacé vers `app/`, dossier complet)
  - `logs/README.md` (modifié)
  - `docs/backlog.md` (modifié — liens relatifs)
  - `docs/data-model.md` (modifié — lien relatif)
- **Résultat** : OK — `git status` montre `README.md`, `roadmap.md`, `logs/` en untracked + `docs/backlog.md`, `docs/data-model.md` modifiés. `AGENTS.md`/`CONTRIBUTING.md` confirmés hors dépôt.
- **Vérifs** :
  - `git rev-parse --show-toplevel` → `.../quarity/app`
  - `git ls-files --error-unmatch ../AGENTS.md` → `fatal: ... outside repository` (exclusion confirmée)
  - `ls app/` → `README.md  roadmap.md  logs/  back/ db/ docs/ front/ infra/ scripts/ docker-compose.yml`
- **Prochaine étape** : recommandation prioritaire #2 — tests + CI minimale (priorité au test d'isolation cross-tenant).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Contexte : `dev` (6543c7d) n'a pas `docs/backlog.md` (créé sur la feature non mergée) → un switch vers `dev` bloque
  > sur les modifs locales de backlog.md. Le plus propre est de committer la structure SUR LA BRANCHE COURANTE
  > `feature/back-jalon2-auth-measurements`, qui porte déjà le walking skeleton + backlog (pas encore mergé sur dev).
  >
  > Prochaine étape (à exécuter par toi) — sans changer de branche :
  > 1) git add README.md roadmap.md logs/ docs/backlog.md docs/data-model.md
  > 2) git commit -m "chore(repo): rapatrier README/roadmap/logs dans le dépôt et corriger les liens (D1)"
  > 3) git push
  > NB : AGENTS.md et CONTRIBUTING.md restent volontairement hors du dépôt — ne pas les ajouter.
  > (Alternative PR D1 séparée depuis dev : `git stash push -- docs/backlog.md`, switch dev, brancher chore/repo-meta-files,
  >  ajouter README/roadmap/logs/data-model.md, commit/push, puis revenir sur la feature et `git stash pop`.)
  > ─────────────────────────────────────────────
