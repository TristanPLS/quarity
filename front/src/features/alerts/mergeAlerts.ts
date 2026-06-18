import type { AlertEvent } from '../../api/types'

/**
 * Fusionne deux flux d'alertes — le backfill HTTP (`GET /api/alert-events`, 4b) et le
 * push WebSocket B8 — en une liste unique, **dédupliquée par `id`**, triée par
 * `fired_at` décroissant (plus récentes en tête), bornée à `max`.
 *
 * Pur et déterministe (donc testable, hors React) : `incoming` peut recouper
 * `existing` (une alerte poussée par la WS juste avant que le backfill ne réponde) —
 * l'`id` tranche, l'ordre d'arrivée n'influe pas sur le résultat. À id égal, `incoming`
 * remplace `existing` (la donnée la plus fraîche gagne). `fired_at` est un ISO-8601
 * UTC : la comparaison lexicographique suffit ; à `fired_at` égal, l'`id` (BIGINT
 * croissant) départage de façon stable.
 */
export function mergeAlerts(
  existing: AlertEvent[],
  incoming: AlertEvent[],
  max: number,
): AlertEvent[] {
  const byId = new Map<number, AlertEvent>()
  for (const a of existing) byId.set(a.id, a)
  for (const a of incoming) byId.set(a.id, a)
  const merged = [...byId.values()].sort((a, b) => {
    if (a.fired_at !== b.fired_at) return a.fired_at < b.fired_at ? 1 : -1
    return b.id - a.id
  })
  return merged.slice(0, Math.max(0, max))
}
