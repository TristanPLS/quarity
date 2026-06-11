import { useEffect } from 'react'
import { AqiBadge, aqiLevelFromValue, Badge } from '../../components/ui'
import { paramLabel, type LocationAqi } from '../../api/types'
import { useAqiOverview } from './useAqiOverview'
import { AqiMap } from './AqiMap'
import styles from './AqiOverview.module.css'

function formatComputedAt(iso: string): string {
  const d = new Date(iso)
  return Number.isNaN(d.getTime())
    ? iso
    : d.toLocaleString('fr-FR', { dateStyle: 'short', timeStyle: 'short' })
}

/** Vue d'ensemble AQI : grille des lieux suivis avec leur indice US EPA courant (B10). */
export function AqiOverviewSection() {
  const { status, locations, computedAt, error, fetchOverview } = useAqiOverview()

  useEffect(() => {
    // Échec géré par l'état (UI) + le handler 401 global (déconnexion/redirection).
    void fetchOverview().catch(() => {})
  }, [fetchOverview])

  return (
    <section className={styles.section} aria-labelledby="aqi-heading">
      <div className={styles.head}>
        <h1 id="aqi-heading" className={styles.title}>
          Qualité de l'air
        </h1>
        <p className={styles.subtitle}>
          Indice US EPA courant de vos lieux suivis.
          {computedAt && (
            <>
              {' '}
              Calculé à <span className={styles.mono}>{formatComputedAt(computedAt)}</span>.
            </>
          )}
        </p>
      </div>

      {status === 'loading' && (
        <div className={styles.stateBox} aria-live="polite">
          <span className={styles.spinner} aria-hidden="true" /> Calcul des indices…
        </div>
      )}

      {status === 'error' && (
        <div role="alert" className={styles.alertError}>
          <strong>Impossible de charger les indices AQI.</strong>
          <span>{error}</span>
          <button type="button" className={styles.retry} onClick={() => void fetchOverview().catch(() => {})}>
            Réessayer
          </button>
        </div>
      )}

      {status === 'success' && locations.length === 0 && (
        <div className={styles.stateBox}>
          Aucun lieu suivi actif. La gestion des lieux et des règles arrive au prochain jalon front (B11).
        </div>
      )}

      {status === 'success' && locations.length > 0 && (
        <>
          <AqiMap locations={locations} />
          <div className={styles.grid}>
            {locations.map((loc) => (
              <AqiCard key={loc.tracked_location_id} loc={loc} />
            ))}
          </div>
        </>
      )}
    </section>
  )
}

function AqiCard({ loc }: { loc: LocationAqi }) {
  const aqi = loc.overall_aqi
  const hasAqi = loc.has_data && aqi != null

  return (
    <article className={styles.card}>
      <h2 className={styles.cardName} title={loc.name}>
        {loc.name}
      </h2>

      {hasAqi ? (
        <>
          <AqiBadge level={aqiLevelFromValue(aqi)} value={aqi} />
          <p className={styles.dominant}>
            Polluant dominant :{' '}
            <strong>{loc.dominant_parameter ? paramLabel(loc.dominant_parameter) : '—'}</strong>
          </p>
          {!loc.valid && (
            <p className={styles.caveat} title="Couverture de la fenêtre réglementaire insuffisante (&lt; 75 %)">
              ⚠ Couverture &lt; 75 % — valeur indicative
            </p>
          )}
          <ul className={styles.pollutants}>
            {loc.pollutants.map((p) => (
              <li key={p.parameter} className={styles.pollutantRow}>
                <span>{paramLabel(p.parameter)}</span>
                <span className={styles.pollutantRight}>
                  <span className={styles.mono}>{p.aqi}</span>
                  {p.is_dominant && <Badge tone="info">dominant</Badge>}
                </span>
              </li>
            ))}
          </ul>
        </>
      ) : (
        <p className={styles.noData}>Pas de mesure récente</p>
      )}
    </article>
  )
}
