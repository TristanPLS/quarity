# Log — agent-back — 2026-06-15 — métriques cache de matching (Vague 1 : 5a)

## CEST — Observabilité du cache Moka de règles (plan-finition-v1, cluster 5)

- **Agent / rôle** : agent-back
- **Jalon / tâche** : finition v1.0 — Vague 1, PR `perf/matching-cache-metrics` (5a).
- **Contexte** : le cache L1 Moka des règles (`RuleCache`, matching.rs) n'avait aucune observabilité — impossible de savoir si le TTL/rechargement se comporte bien en prod.
- **Actions** :
  - `RuleCache` : 3 `Arc<AtomicU64>` (`accesses`/`misses`/`evictions`) + `struct CacheStats` + `pub fn stats() -> CacheStats`.
    - `accesses++` avant le `try_get_with` (tout appel à `index()`),
    - `misses++` **dans** le closure de chargement (ne tourne qu'au miss/TTL expiré),
    - `evictions++` via `.async_eviction_listener` (clone d'`evictions`, `fetch_add` sur chaque `RemovalCause` — TTL expiré).
  - `run_loop` : `tracing::debug!` des stats après le chargement de l'index (gated debug). **Aucun endpoint** (politique 5a).
  - Signatures `new(pg, ttl)` et `index()` **inchangées** → aucun appelant touché.
- **Fichiers touchés** :
  - `back/src/matching.rs` (modifié — 3 hunks : import atomics, RuleCache, run_loop)
- **Vérifs** :
  - `cargo fmt --all -- --check` → OK · `cargo clippy --all-targets -- -D warnings` → OK (valide la signature exacte `async_eviction_listener` + `Box::pin(async {})`).
  - Changement **purement additif** (compteurs) : comportement du cache inchangé → tests b7 (matching, vraies bases) confirment en CI.
  - Runtime : `RUST_LOG=quarity=debug` montre la ligne « matching : stats cache de regles » à chaque tick.
- **Prochaine étape Vague 1** : `infra/compose-mem-limits` (5b/5c) · puis série `refactor/front-dedup-state-ui` (2a) → `refactor/front-selectfield` (2d).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Script : quarity/commit-perf-matching-cache-metrics.cmd
  > Branche cible : perf/matching-cache-metrics (depuis origin/dev)
  > fetch + switch -c, git add matching.rs + ce log, commit ASCII, push, gh pr create vers dev.
  > Puis : attendre la CI verte, squash merge, maj-dev.cmd.
  > ─────────────────────────────────────────────
