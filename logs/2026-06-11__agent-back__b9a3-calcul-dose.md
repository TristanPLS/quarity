# B9a-3 — Calcul de dose d'exposition (ClickHouse)

**Date** : 2026-06-11
**Axe** : back
**Branche cible** : `feature/back-b9a3-calcul-dose` → `dev` (CI verte, squash merge)
**Part de** : `origin/dev` @ `37a85c1` (B9a-2, PR #43)

Dernière tranche de **B9a** (après B9a-1 CRUD profils+seuils et B9a-2 association lieu×profil).
**B9a est désormais complet.** Reste de B9 → **B9b** (API publique + clés + quotas).

## Requête ClickHouse de dose

`db/clickhouse/queries/exposure_dose.sql` (canonique, axe BDD) **et** `back/src/exposure_dose.sql`
(copie embarquée, `include_str!` — leçon B10 : build Docker du back = `back/` seul), tenues
synchrones par `tests/ch.rs::embedded_exposure_dose_sql_matches_canonical`.

Pour un `tracked_location_profile` (plage `start_time`–`end_time` **locale**, `days_mask`, `tz`) +
un seuil (`averaging_period` 1h/8h/24h) + une période `[from, to]` de dates locales :
1. **Dédup** `argMax(value, ingested_at)` (ReplacingMergeTree) → **moyenne horaire par station**.
2. **Moyenne glissante** sur `averaging_period` heures (auto-jointure `hourly h1/h2`, fenêtre
   `h2.hour ∈ (h1.hour-avg_hours, h1.hour]` — gère les trous ; borne amont de dédup élargie de
   `avg_hours` pour que la 1ʳᵉ heure ait une moyenne complète).
3. **MAX multi-stations** par heure (`per_hour: max(roll)`, précaution §D.5).
4. **Filtres** : heures bucketisées en **UTC** (`measured_at` est `DateTime64(3,'UTC')`) puis
   converties en local (`toTimeZone(hour, tz)`) ; date ∈ `[from, to]`, minute du jour
   ∈ `[start_min, end_min)`, `bitTest(days_mask, toDayOfWeek(local,1)-1)`.
5. `hours_over_threshold = countIf(conc > seuil)` (dépassement strict), `sample_count = count()`
   (heures évaluées). Agrégat sans GROUP BY → renvoie toujours une ligne (`0/0` si pas de donnée).

Tous les paramètres en `{name:Type}` server-side (anti-injection). `ch::query_exposure_dose`
(struct `DoseParams`/`DoseRow`).

## Endpoints (`/api/tracked-location-profiles/{id}/…`)

| Endpoint | RBAC | Notes |
|---|---|---|
| `POST …/{id}/compute-dose?period_start&period_end` | `CanWrite` | Pour CHAQUE seuil **1h/8h/24h** du profil (`annual` ignoré en B9a-3), calcule la dose puis fige via la procédure **`compute_exposure_dose`** (P3, snapshot seuil+fenêtre, upsert `exposure_results`) ; renvoie la liste des résultats. **404** si l'association n'est pas de l'org (via son lieu) ; **400** si `period_end < period_start`. |
| `GET …/{id}/results` | `AuthUser` | Doses en cache de l'association (404 cross-tenant). |

Isolation : l'association appartient à l'org de son **lieu** (`EXISTS(tracked_locations … org_id)`).
Erreur ClickHouse → 500 générique (aucune fuite). **Aucune migration** (schéma + procédure P3 0001/0004).

## Fichiers

- `db/clickhouse/queries/exposure_dose.sql` + `back/src/exposure_dose.sql` (NOUVEAUX, identiques).
- `back/src/ch.rs` — `EXPOSURE_DOSE_SQL`, `DoseParams`, `DoseRow`, `query_exposure_dose`.
- `back/src/routes/exposure_dose.rs` (NOUVEAU) — `compute_dose`, `list_results`, DTO `ExposureResultDto`.
- `back/src/routes/mod.rs` (+2 routes), `back/src/openapi.rs` (+2 paths).
- `back/tests/ch.rs` — test anti-dérive `embedded_exposure_dose_sql_matches_canonical`.
- `back/tests/b9_exposure_dose.rs` (NOUVEAU) — **4 tests e2e**.
- `docs/backlog.md` — B9a complet acté.

## Tests

- **e2e** : `compute_dose_counts_hours_over_threshold` (scénario DÉTERMINISTE — 5 h de données
  pm25 dans la fenêtre 08–17 tz UTC : 20/10/30/5/25 → assertit `hours_over_threshold=3`,
  `sample_count=5`, + relecture `/results`) ; `compute_dose_is_org_scoped` (404 cross-tenant sur
  compute-dose ET results) ; `compute_dose_rejects_inverted_period` (400) ;
  `compute_dose_with_only_annual_threshold_returns_empty` (annual ignoré → `[]`, ajouté suite à la revue).
- **anti-dérive** (pur) : `embedded_exposure_dose_sql_matches_canonical`.

## Revue adversariale (2 lentilles, lecture seule)

- **Math ClickHouse** : **SAIN** — les 9 points (moyenne glissante 1h/8h/24h sans off-by-one,
  MAX multi-stations, bucket UTC + conversion locale des filtres, `bitTest` jour aligné bit0=Lundi,
  fenêtre minute `[start,end)`, borne amont de dédup, dédup vs ReplacingMergeTree, anti-injection,
  cas vide→0/0) **confirmés corrects**.
- **Intégration** : tous les constats « majeur » **vérifiés corrects** — isolation org cohérente
  (compute-dose + results), signature/ordre/types de l'appel `CALL compute_exposure_dose` exacts,
  dérivation `start_min/end_min` (cast i32→u16 sûr, ≤ 1439), transmission `days_mask`, borne 400,
  masquage des erreurs ClickHouse en 500, passage des stations en `Array(UInt64)`, pas de régression.
  Seule réserve (mineure) : cas « profil annual-seul » non testé → **test ajouté**.

## Vérifications

- `cargo fmt --all` ✅ · `cargo clippy --all-targets -- -D warnings` ✅ · `cargo build --release` ✅.
- e2e adossés aux 3 bases → délégués à la CI. ⚠ `page_size` non utilisé ici (pas de listing paginé).

## Bloc git (exécuté par Tristan — charte)

Script : `quarity/commit-back-b9a3-calcul-dose.cmd`
- `git fetch origin` ; `git switch -c feature/back-b9a3-calcul-dose --no-track origin/dev`
- `git add` (liste explicite) ; commit ASCII ; `git push -u` ; `gh pr create --base dev`
