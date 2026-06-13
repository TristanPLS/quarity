# Log — agent-back — 2026-06-13 — dose-bound-parallel

## 15:40 CEST — compute-dose : borne de période (A3) + requêtes ClickHouse parallélisées (P3)

- **Agent / rôle** : agent-back
- **Jalon / tâche** : Jalon 4 — constats A3 (borne période) + P3 (parallélisation) des audits.
- **Contexte** : (A3) `compute-dose` ne validait que `period_end >= period_start`, pas la durée — défense en profondeur manquante (le TTL 90 j de ClickHouse borne déjà le scan réel, mais aucune garde applicative). (P3) les requêtes ClickHouse de dose (une par seuil 1h/8h/24h) tournaient en SÉRIE (`for … await`), soit ~×3 sur un profil riche — le pattern `buffer_unordered` borné existait déjà dans `aqi.rs`.
- **Actions** :
  - `routes/exposure_dose.rs` (A3) : les dates sont parsées (`NaiveDate`) au lieu d'une comparaison lexicographique ; refus **400** si `period_end - period_start > 366 jours` (const `DOSE_MAX_PERIOD_DAYS`).
  - `routes/exposure_dose.rs` (P3) : les requêtes ClickHouse de dose tournent désormais en **parallèle bornée** (`futures_util::stream::iter(...).buffer_unordered(DOSE_MAX_CONCURRENT_CH_QUERIES=8)`, calqué sur `aqi.rs`) ; chaque résultat est rattaché à son seuil par index ; les upserts PG (`CALL compute_exposure_dose`) restent séquentiels (écriture rapide, ordre indifférent car chaque dose est désormais discriminée par `averaging_period` — cf. fix #1/0007).
  - `back/tests/b9_exposure_dose.rs` : +1 test `compute_dose_rejects_overlong_period` (plage 2025-01-01→2026-12-31 = 729 j → 400).
- **Fichiers touchés** :
  - `back/src/routes/exposure_dose.rs` (modifié)
  - `back/tests/b9_exposure_dose.rs` (modifié)
- **Résultat** : OK.
- **Vérifs** : `cargo fmt` OK · `cargo clippy --all-targets -- -D warnings` OK (la parallélisation compile sans souci de borrow/lifetime) · `cargo build --release` OK (miroir Docker CI, 2m01). Tests e2e (dont le nouveau test A3) délégués à la CI.
- **Prochaine étape** : fin de la traîne « bugs ». Restent (hors « bug ») : A6 ESLint (PR dédiée) + volet **process** : resync `backlog.md`/`roadmap.md`, README final (étape `JWT_SECRET`, comptes démo), tag `v1.0`.
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Script : quarity/commit-fix-dose-bound-parallel.cmd
  > Branche cible : fix/back-dose-bound-parallel (depuis origin/dev)
  > fetch + switch -c, git add exposure_dose.rs + b9_exposure_dose.rs + ce log, commit ASCII, push, gh pr create vers dev.
  > Puis : attendre la CI verte, squash merge.
  > ─────────────────────────────────────────────
