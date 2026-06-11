import { useEffect, useRef, useState } from 'react'
import { tokenStore } from '../../api/tokenStore'
import type { AlertEvent, AlertMessage } from '../../api/types'

export type WsStatus = 'connecting' | 'open' | 'closed'

/** Nombre d'alertes conservées en mémoire (les plus récentes en tête). */
const MAX_ALERTS = 50

/** URL WebSocket même origine (nginx proxifie `/api/ws` vers le back, Upgrade câblé).
 *  Le jeton d'accès passe en query string : une WebSocket navigateur ne peut pas poser
 *  d'en-tête `Authorization` (limitation HTML5). En prod, WSS chiffre la query (TLS). */
function wsUrl(token: string): string {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  return `${proto}//${window.location.host}/api/ws?token=${encodeURIComponent(token)}`
}

/**
 * Client WebSocket des alertes temps réel (B11a) — consomme le push B8/B8b
 * (`GET /api/ws`). Reconnexion avec backoff exponentiel borné (le jeton est relu à
 * CHAQUE tentative : après un refresh, la WS se rouvre avec le jeton frais — le back ne
 * renouvelle pas le jeton en vol, cf. B8). Tout est nettoyé au démontage.
 */
export function useAlertsSocket() {
  const [status, setStatus] = useState<WsStatus>('connecting')
  const [alerts, setAlerts] = useState<AlertEvent[]>([])

  const socketRef = useRef<WebSocket | null>(null)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const retriesRef = useRef(0)
  const stoppedRef = useRef(false)

  useEffect(() => {
    stoppedRef.current = false

    const scheduleReconnect = () => {
      if (stoppedRef.current) return
      const delay = Math.min(1000 * 2 ** retriesRef.current, 30000)
      retriesRef.current += 1
      timerRef.current = setTimeout(connect, delay)
    }

    function connect() {
      if (stoppedRef.current) return
      const token = tokenStore.getAccess()
      if (!token) {
        // Pas de jeton (déconnecté ou pas encore rafraîchi) — on retente plus tard.
        setStatus('connecting')
        scheduleReconnect()
        return
      }

      setStatus('connecting')
      const ws = new WebSocket(wsUrl(token))
      socketRef.current = ws

      ws.onopen = () => {
        retriesRef.current = 0
        setStatus('open')
      }
      ws.onmessage = (ev) => {
        try {
          const msg = JSON.parse(String(ev.data)) as AlertMessage
          if (msg.type === 'alert' && msg.event) {
            setAlerts((prev) => [msg.event, ...prev].slice(0, MAX_ALERTS))
          }
        } catch {
          // Charge utile illisible — ignorée (jamais de crash sur un message inattendu).
        }
      }
      ws.onerror = () => ws.close() // déclenche onclose → reconnexion
      ws.onclose = () => {
        socketRef.current = null
        if (!stoppedRef.current) {
          setStatus('closed')
          scheduleReconnect()
        }
      }
    }

    connect()

    return () => {
      stoppedRef.current = true
      if (timerRef.current) clearTimeout(timerRef.current)
      socketRef.current?.close()
      socketRef.current = null
    }
  }, [])

  return { status, alerts }
}
