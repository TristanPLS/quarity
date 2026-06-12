import { useCallback, useState } from 'react'
import { api } from '../../api/client'
import { apiErrorMessage, type ListQuery, type Page, type TlpFilters, type TrackedLocationProfile } from '../../api/types'

type Status = 'idle' | 'loading' | 'success' | 'error'

export type ExposuresQuery = ListQuery & TlpFilters

/** Charge la page courante d'associations lieu × profil (`GET /api/tracked-location-profiles`). */
export function useTrackedLocationProfiles() {
  const [status, setStatus] = useState<Status>('idle')
  const [page, setPage] = useState<Page<TrackedLocationProfile> | null>(null)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async (query: ExposuresQuery) => {
    setStatus('loading')
    setError(null)
    try {
      const res = await api.trackedLocationProfiles.list(query)
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
