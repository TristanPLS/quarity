# Log — agent-back — 2026-06-05 — jwt-secret-hardening

## 14:40 CEST — Rejet au boot d'un JWT_SECRET placeholder ou trop faible

- **Agent / rôle** : agent-back (axe back)
- **Jalon / tâche** : Durcissement post-Jalon 2 — recommandation prioritaire #4 (finding sécurité S3). Le `JWT_SECRET` placeholder du `.env.example` (`change_me_…`) passait le seul contrôle existant (longueur ≥ 32) → un déploiement l'oubliant démarrait avec un secret PUBLIC = tous les JWT forgeables.
- **Contexte** : transformer le contrôle de longueur en validation de robustesse, refusée au démarrage, avec tests.
- **Actions** :
  - `back/src/config.rs` : extraction d'une fonction pure `validate_jwt_secret(&str) -> Result<(), String>`, appelée dans `Config::from_env`. Rejette : longueur < 32 (inchangé), **marqueurs de placeholder** (`change_me`, `changeme`, `placeholder`, `your_secret`, `to_change`, `example`, `secret_at_least_32`, insensible à la casse), **entropie trop faible** (< 8 caractères distincts → attrape `aaaa…`, `ababab…`).
  - **5 tests unitaires** (`#[cfg(test)] mod tests`, sans DB) : accepte un secret hex aléatoire ; rejette trop court, le placeholder exact du `.env.example`, `CHANGE_ME` (casse), faible entropie.
  - `.github/workflows/ci.yml` : `JWT_SECRET` de CI remplacé par une vraie valeur hex (`7f3b9c1d…`) pour que les tests d'intégration passent la nouvelle validation (sans coupler le test à un marqueur).
  - `.env.example` : commentaire mis à jour (le back REFUSE de démarrer avec le placeholder ; `openssl rand -hex 32`).
- **Fichiers touchés** :
  - `back/src/config.rs` (modifié — `validate_jwt_secret` + tests)
  - `.github/workflows/ci.yml` (modifié — JWT_SECRET de CI)
  - `.env.example` (modifié — commentaire)
- **Résultat** : OK — vérifié empiriquement.
- **Vérifs** (cargo 1.92) :
  - `cargo fmt --all -- --check` → clean
  - `cargo clippy --all-targets -- -D warnings` → 0 warning
  - `cargo test --lib` → **5 passed ; 0 failed** (config::tests::*)
- **⚠️ Impact comportemental** : après cette PR, le back **refuse de démarrer** si `JWT_SECRET` est le placeholder. Les devs doivent mettre une vraie valeur dans leur `.env` local (sinon `docker compose up` / `cargo test` échoue au boot). C'est l'effet recherché.
- **Prochaine étape** : lot de durcissement A3–A5 (CORS allowlist + headers tower-http, rate-limit Redis, validation `validator`) + findings S1 (denylist jti au logout) / S4 (découpler refresh ≠ jti).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Branche cible : feature/back-jwt-secret-hardening (depuis `dev`)
  > 1) git switch dev && git pull --ff-only && git switch -c feature/back-jwt-secret-hardening
  > 2) git add back/src/config.rs .github/workflows/ci.yml .env.example
  > 3) git commit -m "feat(back): rejeter au boot un JWT_SECRET placeholder ou trop faible (+ tests)"
  > 4) git push -u origin feature/back-jwt-secret-hardening
  > Puis : PR vers `dev`, 1 reviewer, squash merge.
  > NB : mettre une vraie valeur JWT_SECRET dans le `.env` local (openssl rand -hex 32) avant de relancer la stack.
  > ─────────────────────────────────────────────
