# Log — agent-back — 2026-06-15 — fix dose : bitTest CH 24.8 + bug bit lundi + pin image

## CEST — Bloquant n°0 : compute-dose 500 sur ClickHouse 24.8.x

- **Agent / rôle** : agent-back
- **Jalon / tâche** : finition v1.0 — **bloquant n°0** (gèle la Vague 0). La CI de la 1re PR de finition (`chore/front-cosmetic-deadcode`, front pur) est tombée rouge sur `b9_exposure_dose` : `compute-dose` renvoie **500 `internal_error`**.
- **Contexte / diagnostic** :
  - **Pas la PR front** : le job rouge est `back`, test d'intégration `compute-dose`.
  - **Pas flaky** : la CI tourne `--test-threads=1` (sériel). Sur 6 tests dose, les **4 qui passent n'exécutent jamais la vraie requête** (404/400/seuil annual ignoré) ; les **2 seuls qui l'exécutent avec données échouent** (vite, <1 s) → **déterministe**. Rerun CI = `failure` identique (2e point de mesure).
  - **Vraie erreur (invisible en CI)** : `error_for_status()` (ch.rs:349) jette le corps de la réponse CH et `tracing` n'est pas initialisé en test. Reproduit en local contre `clickhouse/clickhouse-server:24.8.14.39-alpine` → `Code: 12 DB::Exception: The bit position argument needs to be a positive value and less or equal to 15 ... bitTest(127, minus(toDayOfWeek(...,1), 1)) (PARAMETER_OUT_OF_BOUND)`.
  - **Deux causes superposées** :
    1. **Garde CH 24.8.x** : `bitTest` (et `bitShiftLeft`) **rejettent désormais une position non-constante de type signé** (le `- 1` produit un `Int16`). Probes : position constante OK, position `materialize(...)` → rejet ; `toUInt8(...)` ne suffit pas (range 0..255 non prouvé ≤ 15).
    2. **Bug latent de correctness** : `toDayOfWeek(t, 1)` est **déjà 0-based (Lun=0)** — vérifié sur dates connues. Donc `toDayOfWeek(...,1) - 1` donnait **−1 le lundi** et décalait *tous* les jours d'un bit. Masqué jusqu'ici car **tous les tests utilisent `days_mask=127`** (tous les jours), et la date de test `now()-7j` tombait sur un samedi/dimanche le 06-13 (CI verte) mais sur un **lundi le 06-15** (CI rouge).
- **Correctif** (chirurgical, SQL uniquement) — `exposure_dose.sql` ligne 61, dans `back/src/` ET le canonique `db/clickhouse/queries/` (identiques, test anti-dérive `embedded_exposure_dose_sql_matches_canonical`) :
  - Avant : `AND bitTest({days_mask:UInt16}, toDayOfWeek(toTimeZone(hour, {tz}), 1) - 1) = 1`
  - Après : `AND bitAnd({days_mask:UInt16}, bitShiftLeft(toUInt16(1), toDayOfWeek(toTimeZone(hour, {tz}), 1))) != 0`
  - → suppression du `- 1` (mode 1 est 0-based) ; forme `bitAnd`/`bitShiftLeft` avec décalage `UInt8` non signé (passe le garde). Commentaire explicatif ajouté pour empêcher un retour à `bitTest`.
- **Épinglage image CH** (décision Tristan : CI + compose) : `24.8-alpine` (roulant) → `24.8.14.39-alpine` (patch exact, existe sur Docker Hub) dans `.github/workflows/ci.yml` ET `docker-compose.yml`. Reproductibilité CI = prod.
- **Fichiers touchés** :
  - `back/src/exposure_dose.sql` (modifié — requête)
  - `db/clickhouse/queries/exposure_dose.sql` (modifié — canonique, identique)
  - `.github/workflows/ci.yml` (modifié — pin image CH)
  - `docker-compose.yml` (modifié — pin image CH)
- **Vérifs (repro locale contre CH 24.8.14.39 exacte)** :
  - Requête corrigée sans erreur. Sémantique validée multi-masques : `127`→{3,5}, `1` (lundi seul, bit0)→{3,5}, `126` (lundi exclu)→{0,0}, `2` (mardi seul)→{0,0}. Variante `24h`→{4,5} sans erreur.
  - Les deux fichiers SQL restent byte-identiques (`diff`=0 → anti-dérive OK).
  - Aucun code Rust touché → fmt/clippy/compilation inchangés.
  - Gate final = CI de la PR (job back doit repasser au vert : `compute_dose_counts_hours_over_threshold` + `compute_dose_distinguishes_averaging_periods_for_same_parameter`).
- **Reliquats / suivi recommandés (NON inclus, à part)** :
  - **Observabilité** : `query_exposure_dose` (et les autres appels CH) jettent le corps d'erreur CH via `error_for_status()` — on a été aveugles 1 h. Capter le corps dans `ClickhouseError` = petite PR de suivi à forte valeur.
  - **Pin PG/Redis** : `postgres:16-alpine` et `redis:7-alpine` sont aussi des tags roulants — épinglage optionnel pour reproductibilité totale (non décidé, hors scope ici).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Script : quarity/commit-fix-dose-bittest-ch248.cmd
  > Branche cible : fix/back-dose-bittest-ch248 (depuis origin/dev)
  > ORDRE : ce fix passe AVANT la PR front (bloquant n°0). Il n'ajoute que les fichiers back/ci/compose + ce log ; les changements front de PR1 restent dans le working tree (disjoints) et survivent à maj-dev.
  > fetch + switch -c, git add des 4 fichiers + ce log, commit ASCII, push, gh pr create vers dev.
  > Puis : attendre la CI VERTE (job back inclus), squash merge, maj-dev.cmd. ENSUITE seulement : PR1 (commit-chore-front-cosmetic-deadcode.cmd).
  > ─────────────────────────────────────────────
