# Log — agent-back — 2026-06-15 — benchmarks C2 (Vague 3 : 7b)

## CEST — Micro-bench matching criterion + protocole p95 (plan-finition-v1, cluster 7)

- **Agent / rôle** : agent-back
- **Jalon / tâche** : finition v1.0 — Vague 3, PR `perf/c2-benchmarks-matching` (7b = backlog C2).
- **Contexte** : C2 exige des benchmarks perf avec hypothèses de charge — matching < 5 ms/mesure (US-03) et timeseries p95 < 200 ms (US-06).
- **Actions** :
  - `back/src/matching.rs` : `#[doc(hidden)] pub fn RuleIndex::from_station_rules(Vec<StationRule>)` — constructeur exposé pour le bench (crate externe ⇒ **`pub`**, pas `pub(crate)` comme suggérait le plan ; le chemin prod reste `from_rows` privé).
  - `back/Cargo.toml` : `criterion` en `[dev-dependencies]` (`features=["html_reports"]`) + `[[bench]] name="matching" harness=false`. `Cargo.lock` mis à jour.
  - `back/benches/matching.rs` : bench `lookup()` + `evaluate()` (500 mesures) sur index 1k/5k/10k règles (1000 stations × 6 polluants).
  - `docs/benchmarks.md` : résultats criterion réels + interprétation + **protocole de charge p95** (`oha`/`k6` contre `GET /api/measurements`, hypothèses 10/50/100 VUs × 60 s, p50/p95/p99) à exécuter sur l'env démo (7d).
  - `docs/backlog.md` : C2 coché.
- **Fichiers touchés** :
  - `back/src/matching.rs` · `back/Cargo.toml` · `back/Cargo.lock` (modifiés)
  - `back/benches/matching.rs` · `docs/benchmarks.md` (créés)
  - `docs/backlog.md` (C2 → [x])
- **Résultats (criterion, médiane, poste de dev, `--release`)** :
  - `lookup` : 37.6 / 40.6 / 37.5 ns (1k/5k/10k) → **O(1)** (plat).
  - `evaluate` 500 mesures : 157.8 / 202.5 / 294.6 µs → **par mesure ≈ 0.32 / 0.40 / 0.59 µs**.
  - ⇒ **~0.3-0.6 µs/mesure** même à 10k règles = **marge ~10 000× sous le budget US-03 (< 5 ms)**. Objectif tenu.
- **Vérifs** :
  - `cargo fmt --all -- --check` → OK · `cargo clippy --all-targets -- -D warnings` → OK (**le bench est compilé par clippy → clippy-clean**, exigence du gate CI ; le bench lui-même n'est PAS un gate de perf).
  - `cargo bench --bench matching` → exécuté, chiffres ci-dessus.
- **Prochaine étape Vague 3** : `chore/ci-supply-chain-audit` (7c) · 5e capture Swagger → puis release 7d.
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Script : quarity/commit-perf-c2-benchmarks-matching.cmd
  > Branche cible : perf/c2-benchmarks-matching (depuis origin/dev)
  > fetch + switch -c, git add matching.rs + Cargo.toml + Cargo.lock + benches/matching.rs + docs/benchmarks.md + docs/backlog.md + ce log, commit ASCII, push, gh pr create vers dev.
  > Puis : attendre la CI verte (clippy --all-targets compile le bench), squash merge, maj-dev.cmd.
  > ─────────────────────────────────────────────
