import { useCallback, useState } from 'react'
import { api } from '../../api/client'
import { ApiError, type Measurement, type Parameter } from '../../api/types'
import { parseMeasuredAt } from '../../lib/datetime'

export interface ChartPoint extends Measurement {
  ts: number
}
export interface MeasurementQuery {
  location_id: number
  parameter: Parameter
  from: string
  to: string
}

type Status = 'idle' | 'loading' | 'success' | 'error'
interface State {
  status: Status
  points: ChartPoint[]
  unit: string
  count: number
  error: string | null
}
const INITIAL: State = { status: 'idle', points: [], unit: '', count: 0, error: null }

export function useMeasurements() {
  const [state, setState] = useState<State>(INITIAL)

  const fetchMeasurements = useCallback(async (q: MeasurementQuery) => {
    setState((s) => ({ ...s, status: 'loading', error: null }))
    try {
      const body = await api.measurements({ ...q, page: 1, page_size: 1000 })
      const points: ChartPoint[] = body.data
        .map((p) => ({ ...p, ts: parseMeasuredAt(p.measured_at) }))
        .filter((p) => Number.isFinite(p.ts) && Number.isFinite(p.value))
        .sort((a, b) => a.ts - b.ts)
      setState({ status: 'success', points, unit: points[0]?.unit ?? '', count: body.count, error: null })
    } catch (err) {
      if (err instanceof ApiError && err.status === 401) throw err // la route protégée gère la redirection
      const msg =
        err instanceof ApiError
          ? err.status === 403
            ? "Cette station n'appartient pas à votre organisation."
            : err.status === 400
              ? 'Paramètre invalide (allowlist : pm25, pm10, no2, o3, so2, co).'
              : `Erreur API (${err.status})`
          : err instanceof Error
            ? err.message
            : 'Erreur inconnue'
      setState({ ...INITIAL, status: 'error', error: msg })
    }
  }, [])

  return { ...state, fetchMeasurements }
}
