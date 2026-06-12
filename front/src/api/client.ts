import {
  ApiError,
  type AlertRule,
  type AlertRuleFilters,
  type AqiOverview,
  type CreateAlertRuleInput,
  type CreateTrackedLocationInput,
  type ListQuery,
  type Me,
  type MeasurementsQuery,
  type MeasurementsResponse,
  type Page,
  type RunOutcome,
  type RunQuery,
  type TokenResponse,
  type TrackedLocation,
  type TrackedLocationDetail,
  type TrackedLocationFilters,
  type UpdateAlertRuleInput,
  type UpdateTrackedLocationInput,
} from './types'
import { tokenStore } from './tokenStore'

// Vide par défaut => chemins relatifs /api (proxy Vite en dev, nginx en prod) => zéro CORS.
const BASE_URL = import.meta.env.VITE_API_BASE_URL ?? ''

let onAuthFailure: (() => void) | null = null
export function setAuthFailureHandler(fn: () => void): void {
  onAuthFailure = fn
}

// --- mutex de refresh : un seul /refresh en vol ---
let refreshInFlight: Promise<string | null> | null = null

async function doRefresh(): Promise<string | null> {
  const refresh = tokenStore.getRefresh()
  if (!refresh) return null
  const res = await fetch(`${BASE_URL}/api/auth/refresh`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ refresh_token: refresh }),
  })
  if (!res.ok) {
    tokenStore.clear()
    return null
  }
  const data = (await res.json()) as TokenResponse
  tokenStore.setTokens(data)
  return data.access_token
}

function refreshAccessToken(): Promise<string | null> {
  if (!refreshInFlight) {
    refreshInFlight = doRefresh().finally(() => {
      refreshInFlight = null
    })
  }
  return refreshInFlight
}

interface RequestOptions extends Omit<RequestInit, 'body'> {
  body?: unknown
  auth?: boolean
  _retried?: boolean
}

async function request<T>(path: string, opts: RequestOptions = {}): Promise<T> {
  const { body, auth = true, headers, _retried, ...rest } = opts

  const finalHeaders = new Headers(headers)
  if (body !== undefined) finalHeaders.set('Content-Type', 'application/json')
  if (auth) {
    const token = tokenStore.getAccess()
    if (token) finalHeaders.set('Authorization', `Bearer ${token}`)
  }

  const res = await fetch(`${BASE_URL}${path}`, {
    ...rest,
    headers: finalHeaders,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })

  // 401 : tentative de refresh + re-essai UNE fois
  if (res.status === 401 && auth && !_retried) {
    const newToken = await refreshAccessToken()
    if (newToken) return request<T>(path, { ...opts, _retried: true })
    tokenStore.clear()
    onAuthFailure?.()
    throw new ApiError(401, null, 'Session expirée')
  }

  if (!res.ok) {
    let parsed: unknown = null
    try {
      parsed = await res.json()
    } catch {
      /* corps non-JSON */
    }
    throw new ApiError(res.status, parsed)
  }

  if (res.status === 204) return undefined as T
  return (await res.json()) as T
}

/**
 * Sérialise des paramètres de requête plats en query string (ignore `undefined`/`null`/`''`).
 * Param typé `object` : les interfaces (sans signature d'index) ne satisfont pas `Record`.
 */
function buildQuery(params: object): string {
  const sp = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== '') sp.set(k, String(v))
  }
  const s = sp.toString()
  return s ? `?${s}` : ''
}

export const api = {
  login: (email: string, password: string) =>
    request<TokenResponse>('/api/auth/login', { method: 'POST', auth: false, body: { email, password } }),

  logout: (refresh_token: string) =>
    request<void>('/api/auth/logout', { method: 'POST', body: { refresh_token } }),

  me: () => request<Me>('/api/auth/me', { method: 'GET' }),

  measurements: (q: MeasurementsQuery) => {
    const params = new URLSearchParams({
      location_id: String(q.location_id),
      parameter: q.parameter,
      from: q.from,
      to: q.to,
      page: String(q.page ?? 1),
      page_size: String(q.page_size ?? 1000),
    })
    return request<MeasurementsResponse>(`/api/measurements?${params.toString()}`, { method: 'GET' })
  },

  // AQI courant des lieux suivis de l'org (B10).
  aqiOverview: () => request<AqiOverview>('/api/aqi', { method: 'GET' }),

  // --- Lieux suivis : CRUD (B6, consommé en B11b) ---
  trackedLocations: {
    list: (params: ListQuery & TrackedLocationFilters = {}) =>
      request<Page<TrackedLocation>>(`/api/tracked-locations${buildQuery(params)}`, { method: 'GET' }),
    get: (id: number) =>
      request<TrackedLocationDetail>(`/api/tracked-locations/${id}`, { method: 'GET' }),
    create: (input: CreateTrackedLocationInput) =>
      request<TrackedLocationDetail>('/api/tracked-locations', { method: 'POST', body: input }),
    update: (id: number, input: UpdateTrackedLocationInput) =>
      request<TrackedLocationDetail>(`/api/tracked-locations/${id}`, { method: 'PATCH', body: input }),
    remove: (id: number) =>
      request<void>(`/api/tracked-locations/${id}`, { method: 'DELETE' }),
  },

  // --- Règles d'alerte : CRUD + force-check (B6/B7, consommé en B11b) ---
  alertRules: {
    list: (params: ListQuery & AlertRuleFilters = {}) =>
      request<Page<AlertRule>>(`/api/alert-rules${buildQuery(params)}`, { method: 'GET' }),
    get: (id: number) => request<AlertRule>(`/api/alert-rules/${id}`, { method: 'GET' }),
    create: (input: CreateAlertRuleInput) =>
      request<AlertRule>('/api/alert-rules', { method: 'POST', body: input }),
    update: (id: number, input: UpdateAlertRuleInput) =>
      request<AlertRule>(`/api/alert-rules/${id}`, { method: 'PATCH', body: input }),
    remove: (id: number) => request<void>(`/api/alert-rules/${id}`, { method: 'DELETE' }),
    /** Force-check immédiat (B7) : réévalue la règle maintenant, hors boucle périodique. */
    run: (id: number, q: RunQuery = {}) =>
      request<RunOutcome>(`/api/alert-rules/${id}/run${buildQuery(q)}`, { method: 'POST' }),
  },
}
