import { lazy, Suspense } from 'react'
import type { LocationAqi } from '../../api/types'
import styles from './AqiOverview.module.css'

// Code-split : `leaflet` + `react-leaflet` + leaflet.css (~250 Kio) ne sont charges
// QUE lorsque la carte est rendue (au moins un lieu suivi), pas dans le bundle d'entree.
const AqiMap = lazy(() => import('./AqiMap').then((m) => ({ default: m.AqiMap })))

/** Carte AQI chargee a la demande (Suspense + fallback). API identique a `AqiMap`. */
export function AqiMapLazy({ locations }: { locations: LocationAqi[] }) {
  return (
    <Suspense
      fallback={
        <div className={styles.stateBox} aria-live="polite">
          <span className={styles.spinner} aria-hidden="true" /> Chargement de la carte…
        </div>
      }
    >
      <AqiMap locations={locations} />
    </Suspense>
  )
}
