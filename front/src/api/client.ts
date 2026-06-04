import { ApiError, type Me, type MeasurementsQuery, type MeasurementsResponse, type TokenResponse } from './types'
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
}
