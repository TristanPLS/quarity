import { useCallback, useState } from 'react'
import { api } from '../../api/client'
import { apiErrorMessage, type AlertRule, type AlertRuleFilters, type ListQuery, type Page } from '../../api/types'

type Status = 'idle' | 'loading' | 'success' | 'error'

export type RulesQuery = ListQuery & AlertRuleFilters

/** Charge la page courante de règles d'alerte (`GET /api/alert-rules`). */
export function useAlertRules() {
  const [status, setStatus] = useState<Status>('idle')
  const [page, setPage] = useState<Page<AlertRule> | null>(null)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async (query: RulesQuery) => {
    setStatus('loading')
    setError(null)
    try {
      const res = await api.alertRules.list(query)
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
