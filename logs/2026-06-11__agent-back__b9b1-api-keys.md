# B9b-1 — Gestion des clés API (`/api/api-keys`)

**Date** : 2026-06-11
**Axe** : back
**Branche cible** : `feature/back-b9b1-api-keys` → `dev` (CI verte, squash merge)
**Part de** : `origin/dev` @ `0d7712a` (B9a-3, PR #44)

Première tranche de **B9b** (API publique + clés + quotas). B9b se découpe : **B9b-1 =
gestion des clés** (celle-ci, interne), **B9b-2 = API publique + auth par clé + quotas**
(externe, sensible — extracteur `ApiKey`, endpoints publics, rate-limit/quota Redis, revue
adversariale).

## Contexte schéma (déjà en place, 0001 — table 7, US-11)

`api_tokens` : `org_id`, `created_by`, `name`, `token_prefix` (8 car. en clair), `token_hash`
(UNIQUE, hash du secret — jamais en clair), `scope` (`read`/`read_write`), `last_used_at`,
`expires_at`, `revoked_at`, `created_at` ; index unique partiel sur `token_hash WHERE revoked_at
IS NULL` (lookup des clés vives, pour B9b-2). **Aucune migration** dans cette PR.

## Livré

| Endpoint | RBAC | Notes |
|---|---|---|
| `POST /api/api-keys` | `RequireAdmin` | Émet une clé : `generate_api_key()` → secret `qrt_<48 hex>` (192 bits). Le **secret n'est renvoyé qu'ICI** (réponse `CreatedApiKey`) ; seul le **hash SHA-256** est stocké. `scope` (déf. `read`), `expires_in_days` optionnel. |
| `GET /api/api-keys` | `RequireAdmin` | Liste les clés de l'org (vives + révoquées), **métadonnées seulement** (jamais secret/hash). |
| `DELETE /api/api-keys/{id}` | `RequireAdmin` | **Révoque** (`revoked_at = now()`) — immédiat ; clé inconnue/déjà révoquée/autre org ⇒ **404**. |

**Sécurité** : secret haute entropie (192 bits) ⇒ **SHA-256** déterministe (lookup O(1) via
`token_hash`, argon2 réservé aux mots de passe à faible entropie) ; `generate_api_key` /
`hash_api_key` dans `security.rs` (CSPRNG `OsRng`). **Émission/listing/révocation réservés à
`RequireAdmin`** (credentials d'org ⇒ 403 `admin_required` sinon). **Isolation** : tout est scopé
`org_id` du JWT (404 cross-tenant). `org_id`/`created_by` viennent du JWT, jamais du corps.

## Fichiers

- `back/Cargo.toml` (+ `Cargo.lock`) — dépendance `sha2 = "0.10"`.
- `back/src/security.rs` — `generate_api_key`, `hash_api_key`, helper `to_hex` (+ 1 test unit).
- `back/src/routes/api_keys.rs` (NOUVEAU) — DTOs (`ApiKeyDto`, `CreatedApiKey`), 3 handlers.
- `back/src/routes/mod.rs` (+2 routes), `back/src/openapi.rs` (+3 paths, tag `api-keys`).
- `back/tests/b9b_api_keys.rs` (NOUVEAU) — **4 tests e2e**.
- `docs/backlog.md` — B9b scindé, B9b-1 acté.

## Tests

- **e2e** : `api_key_lifecycle` (création → secret + prefix, listing sans secret/hash, révocation
  204 puis `revoked_at` renseigné, re-révocation 404) ; `created_secret_is_hashed_not_stored_plaintext`
  (vérifie **EN BASE** que `token_hash` ≠ secret, fait 64 hex, et `token_prefix` = 8 premiers car.) ;
  `api_keys_are_org_scoped` (404 cross-tenant) ; `api_keys_require_admin` (gestionnaire & lecteur → 403).
- **unit** : `security::tests::api_key_generation_and_hash` (préfixe, longueurs, hash déterministe,
  deux clés distinctes) — **vert en local** (DB-free).

## Vérifications

- `cargo fmt --all` ✅ · `cargo clippy --all-targets -- -D warnings` ✅ · `cargo build --release` ✅
  (la nouvelle dép `sha2` build dans l'image).
- 6 tests unit lib verts (dont le nouveau crypto) ; e2e adossés aux 3 bases → délégués à la CI.

## Bloc git (exécuté par Tristan — charte)

Script : `quarity/commit-back-b9b1-api-keys.cmd`
- `git fetch origin` ; `git switch -c feature/back-b9b1-api-keys --no-track origin/dev`
- `git add` (liste explicite) ; commit ASCII ; `git push -u` ; `gh pr create --base dev`
