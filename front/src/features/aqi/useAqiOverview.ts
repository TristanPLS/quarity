import { useCallback, useState } from 'react'
import { api } from '../../api/client'
import { ApiError, type LocationAqi } from '../../api/types'

type Status = 'idle' | 'loading' | 'success' | 'error'

/** Charge la vue d'ensemble AQI (`GET /api/aqi`) des lieux suivis de l'org. */
export function useAqiOverview() {
  const [status, setStatus] = useState<Status>('idle')
  const [locations, setLocations] = useState<LocationAqi[]>([])
  const [computedAt, setComputedAt] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const fetchOverview = useCallback(async () => {
    setStatus('loading')
    setError(null)
    try {
      const res = await api.aqiOverview()
      setLocations(res.data)
      setComputedAt(res.computed_at)
      setStatus('success')
      return res
    } catch (e) {
      setError(e instanceof ApiError ? e.message : 'Erreur réseau')
      setStatus('error')
      throw e
    }
  }, [])

  return { status, locations, computedAt, error, fetchOverview }
}
