# Log — agent-infra — 2026-06-15 — audits supply-chain CI (Vague 3 : 7c / C3)

## CEST — npm audit + cargo audit (advisory) en CI (plan-finition-v1, cluster 7)

- **Agent / rôle** : agent-infra
- **Jalon / tâche** : finition v1.0 — Vague 3, PR `chore/ci-supply-chain-audit` (7c = backlog C3 ; ESLint déjà fait en #65).
- **Contexte** : la CI faisait fmt/clippy/tests/build/docker/vitest/eslint mais **aucun audit de chaîne d'approvisionnement** (dépendances vulnérables).
- **Actions** (`.github/workflows/ci.yml`) :
  - **Step `npm audit --omit=dev`** ajouté DANS le job `front` (après `npm ci`, `continue-on-error: true`). `--omit=dev` : seules les deps de PROD comptent (les 5 vulns connues = devDeps d'outillage/test, hors bundle).
  - **Nouveau job `supply-chain`** (`name: "Supply-chain — cargo audit (advisory)"`) : checkout + `dtolnay/rust-toolchain@stable` + `cargo install cargo-audit --locked` + `cargo audit` (working-dir `back`, `continue-on-error: true`).
  - ⚠️ **Aucun des 3 jobs requis renommé** (back/front/docker = required status checks) ; le nouveau job a un **nom distinct** → pas de collision (cf. [[ci-required-checks-noms-figes]]). Job **non requis** → un avis ne bloque pas la PR.
  - **Phase 1 = advisory** (`continue-on-error`) : signale sans bloquer. Phase 2 (post-v1.0) : rendre bloquant + ajouter aux required checks.
  - `docs/backlog.md` : C3 → [x].
- **Fichiers touchés** :
  - `.github/workflows/ci.yml` (modifié)
  - `docs/backlog.md` (C3 → [x])
- **Vérifs (la CI yaml ne se teste qu'en la poussant)** :
  - **YAML valide** (`yaml.safe_load`).
  - Les **3 jobs requis présents et inchangés** (vérif `issubset` sur les noms exacts) ; nouveau job `supply-chain` distinct ; step `npm audit` bien dans le job `front`.
  - Comportement attendu : `npm audit`/`cargo audit` peuvent trouver des avis → étape/job en « failed » mais **continue-on-error** ⇒ pas de blocage de la PR (phase 1).
- **Prochaine étape Vague 3** : **5e** capture Swagger (~48 chemins ; rebuild back avant) → puis release **7d**.
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Script : quarity/commit-chore-ci-supply-chain-audit.cmd
  > Branche cible : chore/ci-supply-chain-audit (depuis origin/dev)
  > fetch + switch -c, git add ci.yml + docs/backlog.md + ce log, commit ASCII, push, gh pr create vers dev.
  > Puis : attendre la CI verte (les 3 jobs requis + le nouveau job advisory), squash merge, maj-dev.cmd.
  > ─────────────────────────────────────────────
