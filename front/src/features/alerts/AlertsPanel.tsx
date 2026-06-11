import { paramLabel, type AlertEvent } from '../../api/types'
import { useAlertsSocket, type WsStatus } from './useAlertsSocket'
import styles from './AlertsPanel.module.css'

const STATUS_LABEL: Record<WsStatus, string> = {
  connecting: 'Connexion…',
  open: 'En direct',
  closed: 'Reconnexion…',
}

/** Classe de sévérité (miroir de `alert_rules.severity`). Défaut : warning. */
function severityClass(severity: string): string {
  if (severity === 'critical') return styles.critical
  if (severity === 'info') return styles.info
  return styles.warning
}

function formatTime(iso: string): string {
  const d = new Date(iso)
  return Number.isNaN(d.getTime())
    ? iso
    : d.toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit', second: '2-digit' })
}

/**
 * Panneau d'alertes temps réel (B11a) — affiche en direct les dépassements de seuil
 * poussés sur la WebSocket (`/api/ws`, B8/B8b). Consomme enfin la chaîne B7→B8→B8b.
 */
export function AlertsPanel() {
  const { status, alerts } = useAlertsSocket()

  return (
    <section className={styles.panel} aria-labelledby="alerts-heading">
      <div className={styles.head}>
        <h2 id="alerts-heading" className={styles.title}>
          Alertes temps réel
        </h2>
        <span className={`${styles.conn} ${styles[status]}`} role="status">
          <span className={styles.dot} aria-hidden="true" />
          {STATUS_LABEL[status]}
        </span>
      </div>

      {alerts.length === 0 ? (
        <p className={styles.empty} aria-live="polite">
          En écoute - les dépassements de seuil de vos lieux suivis s'afficheront ici en direct.
        </p>
      ) : (
        <ul className={styles.list} aria-live="polite">
          {alerts.map((a: AlertEvent) => (
            <li key={a.id} className={`${styles.item} ${severityClass(a.severity)}`}>
              <span className={styles.sev}>{a.severity}</span>
              <span className={styles.body}>
                <strong>{paramLabel(a.parameter_code)}</strong>{' '}
                <span className={styles.mono}>{a.measured_value}</span> {a.unit}{' '}
                <span className={styles.threshold}>
                  ({a.comparator} {a.threshold_value})
                </span>{' '}
                - station <span className={styles.mono}>{a.openaq_location_id}</span>
              </span>
              <time className={styles.time} dateTime={a.fired_at}>
                {formatTime(a.fired_at)}
              </time>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}
