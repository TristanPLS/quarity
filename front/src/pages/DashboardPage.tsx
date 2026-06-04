import { useEffect, useMemo, useState, type FormEvent } from 'react'
import { useNavigate } from 'react-router'
import {
  CartesianGrid,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
  type TooltipProps,
} from 'recharts'
import { useAuth } from '../auth/AuthContext'
import { ApiError, PARAMETERS, type Parameter } from '../api/types'
import { formatFull, formatTick } from '../lib/datetime'
import { useMeasurements, type ChartPoint, type MeasurementQuery } from '../features/measurements/useMeasurements'
import { Button } from '../components/ui'
import styles from './DashboardPage.module.css'

const BRAND_LINE = '#1FA8B8' // Cyan respiration — accent de DONNÉE (jamais AQI)
const BRAND_GRID = '#CFE8F5'
const BRAND_AXIS = '#13242E'

const PARAM_LABELS: Record<Parameter, string> = {
  pm25: 'PM2.5',
  pm10: 'PM10',
  no2: 'NO₂',
  o3: 'O₃',
  so2: 'SO₂',
  co: 'CO',
}

function CustomTooltip({ active, payload }: TooltipProps<number, string>) {
  if (!active || !payload?.length) return null
  const p = payload[0].payload as ChartPoint
  return (
    <div className={styles.tooltip}>
      <div className={styles.tooltipTime}>{formatFull(p.ts)}</div>
      <div className={styles.tooltipValue}>
        <span className={styles.mono}>{p.value}</span> <span className={styles.unit}>{p.unit}</span>
      </div>
    </div>
  )
}

export function DashboardPage() {
  const navigate = useNavigate()
  const { user, logout } = useAuth()
  const { status, points, unit, count, error, fetchMeasurements } = useMeasurements()

  // Plage par défaut alignée sur les données de démo (seed mai 2026).
  const [form, setForm] = useState<MeasurementQuery>({
    location_id: 1001,
    parameter: 'pm25',
    from: '2026-05-01',
    to: '2026-06-05',
  })

  const runQuery = (q: MeasurementQuery) => {
    fetchMeasurements(q).catch((err) => {
      if (err instanceof ApiError && err.status === 401) navigate('/login', { replace: true })
    })
  }

  useEffect(() => {
    runQuery(form)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const onSubmit = (e: FormEvent) => {
    e.preventDefault()
    runQuery(form)
  }

  const summary = useMemo(() => {
    if (!points.length) return null
    const vals = points.map((p) => p.value)
    return { min: Math.min(...vals), max: Math.max(...vals), avg: vals.reduce((a, b) => a + b, 0) / vals.length }
  }, [points])

  const isLoading = status === 'loading'
  const isEmpty = status === 'success' && points.length === 0

  return (
    <div className={styles.app}>
      <header className={styles.topbar}>
        <div className={styles.brand}>
          <span className={styles.logoRing} aria-hidden="true" />
          <span className={styles.brandName}>Quarity</span>
        </div>
        <div className={styles.userBox}>
          {user && (
            <span className={styles.user}>
              {user.email} <span className={styles.role}>· {user.role}</span>
            </span>
          )}
          <Button variant="ghost" onClick={() => void logout()}>
            Déconnexion
          </Button>
        </div>
      </header>

      <main className={styles.main}>
        <div className={styles.head}>
          <h1 className={styles.title}>Série temporelle</h1>
          <p className={styles.subtitle}>Mesures OpenAQ par station, paramètre et plage de dates.</p>
        </div>

        <form className={styles.form} onSubmit={onSubmit}>
          <div className={styles.field}>
            <label htmlFor="location_id">Station (location_id)</label>
            <input
              id="location_id"
              type="number"
              min={1}
              required
              className={styles.mono}
              value={form.location_id}
              onChange={(e) => setForm((f) => ({ ...f, location_id: Number(e.target.value) }))}
            />
          </div>
          <div className={styles.field}>
            <label htmlFor="parameter">Paramètre</label>
            <select
              id="parameter"
              value={form.parameter}
              onChange={(e) => setForm((f) => ({ ...f, parameter: e.target.value as Parameter }))}
            >
              {PARAMETERS.map((p) => (
                <option key={p} value={p}>
                  {PARAM_LABELS[p]}
                </option>
              ))}
            </select>
          </div>
          <div className={styles.field}>
            <label htmlFor="from">Du</label>
            <input id="from" type="date" required max={form.to} value={form.from} onChange={(e) => setForm((f) => ({ ...f, from: e.target.value }))} />
          </div>
          <div className={styles.field}>
            <label htmlFor="to">Au</label>
            <input id="to" type="date" required min={form.from} value={form.to} onChange={(e) => setForm((f) => ({ ...f, to: e.target.value }))} />
          </div>
          <Button type="submit" loading={isLoading}>
            Afficher
          </Button>
        </form>

        {status === 'success' && summary && (
          <div className={styles.summary}>
            <Stat label="Points" value={String(count)} />
            <Stat label="Min" value={summary.min} unit={unit} />
            <Stat label="Moyenne" value={summary.avg.toFixed(1)} unit={unit} />
            <Stat label="Max" value={summary.max} unit={unit} />
          </div>
        )}

        {status === 'error' && (
          <div role="alert" className={styles.alertError}>
            <strong>Impossible de charger les mesures.</strong>
            <span>{error}</span>
            <button type="button" className={styles.retry} onClick={() => runQuery(form)}>
              Réessayer
            </button>
          </div>
        )}

        {isLoading && (
          <div className={styles.stateBox} aria-live="polite">
            <span className={styles.spinner} aria-hidden="true" /> Chargement des mesures…
          </div>
        )}

        {isEmpty && (
          <div className={styles.stateBox}>Aucune mesure pour cette station, ce paramètre et cette plage.</div>
        )}

        {status === 'success' && points.length > 0 && (
          <>
            <figure className={styles.chartCard}>
              <figcaption className={styles.chartCap}>
                {PARAM_LABELS[form.parameter]} — station <span className={styles.mono}>{form.location_id}</span>
                {unit && <> ({unit})</>}
              </figcaption>
              <ResponsiveContainer width="100%" height={360}>
                <LineChart data={points} margin={{ top: 8, right: 24, left: 8, bottom: 8 }}>
                  <CartesianGrid stroke={BRAND_GRID} strokeDasharray="3 3" />
                  <XAxis
                    dataKey="ts"
                    type="number"
                    scale="time"
                    domain={['dataMin', 'dataMax']}
                    interval="preserveStartEnd"
                    tickFormatter={formatTick}
                    tick={{ fill: BRAND_AXIS, fontSize: 12 }}
                    stroke={BRAND_AXIS}
                    minTickGap={40}
                  />
                  <YAxis dataKey="value" tick={{ fill: BRAND_AXIS, fontSize: 12 }} stroke={BRAND_AXIS} width={48} />
                  <Tooltip content={<CustomTooltip />} />
                  <Line
                    type="monotone"
                    dataKey="value"
                    stroke={BRAND_LINE}
                    strokeWidth={2}
                    dot={false}
                    activeDot={{ r: 4, fill: BRAND_LINE }}
                    isAnimationActive={false}
                    name={PARAM_LABELS[form.parameter]}
                  />
                </LineChart>
              </ResponsiveContainer>
            </figure>

            <div className={styles.tableWrap}>
              <table className={styles.table}>
                <caption className="sr-only">
                  Points de mesure {PARAM_LABELS[form.parameter]} pour la station {form.location_id}
                </caption>
                <thead>
                  <tr>
                    <th scope="col">Horodatage (UTC)</th>
                    <th scope="col">Capteur</th>
                    <th scope="col" className={styles.numCol}>Valeur</th>
                    <th scope="col">Unité</th>
                  </tr>
                </thead>
                <tbody>
                  {points.map((p, i) => (
                    <tr key={`${p.ts}-${p.sensor_id}-${i}`}>
                      <td className={styles.mono}>{formatFull(p.ts)}</td>
                      <td className={styles.mono}>{p.sensor_id}</td>
                      <td className={`${styles.mono} ${styles.numCol}`}>{p.value}</td>
                      <td>{p.unit}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </>
        )}
      </main>
    </div>
  )
}

function Stat({ label, value, unit }: { label: string; value: string | number; unit?: string }) {
  return (
    <div className={styles.stat}>
      <span className={styles.statLabel}>{label}</span>
      <span className={styles.statValue}>
        <span className={styles.mono}>{value}</span>
        {unit && <span className={styles.unit}> {unit}</span>}
      </span>
    </div>
  )
}
