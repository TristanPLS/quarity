# Log — agent-back — 2026-06-15 — index exposure_results (Vague 0 : 5d)

## CEST — Index de lecture exposure_results (plan-finition-v1, cluster 5)

- **Agent / rôle** : agent-back
- **Jalon / tâche** : finition v1.0 — Vague 0, PR `perf/exposure-results-index` (5d, dernière de la Vague 0). **Pré-requis de 8c** (scheduler de dose).
- **Contexte** : `list_results` (`routes/exposure_dose.rs:334`) lit les doses figées d'une association, période la plus récente d'abord : `WHERE tracked_location_profile_id = $1 ORDER BY period_end DESC, parameter, averaging_period`. L'unique `uq_exposure_result (tlp_id, parameter_id, period_start, period_end)` sert le filtre par préfixe mais N'ORDONNE PAS par `period_end` → tri additionnel.
- **Action** : nouvelle migration `back/migrations/0009_exposure_results_lookup_index.sql` :
  `CREATE INDEX IF NOT EXISTS idx_exposure_results_tlp_period ON exposure_results (tracked_location_profile_id, period_end DESC);` (idempotent, aucune donnée touchée). Couvre filtre + tri (évite le sort) et prépare le scheduler de dose (8c). PG uniquement (source de vérité = migrations).
- **Fichiers touchés** :
  - `back/migrations/0009_exposure_results_lookup_index.sql` (créé)
- **Vérifs (Postgres 16 jetable, comme la CI)** :
  - Les **9 migrations s'appliquent proprement dans l'ordre** (0001→0009, `ON_ERROR_STOP=1`).
  - Index présent et conforme : `btree (tracked_location_profile_id, period_end DESC)`.
  - `EXPLAIN` (seqscan off) sur la forme de `list_results` → **utilise `idx_exposure_results_tlp_period`** (Index Scan sur le filtre `tracked_location_profile_id`).
  - Migration appliquée aussi par le job back CI (boucle `back/migrations/*.sql`).
- **➡️ Vague 0 COMPLÈTE** (1a/1b/1c, 2b/2c/2e, 3a, 4a, 5d). Suite : Vague 1 (3b, 5a, 5b/5c, 6a, puis en série 2a → 2d).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Script : quarity/commit-perf-exposure-results-index.cmd
  > Branche cible : perf/exposure-results-index (depuis origin/dev)
  > fetch + switch -c, git add 0009_*.sql + ce log, commit ASCII, push, gh pr create vers dev.
  > Puis : attendre la CI verte (job back applique la migration), squash merge, maj-dev.cmd.
  > ─────────────────────────────────────────────
