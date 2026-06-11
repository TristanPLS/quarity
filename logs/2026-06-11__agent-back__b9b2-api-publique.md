# B9b-2 — API publique authentifiée par clé API (clôt B9)

**Date** : 2026-06-11
**Axe** : back
**Branche cible** : `feature/back-b9b2-api-publique` → `dev` (CI verte, squash merge)
**Part de** : `origin/dev` @ `420e0ae` (B9b-1, PR #45)

Dernière tranche de **B9b** — **B9 est désormais complet**.

## Livré

### Extracteur `ApiKeyAuth` (`security.rs`)
Auth d'une requête publique par clé API en en-tête **`X-API-Key: qrt_…`** (distinct du Bearer JWT) :
1. hash **SHA-256** de la clé présentée → lookup `api_tokens WHERE token_hash = $1 AND revoked_at
   IS NULL` (sert l'index partiel unique) ; **401** si introuvable (`invalid_api_key`), vide/absente
   (`missing_api_key`), ou expirée (`api_key_expired`, `expires_at < now`).
2. **rate-limit/min** (`apikey:rl:{token_id}`, fenêtre 60 s) **puis quota mensuel**
   (`apikey:quota:{token_id}:{YYYYMM}`, TTL ~35 j) via `redis_store::rate_limit_hit` (fenêtre fixe),
   comparés aux limites du **plan de l'abonnement ACTIF** de l'org (`organization_subscriptions` ×
   `subscription_plans`) — **défaut free (1000/mois, 30/min)** si aucun abo ; **`0` = illimité** ;
   dépassement → **429**.
3. `last_used_at` mis à jour best-effort.

`org_id` vient **exclusivement de la clé** (jamais d'un input client). Rate-limit AVANT quota : une
requête rate-limitée ne consomme pas le quota mensuel.

### Endpoints publics (`public_api.rs`)
- `GET /api/public/aqi` → AQI courant des lieux de l'org de la clé.
- `GET /api/public/measurements?location_id&parameter&from&to&page&page_size` → mesures (403 si la
  station n'est pas suivie par l'org).

Read-only, scopés `auth.org_id`. **Réutilisent la logique JWT** via des fonctions partagées
extraites — un seul endroit de vérité : `aqi::overview_for_org(state, org_id)` et
`measurements::measurements_page_for_org(state, org_id, q)`. OpenAPI : schéma de sécurité **`api_key`**
(X-API-Key) + tag `public-api`. **Aucune migration** (`api_tokens`, plans, abos déjà en place).

## Fichiers

- `back/src/security.rs` — extracteur `ApiKeyAuth`.
- `back/src/routes/aqi.rs` + `measurements.rs` — extraction des fns partagées (handlers JWT inchangés).
- `back/src/routes/public_api.rs` (NOUVEAU) — 2 handlers publics.
- `back/src/routes/mod.rs` (+2 routes), `back/src/openapi.rs` (+2 paths, scheme `api_key`, tag).
- `back/tests/b9b_public_api.rs` (NOUVEAU) — **4 tests e2e**.
- `docs/backlog.md` — B9 complet acté.

## Tests (e2e, délégués à la CI)

`public_aqi_with_valid_key` (200) ; `public_api_rejects_missing_invalid_or_revoked_key` (401 sur clé
absente / bidon / **révoquée**) ; `public_measurements_is_org_scoped` (403 station étrangère) ;
`public_api_enforces_rate_limit` (30 req OK, **31ᵉ → 429** au plan free par défaut).

## Revue adversariale (2 lentilles, lecture seule)

- **Auth & isolation : SAIN** — révocation/expiration appliquées (401), **pas de timing-attack**
  (lookup par hash indexé, aucune comparaison de secret en clair), `org_id` immuable (jamais d'un
  input client), anti-injection (tout bindé), **secret jamais loggé/sérialisé**, read-only par
  construction, index partiel `WHERE revoked_at IS NULL` exemplaire.
- **Quota & régression : sain-avec-réserves** — keying par clé, `0`=illimité, défaut free correct,
  rate-limit-avant-quota, fenêtre mensuelle cohérente, **zéro régression** (`/api/aqi` &
  `/api/measurements` inchangés, isolation préservée), OpenAPI propre.

**Constat traité** :
- *(majeur, confirmé)* les requêtes **4xx comptent dans le quota** (l'extracteur incrémente avant le
  handler). Déplacer le contrôle d'ownership dans l'extracteur **ne convient pas** (extracteur
  générique ; `/aqi` n'a pas de `location_id`). Choix : **politique conservatrice assumée et
  DOCUMENTÉE** (toute requête authentifiée compte — anti-abus : un attaquant ne sonde pas sans
  consommer son quota). **Pas de changement de code** hormis un commentaire explicatif (`security.rs`).
- *(mineur, écarté)* « garde de scope `read_write` » : inutile — les endpoints read acceptent
  légitimement `read` ET `read_write` (read_write ⊇ read) ; `read_write` reste un point d'extension
  non émis. Ajouter un rejet serait une régression.

## Vérifications

- `cargo fmt --all` ✅ · `cargo clippy --all-targets -- -D warnings` ✅ · `cargo build --release` ✅.
- e2e adossés aux 3 bases → délégués à la CI.

## Bloc git (exécuté par Tristan — charte)

Script : `quarity/commit-back-b9b2-api-publique.cmd`
- `git fetch origin` ; `git switch -c feature/back-b9b2-api-publique --no-track origin/dev`
- `git add` (liste explicite) ; commit ASCII ; `git push -u` ; `gh pr create --base dev`
