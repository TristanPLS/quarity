import { useCallback, useState } from 'react'
import { api } from '../../api/client'
import { apiErrorMessage, type ListQuery, type Page, type TrackedLocation, type TrackedLocationFilters } from '../../api/types'

type Status = 'idle' | 'loading' | 'success' | 'error'

export type LocationsQuery = ListQuery & TrackedLocationFilters

/** Charge la page courante de lieux suivis (`GET /api/tracked-locations`). */
export function useTrackedLocations() {
  const [status, setStatus] = useState<Status>('idle')
  const [page, setPage] = useState<Page<TrackedLocation> | null>(null)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async (query: LocationsQuery) => {
    setStatus('loading')
    setError(null)
    try {
      const res = await api.trackedLocations.list(query)
      setPage(res)
      setStatus('success')
      return res
    } catch (e) {
      setError(apiErrorMessage(e))
      setStatus('error')
      throw e
    }
  }, [])

  return { status, page, error, load }
}
