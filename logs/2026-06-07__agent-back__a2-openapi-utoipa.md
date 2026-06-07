# Log — agent-back — 2026-06-07 — a2-openapi-utoipa

## 18:25 CEST — Mission A2 : doc OpenAPI `/api/docs` (utoipa + Swagger UI vendored)

- **Agent / rôle** : agent-back
- **Jalon / tâche** : Jalon 2 (reliquat) — backlog **A2** : doc OpenAPI auto-générée sur `/api/docs`.
- **Contexte** : item différé volontairement au Jalon 2 ; prochain item ouvert de la section A du backlog (A1 l'attend pour la capture Swagger). Préalable de fait à B6 (documenter 16+ endpoints CRUD sans utoipa en place = travail en double).
- **Actions** :
  - Deps : `utoipa 5.5` (feature `axum_extras`) + `utoipa-swagger-ui 9.0.2` (features `axum`, `vendored` — assets embarqués, zéro réseau au build Docker/CI).
  - Nouveau module `back/src/openapi.rs` : `ApiDoc` (info + tags), `SecurityAddon` (schéma `bearer_jwt` HTTP Bearer/JWT), schémas **doc-only** (`ErrorBody`, `HealthResponse`, `LogoutResponse`, `MeResponse`, `MeasurementsPage`) décrivant les JSON réels des handlers — **zéro changement de comportement** des endpoints ; `docs_router()` avec en-têtes dédiés.
  - Annotations `#[utoipa::path]` sur les 6 handlers (health, login, refresh, logout, me, measurements) ; `ToSchema` sur `LoginRequest`/`TokenResponse`/`RefreshRequest`/`LogoutRequest`/`MeasurementRow` ; `IntoParams` sur `MeasurementsQuery` (descriptions + exemples conformes au seed).
  - Codes d'erreur documentés exhaustivement, y compris les rejets des extracteurs axum (400 JSON malformé, 415 Content-Type, 422 champs manquants — corps texte, distingués des erreurs applicatives `ErrorBody`).
  - Montage : `docs_router()` mergé **après** les layers API dans `routes/mod.rs` (la CSP API `default-src 'none'` casserait l'UI) ; CSP propre à Swagger (`script-src 'self'`, `style-src 'self' 'unsafe-inline'`, `img-src 'self' data:`) + nosniff + X-Frame DENY + Referrer + HSTS.
  - **nginx** : bloc dédié `location /api/docs/` dans `front/nginx.conf` — un `add_header` local (X-Robots-Tag noindex, utile en soi) coupe l'héritage des en-têtes serveur, sinon **double CSP** (SPA + back) → intersection navigateur → icônes `data:` de swagger-ui.css bloquées.
  - +3 tests e2e : UI servie (200, HTML, CSP docs, nosniff), redirection `/api/docs` → `/api/docs/`, spec complète (6 routes, `bearer_jwt`, 403 multi-tenant documenté). Total : **18 tests e2e**.
  - Doc resynchronisée : backlog A2 coché (+ compte 18 tests), roadmap Jalon 2 ligne `/api/docs` cochée, README pointe http://localhost:3000/api/docs.
- **Fichiers touchés** :
  - `back/Cargo.toml`, `back/Cargo.lock` (modifiés — deps utoipa)
  - `back/src/openapi.rs` (créé)
  - `back/src/lib.rs`, `back/src/ch.rs`, `back/src/routes/mod.rs`, `back/src/routes/auth.rs`, `back/src/routes/health.rs`, `back/src/routes/measurements.rs` (modifiés)
  - `back/tests/e2e.rs` (modifié — +3 tests)
  - `front/nginx.conf` (modifié — bloc `/api/docs/` anti double-CSP)
  - `docs/backlog.md`, `roadmap.md`, `README.md` (modifiés — resync A2)
  - `logs/2026-06-07__agent-back__a2-openapi-utoipa.md` (créé — ce log)
  - `../commit-a2-openapi.cmd`, `../pr-a2-body.md` (créés — hors dépôt, jetables après exécution)
- **Résultat** : OK — livré, vérifié en local de bout en bout.
- **Vérifs** :
  - `cargo fmt --all -- --check` → OK ; `cargo clippy --all-targets -- -D warnings` → 0 warning.
  - `cargo test --all -- --test-threads=1` (vraies bases du compose) → **28/28** (10 unitaires + 18 e2e).
  - HTML Swagger inspecté (binaire local) : **zéro script inline** → CSP `script-src 'self'` validée empiriquement ; initializer externe pointant `/api/docs/openapi.json`.
  - `docker compose up -d --build back front` puis à travers nginx (port 3000) : `/api/docs/` **200 avec UNE seule CSP** (celle du back), `openapi.json` 200, `swagger-ui-bundle.js` 200, `X-Robots-Tag: noindex` présent.
  - Revue adversariale multi-agents (3 lentilles × contre-vérification) : 4 findings confirmés et **corrigés** (400/415/422 manquants sur refresh/logout/login, description 400 measurements élargie aux rejets `Query`, exemple `role` corrigé `org_admin` → `admin` + `full_name` conforme au seed) ; 2 faux positifs réfutés.
  - Observation hors périmètre (préexistant, inoffensif) : les réponses JSON `/api/*` portent une double CSP (back + SPA nginx) depuis A1+A3 — sans effet sur du JSON ; à nettoyer un jour si souhaité.
- **Prochaine étape** : après merge — capture Swagger dans `docs/captures/` (solde le reliquat d'A1, règle de discipline #1) ; puis mission **A5** (validation des inputs, crate `validator`) sur une branche fraîche depuis `dev` à jour.
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Exécuter le script `..\commit-a2-openapi.cmd` (depuis la racine `quarity/`), qui fait :
  > Branche cible : feature/back-openapi-utoipa     (créée depuis `dev` à jour — jamais master/dev en direct)
  > 1) git add back/Cargo.toml back/Cargo.lock back/src/openapi.rs back/src/lib.rs back/src/ch.rs back/src/routes/mod.rs back/src/routes/auth.rs back/src/routes/health.rs back/src/routes/measurements.rs back/tests/e2e.rs front/nginx.conf docs/backlog.md roadmap.md README.md logs/2026-06-07__agent-back__a2-openapi-utoipa.md
  > 2) git commit -m "feat(back): A2 - doc OpenAPI /api/docs (utoipa + swagger-ui vendored), CSP dediee, +3 tests e2e"
  > 3) git push -u origin feature/back-openapi-utoipa
  > 4) gh pr create --base dev (corps : ../pr-a2-body.md)
  > Puis : attendre la CI verte (3 checks), squash merge, supprimer les 2 fichiers jetables.
  > ─────────────────────────────────────────────
