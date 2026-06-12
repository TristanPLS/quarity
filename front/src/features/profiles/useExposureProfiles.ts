import { useCallback, useState } from 'react'
import { api } from '../../api/client'
import { apiErrorMessage, type ExposureProfile, type ExposureProfileFilters, type ListQuery, type Page } from '../../api/types'

type Status = 'idle' | 'loading' | 'success' | 'error'

export type ProfilesQuery = ListQuery & ExposureProfileFilters

/** Charge la page courante de profils d'exposition (`GET /api/exposure-profiles`). */
export function useExposureProfiles() {
  const [status, setStatus] = useState<Status>('idle')
  const [page, setPage] = useState<Page<ExposureProfile> | null>(null)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async (query: ProfilesQuery) => {
    setStatus('loading')
    setError(null)
    try {
      const res = await api.exposureProfiles.list(query)
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
