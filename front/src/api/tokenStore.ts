// access_token en MÉMOIRE (jamais persisté) ; refresh_token en localStorage (survit au F5).
const REFRESH_KEY = 'quarity.refresh_token'

let accessToken: string | null = null

export const tokenStore = {
  getAccess: (): string | null => accessToken,
  setAccess: (t: string | null): void => {
    accessToken = t
  },

  getRefresh: (): string | null => localStorage.getItem(REFRESH_KEY),
  setRefresh: (t: string | null): void => {
    if (t) localStorage.setItem(REFRESH_KEY, t)
    else localStorage.removeItem(REFRESH_KEY)
  },

  setTokens: (tr: { access_token: string; refresh_token: string }): void => {
    accessToken = tr.access_token
    localStorage.setItem(REFRESH_KEY, tr.refresh_token)
  },

  clear: (): void => {
    accessToken = null
    localStorage.removeItem(REFRESH_KEY)
  },

  hasSession: (): boolean => !!localStorage.getItem(REFRESH_KEY),
}
