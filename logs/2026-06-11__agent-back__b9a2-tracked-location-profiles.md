# B9a-2 — Association lieu suivi × profil d'exposition

**Date** : 2026-06-11
**Axe** : back
**Branche cible** : `feature/back-b9a2-tracked-location-profiles` → `dev` (CI verte, squash merge)
**Part de** : `origin/dev` @ `41c885d` (B9a-1, PR #42)

2ᵉ tranche de **B9a**. Suite : **B9a-3 = calcul de dose** (ClickHouse → `exposure_results`).

## Contexte schéma (déjà en place, 0001 — table 21)

`tracked_location_profiles` (association n-aire **lieu × profil** + plage horaire) :
`start_time`/`end_time` (TIME, CHECK `end > start`), `days_mask` (SMALLINT, CHECK 1..=127,
bitmask bit0=Lundi … bit6=Dimanche), `timezone`, `is_active` ; UNIQUE (tracked_location_id,
exposure_profile_id) ; FK lieu CASCADE, FK profil RESTRICT. **Aucune migration** dans cette PR.

## Livré — `/api/tracked-location-profiles`

| Endpoint | RBAC | Notes |
|---|---|---|
| `GET /api/tracked-location-profiles` | `AuthUser` | Liste paginée des associations de l'org (via le LIEU porteur). Filtres `tracked_location_id`, `exposure_profile_id`, `is_active`, `q` (nom du lieu). |
| `POST /api/tracked-location-profiles` | `CanWrite` | Applique un profil **visible** (système ou org) à un lieu **de l'org**. **404** si lieu (`tracked_location_not_found`) ou profil (`exposure_profile_not_found`) non visible ; **409** doublon (lieu, profil) ; **422** plage invalide (`end<=start`) ; **400** `days_mask` hors 1..=127 ou heure mal formée. |
| `GET …/{id}` | `AuthUser` | Détail si le lieu porteur ∈ org ; **404** sinon. |
| `PATCH …/{id}` | `CanWrite` | Modifie `start_time`/`end_time`/`days_mask`/`timezone`/`is_active`. FK **immuables** ; **422** si plage résultante invalide. |
| `DELETE …/{id}` | `CanWrite` | Supprime (doses en cache `exposure_results` suivent — FK CASCADE). |

**Isolation** : un `tracked_location_profile` appartient à l'org du **lieu** qu'il porte. CHAQUE
requête est filtrée par un prédicat `EXISTS(tracked_locations … org_id = <JWT>)` — une association
d'un lieu étranger est invisible ⇒ **404** (anti-énumération ; 403 réservé au rôle). Le profil
attaché doit être **visible** (système `org_id NULL` ou de l'org) : on n'applique pas un profil
d'une autre org.

**Détails** : validateur local `validate_time_hm` (HH:MM[:SS]) — l'ordre `end>start` reste
vérifié EN BASE (CHECK → 422, backstop) ; `days_mask` validé 1..=127 au boundary (miroir du CHECK) ;
TIME exposé en `::text` (`HH:MM:SS`), bindé `$n::time` ; mappings auto 409 (23505) / 422 (23514) ;
OpenAPI réutilise le tag `exposure-profiles`.

## Fichiers

- `back/src/routes/tracked_location_profiles.rs` (NOUVEAU) — DTOs, validateur, 5 handlers.
- `back/src/routes/mod.rs` — 2 routes (+ déclaration du module).
- `back/src/openapi.rs` — 5 paths (tag `exposure-profiles` réutilisé).
- `back/tests/b9_tracked_location_profiles.rs` (NOUVEAU) — **5 tests e2e**.
- `docs/backlog.md` — B9a-2 acté.

## Tests (e2e, délégués à la CI)

`tlp_crud_lifecycle` (création + détail + PATCH plage/état + listing filtré + suppression),
`tlp_is_invisible_cross_tenant` (404 GET/PATCH/DELETE + absent du listing d'org B),
`tlp_rejects_foreign_location` (404 sur un lieu d'une autre org),
`tlp_duplicate_location_profile_conflicts` (409), `tlp_invalid_window_is_rejected` (422 `end<=start`
+ 400 `days_mask=200`). Setup : station synthétique (`ref_locations`) + lieu via l'API + profil custom.

## Vérifications

- `cargo fmt --all` ✅ · `cargo clippy --all-targets -- -D warnings` ✅ · `cargo build --release` ✅.
- ⚠ Leçon B9a-1 appliquée : `page_size` borné à **1..=100** (`ListParams`) — les tests respectent.
- e2e adossés aux 3 bases → délégués à la CI.

## Bloc git (exécuté par Tristan — charte)

Script : `quarity/commit-back-b9a2-tracked-location-profiles.cmd`
- `git fetch origin` ; `git switch -c feature/back-b9a2-tracked-location-profiles --no-track origin/dev`
- `git add` (liste explicite) ; commit ASCII ; `git push -u` ; `gh pr create --base dev`
