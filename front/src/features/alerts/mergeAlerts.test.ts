import { describe, it, expect } from 'vitest'
import type { AlertEvent } from '../../api/types'
import { mergeAlerts } from './mergeAlerts'

/** Fabrique un AlertEvent minimal — seuls `id` et `fired_at` comptent pour le tri/dédup. */
function ev(id: number, fired_at: string, over: Partial<AlertEvent> = {}): AlertEvent {
  return {
    id,
    alert_rule_id: null,
    org_id: 1,
    tracked_location_id: null,
    openaq_location_id: 1001,
    openaq_sensor_id: null,
    parameter_code: 'pm25',
    measured_value: 50,
    unit: 'µg/m³',
    measured_at: fired_at,
    threshold_value: 15,
    comparator: '>',
    severity: 'warning',
    fired_at,
    ...over,
  }
}

describe('mergeAlerts', () => {
  it('trie par fired_at décroissant (plus récentes en tête)', () => {
    const out = mergeAlerts(
      [ev(1, '2026-06-12T10:00:00Z'), ev(2, '2026-06-12T12:00:00Z')],
      [ev(3, '2026-06-12T11:00:00Z')],
      50,
    )
    expect(out.map((a) => a.id)).toEqual([2, 3, 1])
  })

  it('déduplique par id (un event présent dans les deux flux n’apparaît qu’une fois)', () => {
    const out = mergeAlerts(
      [ev(7, '2026-06-12T10:00:00Z')],
      [ev(7, '2026-06-12T10:00:00Z')],
      50,
    )
    expect(out).toHaveLength(1)
    expect(out[0].id).toBe(7)
  })

  it('à id égal, incoming remplace existing (donnée la plus fraîche)', () => {
    const out = mergeAlerts(
      [ev(7, '2026-06-12T10:00:00Z', { severity: 'info' })],
      [ev(7, '2026-06-12T10:00:00Z', { severity: 'critical' })],
      50,
    )
    expect(out).toHaveLength(1)
    expect(out[0].severity).toBe('critical')
  })

  it('borne le résultat à max (les plus récentes conservées)', () => {
    const out = mergeAlerts(
      [ev(1, '2026-06-12T10:00:00Z'), ev(2, '2026-06-12T11:00:00Z')],
      [ev(3, '2026-06-12T12:00:00Z')],
      2,
    )
    expect(out.map((a) => a.id)).toEqual([3, 2])
  })

  it('est indépendant de l’ordre d’arrivée (résultat déterministe)', () => {
    const a = ev(1, '2026-06-12T10:00:00Z')
    const b = ev(2, '2026-06-12T11:00:00Z')
    const c = ev(3, '2026-06-12T12:00:00Z')
    const r1 = mergeAlerts([a, b], [c], 50).map((x) => x.id)
    const r2 = mergeAlerts([c], [b, a], 50).map((x) => x.id)
    expect(r1).toEqual(r2)
  })

  it('à fired_at égal, départage par id décroissant (stable)', () => {
    const out = mergeAlerts(
      [ev(5, '2026-06-12T10:00:00Z'), ev(9, '2026-06-12T10:00:00Z')],
      [],
      50,
    )
    expect(out.map((a) => a.id)).toEqual([9, 5])
  })

  it('incoming vide renvoie existing trié et borné', () => {
    const out = mergeAlerts([ev(1, '2026-06-12T10:00:00Z')], [], 50)
    expect(out.map((a) => a.id)).toEqual([1])
  })
})
