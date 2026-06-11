import { useMemo } from 'react'
import { CircleMarker, MapContainer, Popup, TileLayer } from 'react-leaflet'
import type { LatLngBoundsExpression } from 'leaflet'
import { AQI_SCALE, aqiLevelFromValue, type AqiLevel } from '../../components/ui'
import { paramLabel, type LocationAqi } from '../../api/types'
import 'leaflet/dist/leaflet.css'
import styles from './AqiMap.module.css'

/** Couleurs US EPA par niveau AQI — alignées sur les tokens `--aqi-N` (cohérence badges). */
const AQI_COLORS: Record<AqiLevel, string> = {
  1: '#00E400',
  2: '#FFFF00',
  3: '#FF7E00',
  4: '#FF0000',
  5: '#8F3F97',
  6: '#7E0023',
}
const NO_DATA_COLOR = '#9AA0A6'

type Located = LocationAqi & { latitude: number; longitude: number }

/**
 * Carte des lieux suivis (B10b) — un marqueur coloré par niveau AQI courant, réutilisant
 * les coordonnées exposées par `/api/aqi` (aucun second appel réseau). Les lieux sans
 * coordonnées sont omis de la carte (ils restent dans la grille de jauges). Fond OpenStreetMap.
 */
export function AqiMap({ locations }: { locations: LocationAqi[] }) {
  const located = useMemo(
    () => locations.filter((l): l is Located => l.latitude != null && l.longitude != null),
    [locations],
  )
  const bounds = useMemo<LatLngBoundsExpression | undefined>(
    () =>
      located.length > 0
        ? located.map((l) => [l.latitude, l.longitude] as [number, number])
        : undefined,
    [located],
  )

  if (located.length === 0) return null

  return (
    <div className={styles.wrap}>
      <MapContainer
        bounds={bounds}
        boundsOptions={{ padding: [40, 40], maxZoom: 13 }}
        scrollWheelZoom={false}
        className={styles.map}
        aria-label="Carte des lieux suivis"
      >
        <TileLayer
          url="https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png"
          attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>'
        />
        {located.map((loc) => {
          const aqi = loc.has_data ? loc.overall_aqi : null
          const color = aqi != null ? AQI_COLORS[aqiLevelFromValue(aqi)] : NO_DATA_COLOR
          return (
            <CircleMarker
              key={loc.tracked_location_id}
              center={[loc.latitude, loc.longitude]}
              radius={10}
              pathOptions={{ color: '#1A1A1A', weight: 1, fillColor: color, fillOpacity: 0.9 }}
            >
              <Popup>
                <strong>{loc.name}</strong>
                <br />
                {aqi != null ? (
                  <>
                    AQI {aqi} — {AQI_SCALE[aqiLevelFromValue(aqi)].label}
                    <br />
                    Polluant dominant :{' '}
                    {loc.dominant_parameter ? paramLabel(loc.dominant_parameter) : '—'}
                  </>
                ) : (
                  'Pas de mesure récente'
                )}
              </Popup>
            </CircleMarker>
          )
        })}
      </MapContainer>
    </div>
  )
}
