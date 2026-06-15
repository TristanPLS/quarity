# Log — agent-back — 2026-06-15 — rate_limit_hit atomique (Vague 1 : 3b)

## CEST — INCR+EXPIRE atomique + auto-guérison (plan-finition-v1, cluster 3)

- **Agent / rôle** : agent-back
- **Jalon / tâche** : finition v1.0 — Vague 1, PR `fix/rate-limit-atomic-eval` (3b).
- **Contexte** : `rate_limit_hit` (`redis_store.rs`) faisait `INCR` puis, si `count==1`, `EXPIRE` en DEUX allers-retours. Un crash entre les deux laissait un compteur **orphelin SANS TTL** → lockout permanent de la clé (le compteur ne retombe jamais sous la limite).
- **Action** : remplacement par un `redis::Script` Lua (exécution atomique côté Redis) **auto-guérison** :
  ```lua
  local n = redis.call('INCR', KEYS[1])
  if redis.call('TTL', KEYS[1]) < 0 then redis.call('EXPIRE', KEYS[1], ARGV[1]) end
  return n
  ```
  Ré-arme l'expiration dès que la clé n'a pas de TTL (`TTL < 0` : -1 = pas d'expiration) → une clé jadis orpheline se répare à la frappe suivante. **Signature `pub async fn rate_limit_hit(conn, key, window_secs) -> RedisResult<u64>` PRÉSERVÉE** (5 appelants inchangés : `auth.rs` ×3, `security.rs` ×2). `AsyncCommands` reste requis (`conn.del` l.74).
- **Fichiers touchés** :
  - `back/src/redis_store.rs` (modifié)
- **Vérifs** :
  - `cargo fmt --all -- --check` → OK · `cargo clippy --all-targets -- -D warnings` → OK.
  - **Comportement validé contre un Redis 7 jetable** : hit 1 (neuf) → 1, TTL=60 ; hit 2 → 2, TTL=59 (**fenêtre fixe non reset**) ; orphelin (`PERSIST` → TTL=-1) → hit 3 → 3, **TTL ré-armé à 60** (auto-guérison).
  - Runtime rate-limit (30→429) couvert par `b9b_public_api.rs` (CI, vraies bases).
- **Prochaine étape Vague 1** : `perf/matching-cache-metrics` (5a) · `infra/compose-mem-limits` (5b/5c) · `test/ingest-parsing-openaq` (6a) · puis série `refactor/front-dedup-state-ui` (2a) → `refactor/front-selectfield` (2d).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Script : quarity/commit-fix-rate-limit-atomic-eval.cmd
  > Branche cible : fix/rate-limit-atomic-eval (depuis origin/dev)
  > fetch + switch -c, git add redis_store.rs + ce log, commit ASCII, push, gh pr create vers dev.
  > Puis : attendre la CI verte, squash merge, maj-dev.cmd.
  > ─────────────────────────────────────────────
