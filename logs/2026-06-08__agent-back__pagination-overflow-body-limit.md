# Log — agent-back — 2026-06-08 — pagination-overflow-body-limit

## 22:52 CEST — Mineurs back : offset de pagination borné + DefaultBodyLimit /auth

- **Agent / rôle** : agent-back
- **Jalon / tâche** : durcissement post-analyse (rapport multi-agents du 2026-06-08) — deux mineurs back confirmés.
- **Contexte** : (1) `measurements` calculait `offset = (page - 1) * page_size` en `u32` sans borne haute sur `page` — en build release (profil sans overflow-checks), un `page` énorme wrappe silencieusement l'offset → page de résultats fausse. (2) La borne anti-DoS du mot de passe (`max=512`) ne s'applique qu'APRÈS désérialisation complète du JSON ; aucune borne de corps explicite n'était posée, la seule protection étant la limite axum implicite de 2 Mio.
- **Actions** :
  - **Offset borné** (`back/src/routes/measurements.rs`) : `page` reçoit une validation déclarative `#[validate(range(min = 1, max = 1000000))]` (cohérent avec A5 → 400 `bad_request` hors borne) ; l'offset est calculé en `u64` puis `u32::try_from(...).unwrap_or(u32::MAX)` (sature au lieu de wrapper). Plus de débordement silencieux, comportement debug/release identique.
  - **Borne de corps /auth** (`back/src/routes/mod.rs`) : sous-routeur `auth_routes` (login/refresh/logout/me) portant `DefaultBodyLimit::max(8 * 1024)` puis `merge` dans le routeur principal. Un corps > 8 Kio sur `/auth` est rejeté en **413** AVANT désérialisation. Rend la borne anti-DoS visible/auditable ; n'affecte pas `/measurements` ni les futurs endpoints B6.
  - **2 tests e2e** (`back/tests/e2e.rs`) : `measurements_out_of_range_page_is_bad_request` (`page=5000000` → 400 `bad_request`) ; `oversized_auth_body_is_rejected` (corps ~64 Kio → 413).
- **Fichiers touchés** :
  - `back/src/routes/measurements.rs` (modifié — validation `page` + offset u64/try_from)
  - `back/src/routes/mod.rs` (modifié — sous-routeur auth + `DefaultBodyLimit`)
  - `back/tests/e2e.rs` (modifié — +2 tests)
  - `logs/2026-06-08__agent-back__pagination-overflow-body-limit.md` (créé — ce log)
- **Résultat** : OK — code en place, portes CI locales vertes.
- **Vérifs** :
  - `cargo check --tests` → **OK** (confirme `#[validate(range)]` sur `Option<u32>` et l'import `axum::extract::DefaultBodyLimit`).
  - `cargo clippy --all-targets -- -D warnings` → **OK** ; `cargo fmt --check` → **OK**.
  - e2e contre les vraies bases → **délégués à la CI**.
- **Prochaine étape** : exécuter l'action git ci-dessous ; PR vers `dev`, CI verte, squash merge. Mineurs restants non traités ici (autres axes) : healthcheck Docker du `back` (infra) ; dépoussiérer `foundations.md §4` (doc).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Branche cible : fix/back-pagination-overflow-body-limit   (depuis origin/dev — jamais master/dev en direct)
  > Script prêt : `..\commit-back-pagination-body-limit.cmd`
  > 1) git fetch origin
  > 2) git switch -c fix/back-pagination-overflow-body-limit --no-track origin/dev
  > 3) git add back/src/routes/measurements.rs back/src/routes/mod.rs back/tests/e2e.rs logs/2026-06-08__agent-back__pagination-overflow-body-limit.md
  > 4) git commit -m "fix(back): borne offset pagination (anti-overflow) + DefaultBodyLimit /auth + 2 tests"
  > 5) git push -u origin fix/back-pagination-overflow-body-limit
  > Puis : ouvrir une PR vers `dev`, attendre la CI verte, squash merge.
  > ─────────────────────────────────────────────
