# Log — agent-bdd — 2026-06-13 — aqi-integrity-test

## 13:10 CEST — Test d'intégrité croisé des paliers AQI Postgres ↔ ClickHouse (A4 / P1-3)

- **Agent / rôle** : agent-bdd
- **Jalon / tâche** : Jalon 3/4 — constat A4 (audit 06-12) = P1-3 (audit 06-10), resté OUVERT.
- **Contexte** : les 24 paliers AQI EPA (6 catégories × 4 polluants) sont dupliqués **verbatim** entre le seed Postgres (`aqi_breakpoints`, déclaré « source de vérité » dans data-model.md) et le CTE `breakpoints` du SQL ClickHouse (`rolling_regulatory.sql` Q2). Or **c'est le CTE ClickHouse qui est réellement exécuté** à l'exécution — le back ne lit JAMAIS la table PG (`grep aqi_breakpoints back/src` = 0). Le seul test anti-dérive existant ne comparait que CH↔CH (copie embarquée `aqi_snapshot.sql` vs canonique). **Aucun test ne croisait PG↔CH** → une modification d'un seul côté aurait faussé l'AQI **en silence**.
- **Actions** :
  - `back/tests/ch.rs` : nouveau test **pur** `aqi_breakpoints_postgres_matches_clickhouse_cte` + helper `breakpoint_key`. Extrait les tuples `(parameter, conc_low, conc_high, aqi_low, aqi_high)` du seed PG (4 blocs `INSERT INTO aqi_breakpoints`, parsing par ligne) ET du CTE `breakpoints` CH (entre `breakpoints AS` et `win AS`), normalise (conc sur 3 décimales, aqi entiers) et compare les 24 lignes triées. Échouera désormais à toute dérive d'un seul côté.
  - `docs/data-model.md` (§D.1) : clarifié que la table PG `aqi_breakpoints` est la **référence métier** mais que le **calcul runtime lit le miroir embarqué côté ClickHouse** — les deux étant tenus identiques par ce test. (Lève l'ambiguïté « source de vérité = Postgres » qui ne correspondait pas au chemin de calcul réel.)
- **Fichiers touchés** :
  - `back/tests/ch.rs` (modifié)
  - `docs/data-model.md` (modifié)
- **Résultat** : OK.
- **Vérifs** : `cargo fmt` OK · `cargo clippy --all-targets -- -D warnings` OK · `cargo test --test ch aqi_breakpoints_postgres_matches_clickhouse_cte -- --exact` → **1 passed** (test pur, sans base ; les 24 paliers concordent aujourd'hui).
- **Prochaine étape** : #5 trigger T8 (isolation lieu×profil, migration 0008), #6 borne+parallélisation dose (A3/P3), puis ESLint + process (resync docs, README final, tag v1.0).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Script : quarity/commit-fix-aqi-integrity.cmd
  > Branche cible : fix/bdd-aqi-integrity (depuis origin/dev)
  > fetch + switch -c, git add ch.rs + data-model.md + ce log, commit ASCII, push, gh pr create vers dev.
  > Puis : attendre la CI verte, squash merge.
  > ─────────────────────────────────────────────
