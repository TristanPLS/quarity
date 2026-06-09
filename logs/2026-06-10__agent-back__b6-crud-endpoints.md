# Log — agent-back — 2026-06-10 — b6-crud-endpoints

## 01:30 CEST — B6 : CRUD complet (20 endpoints) users / organizations / tracked-locations / alert-rules

- **Agent / rôle** : agent-back (orchestrateur + 3 agents d'implémentation parallèles + 3 relecteurs adversariaux)
- **Jalon / tâche** : Jalon 3 — axe back, **B6** (backlog.md). Première grosse brique après la clôture de l'axe BDD (B1–B5) et le durcissement #28–#33.
- **Contexte** : seules 6 routes existaient (auth ×4, measurements, health). La roadmap exige « CRUD sur 4 ressources (= 16+ endpoints), codes HTTP corrects, pagination + filtrage + tri sur tous les listings ». Risque structurel identifié à l'analyse d'ouverture : l'isolation org se vérifiait À LA MAIN dans chaque handler — sur 20 endpoints, un oubli = fuite cross-tenant.
- **Actions** :
  - **Socle transverse** :
    - `back/src/error.rs` : `AppError::Conflict` (409 `conflict`) + `Unprocessable` (422 `unprocessable_entity`) ; `From<sqlx::Error>` mappe 23505 (unique) → 409 avec message par contrainte, 23514/23503 (triggers `QRT_*`, CHECK, FK) → 422. Les SELECT d'auth ne produisent pas ces codes : comportement inchangé.
    - `back/src/security.rs` : extracteurs **`CanWrite`** (403 `read_only_role`) et **`RequireAdmin`** (403 `admin_required`) — la garde RBAC est dans la SIGNATURE du handler, in-oubliable.
    - `back/src/listing.rs` (nouveau) : `ListParams` (page 1..=10⁶, page_size 1..=100 déf. 25, tri `-prefixe` par **allowlist stricte** + clé secondaire `id`, recherche `q` ILIKE échappée) + 7 tests unitaires. Réponse de page uniforme `{ page, page_size, count, total, data }`.
    - `back/src/db.rs` : `fetch_role_in_org` (rôle re-résolu en base pour les opérations multi-org), `set_audit_actor` (GUC `quarity.actor_user_id`, `is_local=true`) ; **filtre `deleted_at IS NULL`** ajouté aux requêtes de login et de refresh (une org soft-supprimée tue ses sessions).
    - `back/src/validation.rs` : validateurs d'allowlist miroirs des CHECK du schéma (parameter_code, comparator, severity, role_code, segment, org_slug) + tests.
    - `back/src/routes/mod.rs` : CORS élargi PATCH/DELETE ; sous-routeur CRUD sous `DefaultBodyLimit::max(64 Kio)`.
    - `back/Cargo.toml` : features `sqlx/chrono`, `chrono/serde`, `utoipa/chrono` (timestamps dans les DTOs).
  - **4 modules CRUD** (5 endpoints chacun — list/create/get/patch/delete), conventions identiques (gabarit : `tracked_locations`) :
    - `back/src/routes/tracked_locations.rs` : POST via **P1** `create_tracked_location_with_rules` (lieu + stations [1re = primaire] + règles, atomique ; QRT_P1 → 422) ; DELETE en cascade audité T5 ; stations figées à la création (édition fine = sous-ressource post-B6, T1 garde la base).
    - `back/src/routes/alert_rules.rs` : `tracked_location_id`/`parameter` IMMUABLES ; lieu vérifié ∈ org AVANT insert (404 sinon) ; `INSERT … SELECT FROM parameters` (le référentiel fait foi) ; TOUTES les mutations en transaction avec `set_audit_actor` (audit **T5** : create/update/activate/deactivate/delete) ; doublon exact → 409 `uq_alert_rule` ; seuil NUMERIC(12,4) — arrondi documenté.
    - `back/src/routes/users.rs` : périmètre = membres de l'org du JWT ; mutations `RequireAdmin` ; POST = user + membership en une transaction (Argon2id, email normalisé, unicité GLOBALE → 409) ; PATCH full_name/role (son propre rôle : 400) ; **DELETE = retrait de la MEMBERSHIP, jamais du compte** (pas de débordement cross-tenant ; un compte sans membership ne peut plus se connecter).
    - `back/src/routes/organizations.rs` : périmètre = orgs dont l'appelant est MEMBRE (multi-org) ; rôle re-résolu en base pour `/{id}` (claims valables uniquement pour l'org du JWT) ; ordre des refus 404-avant-403 (pas d'oracle d'existence) ; POST ouvert à tout authentifié (fonder un tenant ≠ muter le tenant courant ; le créateur devient admin, limite multi-org documentée) ; slug IMMUABLE ; **DELETE = soft-delete** (`deleted_at`, alert_events préservés — FK RESTRICT).
  - **Isolation par convention d'écriture** : toute requête SQL porte le filtre org/membership ; une ressource d'une autre org renvoie **404** (anti-énumération — indistinguable d'un id inexistant), le 403 est réservé aux refus de RÔLE.
  - **OpenAPI** : 20 paths + 4 tags ajoutés (`openapi.rs`), annotations complètes par endpoint (dont 413 des bornes de corps).
  - **Tests** : harnais partagé extrait dans `back/tests/common/mod.rs` (+ `create_test_org` : org JETABLE par test avec admin/gestionnaire/lecteur — la seed n'est JAMAIS mutée, tests parallélisables) ; `e2e.rs` adapté (helpers importés) ; **4 suites nouvelles** `b6_*.rs` = **55 tests e2e** : cycles CRUD, 409 (nom/email/slug/règle), RBAC 403 (codes stables), cross-tenant 404 (GET/PATCH/DELETE/listing), pagination/tri/q/filtres, 422 QRT_P1, PATCH vide 400, audit **T5 prouvé de bout en bout** (POST→PATCH→DELETE puis lecture `audit_log` : actions create/update/delete, acteur = admin appelant), soft-delete org → login 401, retrait de membership → compte préservé en base.
  - **Revue adversariale** (3 lentilles : isolation, sémantique HTTP/transactions, contrat OpenAPI) : aucune faille d'isolation ; retouches appliquées — 413 documenté partout, arrondi NUMERIC(12,4) documenté, test « le compte survit au retrait de membership » ajouté.
- **Fichiers touchés** :
  - `back/src/listing.rs`, `back/src/routes/{tracked_locations,alert_rules,users,organizations}.rs` (créés)
  - `back/src/{error,security,validation,db,lib,openapi}.rs`, `back/src/routes/mod.rs`, `back/Cargo.toml`, `back/Cargo.lock` (modifiés)
  - `back/tests/common/mod.rs`, `back/tests/b6_{tracked_locations,alert_rules,users,organizations}.rs` (créés), `back/tests/e2e.rs` (modifié — helpers déplacés)
  - `docs/backlog.md` (B6 ✓ + date d'en-tête), `logs/2026-06-10__agent-back__b6-crud-endpoints.md` (ce log)
- **Résultat** : OK — **149 tests verts en local contre les vraies bases** : 85 e2e (30 existants + 55 B6) + 24 db + 13 ch + 27 unitaires. `cargo check --tests`, `clippy --tests -D warnings`, `fmt --check` : verts.
- **Vérifs** :
  - Suite complète exécutée DEUX FOIS en local (stack compose : Postgres + ClickHouse + Redis réels, seed chargée) — 0 échec.
  - Incident local SANS rapport avec B6, résolu en passant : la base locale n'avait pas la migration `0005` (l'image docker du back date d'avant #30 — même cause que son healthcheck « unhealthy » : pas de `curl` dans l'image). Migration appliquée manuellement (SQL idempotent) ; un `docker compose build back` remettra l'image au niveau (#30+#32) et réenregistrera 0005 dans `_sqlx_migrations`.
- **Prochaine étape** : exécuter l'action git ci-dessous ; PR vers `dev`, CI verte, squash merge. Puis **B7** (boucle de matching Moka → `alert_events` — T7 garde la base, AJOUTER le test cross-tenant sur la boucle elle-même ; brancher Q2 ClickHouse). Reliquats B6 notés au backlog : sélecteur multi-org au login, sous-ressource stations, invitation de compte existant, rate-limit CRUD (A4/B9).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Branche cible : feature/back-b6-crud-endpoints   (depuis origin/dev — jamais master/dev en direct)
  > Script prêt : `..\commit-back-b6-crud.cmd`
  > 1) git fetch origin
  > 2) git switch -c feature/back-b6-crud-endpoints --no-track origin/dev
  > 3) git add back/Cargo.toml back/Cargo.lock back/src back/tests docs/backlog.md logs/2026-06-10__agent-back__b6-crud-endpoints.md
  > 4) git commit -m "feat(back): B6 - CRUD complet 20 endpoints (users, orgs, tracked-locations, alert-rules) + pagination/tri/filtre + 55 tests e2e"
  > 5) git push -u origin feature/back-b6-crud-endpoints
  > Puis : ouvrir une PR vers `dev`, attendre la CI verte, squash merge.
  > ─────────────────────────────────────────────
