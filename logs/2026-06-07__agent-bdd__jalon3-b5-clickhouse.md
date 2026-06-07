# Log — agent-bdd — 2026-06-07 — jalon3-b5-clickhouse

## 22:30 CEST — B5 : moyennes glissantes réglementaires ClickHouse + AQI US EPA

- **Agent / rôle** : agent-bdd
- **Jalon / tâche** : Jalon 3 — backlog **B5** (ClickHouse : `db/clickhouse/queries/rolling_regulatory.sql`, calcul **AQI** via breakpoints).
- **Contexte** : dernier item BDD du Jalon 3 (B1–B4 livrés le même jour côté Postgres). Fichier de requêtes STANDALONE (pas de mécanisme de migration ClickHouse) prouvé exécutable tel quel par les tests.
- **Actions** :
  - **Q1 `rolling_regulatory`** : bruts dédupliqués (vérité `argMax(value, ingested_at)`, JAMAIS les rollups sur-comptants cf. `01_schema.sql` §C.4) → buckets horaires (pratique EPA : moyenne des moyennes horaires) → fenêtres glissantes `RANGE BETWEEN N PRECEDING` en secondes (86399 = 24 h − 1 s pour pm25/pm10, 28799 = 8 h − 1 s pour o3, no2 = le bucket lui-même), une branche `UNION ALL` par fenêtre (borne de frame constante obligatoire). Amorçage des frames à `{from}` − 24 h, sortie [from, to[. `coverage` + `is_valid` (règle EPA ≥ 75 %).
  - **Q2 `aqi_snapshot`** : ancrage à l'heure pleine de `{at}`, fenêtre propre par polluant (24/24/8/1 h), conversion ppb pour o3/no2 (D4.2 : 25 °C / 1 atm, Vm = 24.45, M(O3) = 48.00, M(NO2) = 46.01), troncature EPA (`floor(x, 1)` pm25, `floor(x)` sinon), `greatest(., 0)`, clamp à la borne haute du barème (AQI plafonné à 500), interpolation EPA (`0001_init.sql` l.200), arrondi arithmétique `floor(x + 0.5)` (PAS `round()` : banker's rounding), `is_dominant` = max des AQI.
  - **CTE `breakpoints`** : miroir VERBATIM des 24 paliers de `db/sql/02_seed.sql` (source de vérité Postgres, barème US EPA décision n°6, pm25 EPA 2024) — toute évolution du seed à répercuter.
  - **so2/co** : EXCLUS (aucun breakpoint seedé, pas de fenêtre US-07 — décision de périmètre B5 du 2026-06-07, commentée dans le SQL). **Multi-stations** (data-model.md D.5, agrégation MAX) : hors scope mono-station, documenté en en-tête.
  - **Tests `back/tests/ch.rs`** : style db.rs/e2e.rs, `include_str!` du fichier versionné + découpe sur les séparateurs stables (zéro SQL dupliqué), env `CLICKHOUSE_*` (mêmes noms que la CI). Autonomes : chaque test insère ses mesures sur un `location_id` réservé unique par exécution (999×10⁹ + timestamp×10 + n), ancres relatives à now() (compatibles TTL 90 j).
- **Fichiers touchés** :
  - `db/clickhouse/queries/rolling_regulatory.sql` (créé)
  - `back/tests/ch.rs` (créé)
- **Résultat** : OK — exécution HTTP directe validée sur données réelles Nice #4085, calculs recoupés à la main.
- **Vérifs** : `cargo fmt --all -- --check` OK ; `cargo clippy --all-targets -- -D warnings` OK ; `cargo test --test ch -- --test-threads=1` → 7/7, deux exécutions consécutives (idempotence prouvée).
- **Prochaine étape** : revue adversariale du livrable, puis B9 (agrégat ClickHouse de `compute_exposure_dose`) ou A6a (scheduler d'ingestion).
- **Action Git suggérée à l'humain** : voir l'entrée 22:50 ci-dessous (livrable consolidé après revue).

## 22:50 CEST — Revue adversariale B5 : correctifs documentaires + test de frontière + log

- **Agent / rôle** : agent-bdd
- **Jalon / tâche** : Jalon 3 — B5, passe de correction post-revue (13 findings : 1 must_fix, 2 mineurs, 10 info).
- **Contexte** : la revue adversariale (exécutions HTTP indépendantes + recalculs manuels) n'a trouvé **aucune erreur réglementaire** ; le seul must_fix était l'absence de CE log (la mission B5 imposait « exactement deux fichiers », l'implémenteur l'avait signalé). Correctifs triviaux appliqués au passage.
- **Actions** :
  - **must_fix — log manquant** : ce fichier (`logs/2026-06-07__agent-bdd__jalon3-b5-clickhouse.md`) + script de commit préparé hors dépôt (`../commit-bdd-b5-clickhouse.cmd`, jetable) — zéro commande git exécutée, conformément à la charte.
  - **mineur — frontière exacte des frames non testée** : nouveau test `q1_range_frame_excludes_bucket_at_exact_window_boundary` (8ᵉ) : bucket à exactement h − 24 h (valeur aberrante 100.0) EXCLU du frame 86399 s et h − 23 h inclus → avg 15.0 / 2 h ; idem o3 (h − 8 h à 999.0 exclu du frame 28799 s, h − 7 h inclus → avg 40.0 / 2 h). Une régression off-by-one (86400/28800) fait désormais échouer la suite.
  - **info — sémantique `{from}` non aligné (Q1)** : documentée en en-tête de Q1 (le filtre de sortie compare le DÉBUT de bucket ; le bucket contenant `{from}` nourrit les frames sans être affiché — cohérent avec l'ancrage de Q2).
  - **mineur — « lookahead » intra-heure (Q2)** : en-tête de Q2 reformulé (TOUT le bucket [anchor_hour, anchor_hour + 1 h[ compte, y compris les mesures postérieures à `{at}` dans la même heure — pas un instantané strict).
  - **info — UInt64 en chaînes JSON** : dépendance client `output_format_json_quote_64bit_integers=0` documentée en en-tête du fichier SQL pour les futurs consommateurs directs.
  - **Non corrigés (assumés/documentés, conformes)** : résidus de test dans les rollups (TTL 2/5 ans, ids réservés, sans impact) ; Q2 renvoie zéro ligne sans donnée dans les fenêtres (à gérer côté B10) ; `is_dominant` calculé aussi sur fenêtres `is_valid=0` (le consommateur filtre via coverage/is_valid).
- **Fichiers touchés** :
  - `db/clickhouse/queries/rolling_regulatory.sql` (modifié — commentaires uniquement, zéro changement de logique)
  - `back/tests/ch.rs` (modifié — +1 test de frontière)
  - `logs/2026-06-07__agent-bdd__jalon3-b5-clickhouse.md` (créé — ce log)
  - `../commit-bdd-b5-clickhouse.cmd` (créé — hors dépôt, jetable)
- **Résultat** : OK — livrable consolidé, re-vérifié de bout en bout.
- **Vérifs** :
  - HTTP direct (curl, découpe en temp système) sur Nice #4085 : Q1 [2026-06-06 00:00, 12:00[ → bucket 11:00 pm25 `rolling_avg` 6.6333, coverage 0.5 ; Q2 at=2026-06-06 11:37 → no2 6.27 ppb → AQI 6, pm10 25.225 → AQI 23, pm25 6.6333 → AQI 37 `is_dominant=1` (identique aux recalculs manuels de la revue ; cohérence croisée Q1/Q2 sur le bucket 11:00).
  - `cargo fmt --all -- --check` OK ; `cargo clippy --all-targets -- -D warnings` OK (0 warning).
  - `cargo test --test ch -- --test-threads=1` → **8 passed; 0 failed**, deux exécutions consécutives (0.39 s puis 0.35 s).
- **Prochaine étape** : exécuter le script de commit ci-dessous, CI verte, squash merge ; puis B9 ou A6a.
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Exécuter le script `..\commit-bdd-b5-clickhouse.cmd` (depuis la racine `quarity/`), qui fait :
  > Branche cible : feature/bdd-jalon3-b5-clickhouse   (créée depuis dev à jour — jamais master/dev en direct)
  > 1) git add db/clickhouse/queries/rolling_regulatory.sql back/tests/ch.rs logs/2026-06-07__agent-bdd__jalon3-b5-clickhouse.md
  > 2) git commit -m "feat(bdd): jalon 3 - B5 ClickHouse moyennes glissantes reglementaires + AQI US EPA (+8 tests ch)"
  > 3) git push -u origin feature/bdd-jalon3-b5-clickhouse
  > 4) gh pr create --base dev
  > Puis : attendre la CI verte, squash merge, supprimer le script jetable.
  > ─────────────────────────────────────────────
  > (OBSOLÈTE — remplacé par l'entrée 23:35 ci-dessous : le script a été mis à
  > jour pour couvrir l'extension Q3/Q4, ne pas exécuter la version « +8 tests ».)

## 23:35 CEST — Extension B5 (Q3 `o3_daily_max_8h` + Q4 `no2_annual_mean`) + passe de correction post-revue

- **Agent / rôle** : agent-bdd (entrée rédigée par l'agent correcteur ; l'implémenteur de l'extension était limité à exactement deux fichiers et avait signalé le log manquant — c'était le must_fix n°1 de la revue).
- **Jalon / tâche** : Jalon 3 — **B5 étendu** au-delà du backlog (roadmap.md l.146 : « O3 max journalier de la moyenne 8 h, NO2 moyenne annuelle » — PM2.5 24 h déjà couvert par Q1), puis correction des findings de la revue adversariale de l'extension (2 must_fix, le reste mineur/info).
- **Actions — extension (implémentée à 23:07/23:09, re-documentée ici)** :
  - **Q3 `o3_daily_max_8h`** : dedup argMax → buckets horaires (pattern Q1) → frame `RANGE 28799 PRECEDING` ; fenêtre rattachée au jour UTC de son heure de DÉPART (fin − 7 h) ; ÉPILOGUE +7 h (bruts lus jusqu'à `{to}` + 7 h, symétrique de l'amorçage −24 h de Q1) ; fenêtre valide ≥ 6/8 h ; `daily_max_8h` = max des fenêtres VALIDES, FALLBACK max des présentes (garde `countIf > 0`) ; `windows_present`, `valid_windows`, `day_valid` = valid_windows ≥ 18/24.
  - **Q4 `no2_annual_mean`** : EXCEPTION documentée à la règle « jamais les rollups » (TTL 90 j des bruts ⇒ annuel impossible) : `avgMerge(avg_state)` + re-GROUP BY sur `quarity.measurements_daily` (rétention 5 ans), `uniqExact(bucket_day)` pour `days_present`, `days_in_year` 365/366 par différence des 1ers janvier (`makeDate` — `toDaysInYear` n'existe pas en 24.8, vérifié UNKNOWN_FUNCTION), `is_valid` ≥ 0.75 par cohérence de fichier (règle UE 90 % mentionnée).
  - **Tests** : +5 (13 au total), mêmes invariants (include_str! + découpe sur séparateurs, autonomes, valeurs attendues calculées à la main) ; nouveaux helpers `q3_sql()`/`q4_sql()`/`day_start()`, `q2_sql()` borné à `SEP_Q3`.
- **Actions — correctifs post-revue (cette passe)** :
  - **must_fix n°1 (charte)** : cette entrée de log + script `../commit-bdd-b5-clickhouse.cmd` MIS À JOUR (le script « +8 tests / Q1-Q2 » du workflow précédent aurait commité l'état étendu avec un message et un corps de PR inexacts ; désormais : 4 requêtes, 13 tests, corps de PR couvrant Q3/Q4).
  - **must_fix n°2 (ch.rs)** : `test_location(n)` passait de `timestamp×10 + n` à **`timestamp×100 + n`** — avec ×10 et n = 10..13, `10T + 13 = 10(T+1) + 3` : deux exécutions lancées à 1 s d'écart pouvaient partager un location_id (l'invariant d'unicité de l'en-tête était mathématiquement faux ; aucune assertion actuelle ne pouvait flaker, mais tout futur test n = 14 aurait collisionné avec `q2_no2`). En-tête et doc du helper mis à jour (n < 100).
  - **mineurs/info documentaires (SQL, zéro changement de comportement)** : Q3 — documenté qu'un jour AVEC mesures peut ne produire AUCUNE ligne en données très clairsemées (aucun bucket aux heures de FIN des fenêtres du jour : l'absence de ligne ≠ absence de donnée) ; convention des 24 fenêtres + ≥ 18/24 datée comme EPA HISTORIQUE (40 CFR 50 App. I, NAAQS 1997/2008) vs méthode en vigueur (App. U 2015 : 17 fenêtres 07:00–23:00 locales, ≥ 13/17) — seul le seuil 6/8 est commun ; alias interne `window_hours` du CTE `windowed` renommé **`hours_present`** (convention Q1/Q2 : c'est un compte d'heures présentes, pas une taille de fenêtre). Q4 — le commentaire (b) présente désormais la pondération par MESURE comme une SECONDE approximation (l'UE moyenne les valeurs HORAIRES : une heure à N mesures pèse N fois plus ici) au lieu de la survendre « plus proche de l'UE » ; note ajoutée : directive (UE) 2024/2881 → 20 µg/m³ au 1er janvier 2030 (toujours aucun seuil codé). ch.rs — doc du struct `Sample` étendue à Q1–Q4.
- **Fichiers touchés** :
  - `db/clickhouse/queries/rolling_regulatory.sql` (modifié — en-têtes de sections Q3/Q4 + renommage d'alias interne ; Q1/Q2 intacts)
  - `back/tests/ch.rs` (modifié — ×100 + commentaires)
  - `logs/2026-06-07__agent-bdd__jalon3-b5-clickhouse.md` (cette entrée)
  - `../commit-bdd-b5-clickhouse.cmd` (mis à jour — hors dépôt, jetable)
- **Résultat** : OK — extension consolidée, les deux must_fix levés, tous les findings triviaux corrigés.
- **Vérifs** (après correctifs) : HTTP direct OK (Q3 renommé exécutable tel quel) ; `cargo fmt --all -- --check` OK ; `cargo clippy --all-targets -- -D warnings` OK ; `cargo test --test ch -- --test-threads=1` → **13 passed; 0 failed**, deux exécutions consécutives.
- **Prochaine étape** : exécuter le script de commit mis à jour, CI verte, squash merge ; puis B9 ou A6a.
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Exécuter le script `..\commit-bdd-b5-clickhouse.cmd` (depuis la racine `quarity/`), qui fait :
  > Branche cible : feature/bdd-jalon3-b5-clickhouse   (créée depuis dev à jour — jamais master/dev en direct)
  > 1) git add db/clickhouse/queries/rolling_regulatory.sql back/tests/ch.rs logs/2026-06-07__agent-bdd__jalon3-b5-clickhouse.md
  > 2) git commit -m "feat(bdd): jalon 3 - B5 ClickHouse moyennes glissantes reglementaires + AQI US EPA (4 requetes, 13 tests ch)"
  > 3) git push -u origin feature/bdd-jalon3-b5-clickhouse
  > 4) gh pr create --base dev
  > Puis : attendre la CI verte, squash merge, supprimer le script jetable.
  > ─────────────────────────────────────────────
  > (OBSOLÈTE — remplacé par l'entrée 23:58 ci-dessous : le resync docs ajoute
  > 4 fichiers au commit ; le script a été mis à jour en conséquence.)

## 23:58 CEST — Resync docs (backlog / roadmap / foundations / data-model) + consolidation finale

- **Agent / rôle** : agent-bdd (consolidation orchestrateur — même recette que B1–B4).
- **Jalon / tâche** : Jalon 3 — clôture de **B5** : répercuter la livraison dans la doc projet et finaliser le commit préparé.
- **Contexte** : le livrable code (4 requêtes + 13 tests) est consolidé depuis 23:35 ; restait la trace documentaire, partie intégrante de la recette de livraison.
- **Actions** :
  - `docs/backlog.md` : **B5 coché** (détail Q1–Q4, 13 tests, 2 revues adversariales) + en-tête « Jalon 3 — axe BDD clos : B1–B5 » ; note so2/co (pas de breakpoints seedés, à seeder au besoin avec B9/B10).
  - `roadmap.md` (§Jalon 3 ClickHouse) : 4 items cochés avec annotations — MV rollup « jour × ville × polluant » : en place dès le Jalon 1, granularité **station** documentée (la « ville » n'existe pas dans le schéma CH) ; moyennes glissantes + AQI : backlog B5 ; TTL 90 j : livré dès le Jalon 1. Le benchmark ClickHouse vs Postgres reste ouvert (→ C2).
  - `docs/foundations.md` (§4 « À confirmer ») : hypothèses de conversion ppb↔µg/m³ **figées** (25 °C / 1 atm, Vm = 24.45 L/mol, M(O₃) = 48.00, M(NO₂) = 46.01) — le prérequis D4.2 « Avant le calcul AQI (Jalon 3 B5) » est soldé.
  - `docs/data-model.md` (§E) : bannière « B5 livré » ajoutée (nuances : exception Q4 rollup, mono-station/D.5, so2/co exclus).
  - `../commit-bdd-b5-clickhouse.cmd` : liste `git add` complétée des 4 fichiers docs ; corps de PR mis à jour.
- **Fichiers touchés** :
  - `docs/backlog.md`, `roadmap.md`, `docs/foundations.md`, `docs/data-model.md` (modifiés — resync)
  - `logs/2026-06-07__agent-bdd__jalon3-b5-clickhouse.md` (cette entrée)
  - `../commit-bdd-b5-clickhouse.cmd` (mis à jour — hors dépôt, jetable)
- **Résultat** : OK — mission B5 complète (code + tests + docs + log).
- **Vérifs** : modifications documentaires uniquement (la suite `cargo test --test ch` 13/13 ×2, fmt et clippy ont été validés à 23:35 ; aucune modification de code depuis).
- **Prochaine étape** : exécuter le script ci-dessous, CI verte, squash merge ; puis **A6a** (scheduler d'ingestion — la scission A6a/A6b sera actée au backlog avec la mission A6a).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Exécuter le script `..\commit-bdd-b5-clickhouse.cmd` (depuis la racine `quarity/`), qui fait :
  > Branche cible : feature/bdd-jalon3-b5-clickhouse   (créée depuis dev à jour — jamais master/dev en direct)
  > 1) git add db/clickhouse/queries/rolling_regulatory.sql back/tests/ch.rs docs/backlog.md roadmap.md docs/foundations.md docs/data-model.md logs/2026-06-07__agent-bdd__jalon3-b5-clickhouse.md
  > 2) git commit -m "feat(bdd): jalon 3 - B5 ClickHouse moyennes glissantes reglementaires + AQI US EPA (4 requetes, 13 tests ch)"
  > 3) git push -u origin feature/bdd-jalon3-b5-clickhouse
  > 4) gh pr create --base dev
  > Puis : attendre la CI verte, squash merge, supprimer le script jetable.
  > ─────────────────────────────────────────────
