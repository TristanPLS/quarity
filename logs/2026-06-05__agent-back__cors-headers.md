# Log — agent-back — 2026-06-05 — cors-headers

## 15:10 CEST — CORS allowlist + en-têtes de sécurité (tower-http) — durcissement A3

- **Agent / rôle** : agent-back (axe back)
- **Jalon / tâche** : Durcissement post-Jalon 2 — lot sécurité A3 (findings D5/headers). Remplace le CORS permissif `Any` et ajoute les en-têtes de sécurité manquants.
- **Contexte** : le routeur autorisait `Any` origin/method/header (skeleton) et ne posait aucun en-tête de sécurité.
- **Actions** :
  - **CORS strict** (`back/src/routes/mod.rs`) : allowlist d'origines depuis `Config::cors_allowed_origins` (env `CORS_ALLOWED_ORIGINS`, défaut dev `http://localhost:3000`), méthodes `GET/POST`, headers `Authorization`/`Content-Type`. **Pas** d'`allow_credentials` (auth Bearer, pas de cookie ambiant).
  - **En-têtes de sécurité** via `tower-http` `SetResponseHeaderLayer` (feature `set-header` ajoutée au `Cargo.toml`) : CSP `default-src 'none'; frame-ancestors 'none'` (API JSON only), `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`, `Strict-Transport-Security`.
  - **Config** (`back/src/config.rs`) : champ `cors_allowed_origins: Vec<String>` + parsing CSV.
  - **nginx** (`front/nginx.conf`) : en-têtes sûrs côté front (nosniff, X-Frame DENY, Referrer-Policy, HSTS). CSP complète du SPA laissée à valider en navigateur (backlog A1).
  - **3 tests d'intégration** (`back/tests/e2e.rs`) : présence des en-têtes de sécurité, CORS autorise l'origine configurée, CORS refuse une origine inconnue (pas d'ACAO). → e2e passe de 9 à **12 tests**.
  - **Config d'exécution** : `.env.example` + `docker-compose.yml` (service back) reçoivent `CORS_ALLOWED_ORIGINS`.
- **Fichiers touchés** :
  - `back/src/routes/mod.rs`, `back/src/config.rs`, `back/Cargo.toml`, `back/tests/e2e.rs` (modifiés)
  - `.env.example`, `docker-compose.yml`, `front/nginx.conf` (modifiés)
- **Résultat** : OK — vérifié empiriquement.
- **Vérifs** (cargo 1.92) : `cargo fmt --all -- --check` clean ; `cargo clippy --all-targets -- -D warnings` 0 warning ; `cargo test --no-run` compile (e2e = 12 tests) ; `cargo test --lib` 5/5.
- **Correctif process** : les logs des PR #2/#3/#4 avaient été **oubliés** dans les `git add` → ils sont rattrapés dans cette livraison (commit dédié). Désormais, le fichier `logs/` est toujours inclus.
- **Prochaine étape** : A4 (rate-limit Redis sur /login & /refresh) puis A5 (validation `validator` + bornage dates) ; S1 (denylist jti au logout), S4 (découpler refresh ≠ jti).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Branche cible : feature/back-cors-headers (depuis `dev` à jour ; la branche #4 est mergée).
  > 1) git switch dev && git pull --ff-only && git switch -c feature/back-cors-headers
  > 2) git add back/src/routes/mod.rs back/src/config.rs back/Cargo.toml back/tests/e2e.rs .env.example docker-compose.yml front/nginx.conf
  >    git commit -m "feat(back): CORS allowlist + en-têtes de sécurité (tower-http) + tests"
  > 3) git add logs/2026-06-05__agent-back__cross-tenant-tests-ci.md logs/2026-06-05__agent-doc__foundations.md logs/2026-06-05__agent-back__jwt-secret-hardening.md logs/2026-06-05__agent-back__cors-headers.md
  >    git commit -m "docs(repo): journaux d'agents #2-#5 (logs oubliés des PR précédentes + A3)"
  > 4) git push -u origin feature/back-cors-headers
  > Puis : PR vers `dev`, 1 reviewer, squash merge.
  > ─────────────────────────────────────────────
