# B9a-1 — CRUD profils d'exposition + seuils adaptés

**Date** : 2026-06-11
**Axe** : back
**Branche cible** : `feature/back-b9a-exposure-profiles` → `dev` (CI verte, squash merge)
**Part de** : `origin/dev` @ `74f8d97` (quick wins, PR #41)

Première tranche de **B9a** (profils d'exposition + dose). B9 est découpé : **B9a**
(exposition, interne) et **B9b** (API publique + clés + quotas). B9a lui-même se livre
en 3 PR : **B9a-1 = CRUD profils + seuils** (celle-ci), **B9a-2 = association lieu×profil**
(`tracked_location_profiles`), **B9a-3 = calcul de dose** (ClickHouse → `exposure_results`).

## Contexte schéma (déjà en place, 0001)

`exposure_profiles` (système `org_id NULL`/`is_system` partagés + custom par org ; code ∈
{enfants, asthmatiques, personnes_agees, sportifs, general} ; UNIQUE (org_id, code) NULLS NOT
DISTINCT), `exposure_thresholds` (seuil par profil × polluant × période ; UNIQUE ; unité dérivée
de `parameters`). Profils système + seuils **seedés**. **Aucune migration** dans cette PR.

## Livré — `/api/exposure-profiles` (+ seuils nichés)

| Endpoint | RBAC | Notes |
|---|---|---|
| `GET /api/exposure-profiles` | `AuthUser` | Liste paginée : système (`org_id NULL`) + org du JWT. Filtres `scope` (system/custom), `code`, `q`. Tri allowlist. |
| `POST /api/exposure-profiles` | `CanWrite` | Crée un profil **custom** (org du JWT, `is_system=false`). **409** si (org, code) existe. |
| `GET /api/exposure-profiles/{id}` | `AuthUser` | Détail si VISIBLE (système ou org) ; **404** sinon. |
| `PATCH /api/exposure-profiles/{id}` | `CanWrite` | Modifie name/description d'un profil **propre** (système ⇒ **404**, lecture seule) ; `code` immuable. |
| `DELETE /api/exposure-profiles/{id}` | `CanWrite` | Supprime un profil propre (seuils en cascade ; profil encore appliqué à un lieu ⇒ **422** via FK RESTRICT). |
| `GET …/{id}/thresholds` | `AuthUser` | Seuils d'un profil visible (non paginé — peu de lignes). |
| `POST …/{id}/thresholds` | `CanWrite` | Ajoute un seuil à un profil **propre** ; **400** polluant hors référentiel (`INSERT … SELECT parameters`) ; **409** doublon (profil, polluant, période). |
| `DELETE …/{id}/thresholds/{threshold_id}` | `CanWrite` | Supprime un seuil d'un profil propre. |

**Isolation** : lecture = système OU org ; **toute mutation scopée `org_id = <JWT>`** → un profil
système ou d'une autre org est invisible à l'écriture ⇒ **404** (anti-énumération ; 403 réservé au
rôle). `org_id` vient TOUJOURS du JWT. Pas d'audit T5 (aucun trigger d'audit sur l'exposition).

**Détails** : validateurs locaux `validate_exposure_code` / `validate_averaging_period` / `validate_scope`
(surface partagée non élargie) ; unité du seuil **dérivée** de `parameters.unit` (jamais dupliquée) ;
mappings auto 409 (23505) / 422 (23503) ; OpenAPI complet (nouveau tag `exposure-profiles`).

## Fichiers

- `back/src/routes/exposure_profiles.rs` (NOUVEAU) — DTOs, validateurs, 8 handlers.
- `back/src/routes/mod.rs` — 4 routes (+ import `delete`).
- `back/src/openapi.rs` — 8 paths + tag `exposure-profiles`.
- `back/tests/b9_exposure_profiles.rs` (NOUVEAU) — **6 tests e2e**.
- `docs/backlog.md` — B9 scindé B9a/B9b, B9a-1 acté.

## Tests (e2e, délégués à la CI)

`profile_crud_lifecycle_with_thresholds` (cycle complet + 409 doublon + effacement description),
`custom_profile_is_invisible_cross_tenant` (404 GET/PATCH/DELETE/POST-seuil + absent du listing),
`system_profiles_are_visible_but_read_only` (visibles en `scope=system`, 404 en mutation),
`reader_role_cannot_write` (403), `duplicate_code_for_same_org_conflicts` (409),
`threshold_with_unknown_parameter_is_rejected` (400).

## Vérifications

- `cargo fmt --all` ✅ · `cargo clippy --all-targets -- -D warnings` ✅ ·
  `cargo build --release` ✅ (mirror Docker CI).
- e2e adossés aux 3 bases → **délégués à la CI** (un `cargo test` local pollue le Postgres partagé).

## Bloc git (exécuté par Tristan — charte)

Script : `quarity/commit-back-b9a-exposure-profiles.cmd`
- `git fetch origin` ; `git switch -c feature/back-b9a-exposure-profiles --no-track origin/dev`
- `git add` (liste explicite) ; commit ASCII ; `git push -u` ; `gh pr create --base dev`
