import { createContext, useContext, useState, useEffect, useCallback, type ReactNode } from 'react'
import { api, setAuthFailureHandler } from '../api/client'
import { tokenStore } from '../api/tokenStore'
import type { Me } from '../api/types'

interface AuthState {
  user: Me | null
  isAuthenticated: boolean
  isLoading: boolean
  login: (email: string, password: string) => Promise<void>
  logout: () => Promise<void>
}

const AuthContext = createContext<AuthState | undefined>(undefined)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<Me | null>(null)
  const [isLoading, setIsLoading] = useState(true)

  const logout = useCallback(async () => {
    const refresh = tokenStore.getRefresh()
    try {
      if (refresh) await api.logout(refresh)
    } catch {
      /* best effort */
    } finally {
      tokenStore.clear()
      setUser(null)
    }
  }, [])

  useEffect(() => {
    setAuthFailureHandler(() => {
      tokenStore.clear()
      setUser(null)
    })
  }, [])

  // Bootstrap : restaure la session si un refresh_token existe (refresh silencieux via le 401-handler).
  useEffect(() => {
    let cancelled = false
    ;(async () => {
      if (!tokenStore.hasSession()) {
        setIsLoading(false)
        return
      }
      try {
        const me = await api.me()
        if (!cancelled) setUser(me)
      } catch {
        if (!cancelled) {
          tokenStore.clear()
          setUser(null)
        }
      } finally {
        if (!cancelled) setIsLoading(false)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])

  const login = useCallback(async (email: string, password: string) => {
    const tokens = await api.login(email, password)
    tokenStore.setTokens(tokens)
    const me = await api.me()
    setUser(me)
  }, [])

  return (
    <AuthContext.Provider value={{ user, isAuthenticated: !!user, isLoading, login, logout }}>
      {children}
    </AuthContext.Provider>
  )
}

// eslint-disable-next-line react-refresh/only-export-components
export function useAuth(): AuthState {
  const ctx = useContext(AuthContext)
  if (!ctx) throw new Error('useAuth doit être utilisé dans <AuthProvider>')
  return ctx
}
