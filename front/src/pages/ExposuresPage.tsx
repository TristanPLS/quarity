import { useCallback, useEffect, useMemo, useState, type FormEvent } from 'react'
import { api } from '../api/client'
import {
  apiErrorMessage,
  COMMON_TIMEZONES,
  daysMaskLabel,
  exposureCodeLabel,
  paramLabel,
  WEEKDAYS,
  type CreateTlpInput,
  type ExposureProfile,
  type ExposureResult,
  type TrackedLocation,
  type TrackedLocationProfile,
  type UpdateTlpInput,
} from '../api/types'
import { useCanWrite } from '../auth/usePermissions'
import { Badge, Button, Input } from '../components/ui'
import { Modal } from '../components/Modal'
import { useTrackedLocationProfiles, type ExposuresQuery } from '../features/exposures/useTrackedLocationProfiles'
import styles from './ExposuresPage.module.css'

const PAGE_SIZE = 25

type StatusFilter = 'all' | 'active' | 'inactive'

const isDaySet = (mask: number, bit: number) => (mask & (1 << bit)) !== 0
const toggleDay = (mask: number, bit: number) => mask ^ (1 << bit)
/** `HH:MM:SS` → `HH:MM` (valeur d'un `<input type=time>`). */
const hhmm = (t: string) => t.slice(0, 5)
const isoDate = (d: Date) => d.toISOString().slice(0, 10)

/** Gestion des associations lieu × profil + calcul de dose (B11c) — consomme B9a. */
export function ExposuresPage() {
  const canWrite = useCanWrite()
  const { status, page, error, load } = useTrackedLocationProfiles()

  const [locations, setLocations] = useState<TrackedLocation[]>([])
  const [profiles, setProfiles] = useState<ExposureProfile[]>([])
  const [refsReady, setRefsReady] = useState(false)

  const [locationFilter, setLocationFilter] = useState<number | 'all'>('all')
  const [profileFilter, setProfileFilter] = useState<number | 'all'>('all')
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('all')
  const [pageNum, setPageNum] = useState(1)
  const [refreshTick, setRefreshTick] = useState(0)

  const [creating, setCreating] = useState(false)
  const [editing, setEditing] = useState<TrackedLocationProfile | null>(null)
  const [deleting, setDeleting] = useState<TrackedLocationProfile | null>(null)
  const [doseOf, setDoseOf] = useState<TrackedLocationProfile | null>(null)

  useEffect(() => {
    let cancelled = false
    void Promise.all([
      api.trackedLocations.list({ page_size: 100, sort: 'name' }).catch(() => null),
      api.exposureProfiles.list({ page_size: 100, sort: 'name' }).catch(() => null),
    ])
      .then(([locs, profs]) => {
        if (cancelled) return
        if (locs) setLocations(locs.data)
        if (profs) setProfiles(profs.data)
      })
      .finally(() => {
        if (!cancelled) setRefsReady(true)
      })
    return () => {
      cancelled = true
    }
  }, [])

  const query = useMemo<ExposuresQuery>(
    () => ({
      tracked_location_id: locationFilter === 'all' ? undefined : locationFilter,
      exposure_profile_id: profileFilter === 'all' ? undefined : profileFilter,
      is_active: statusFilter === 'all' ? undefined : statusFilter === 'active',
      page: pageNum,
      page_size: PAGE_SIZE,
      sort: '-created_at',
    }),
    [locationFilter, profileFilter, statusFilter, pageNum],
  )

  useEffect(() => {
    const t = setTimeout(() => {
      void load(query).catch(() => {})
    }, 200)
    return () => clearTimeout(t)
  }, [load, query, refreshTick])

  const reload = useCallback(() => setRefreshTick((t) => t + 1), [])

  useEffect(() => {
    setPageNum(1)
  }, [locationFilter, profileFilter, statusFilter])

  const onMutated = () => {
    setCreating(false)
    setEditing(null)
    setDeleting(null)
    setPageNum(1)
    setRefreshTick((t) => t + 1)
  }

  const rows = page?.data ?? []
  const total = page?.total ?? 0
  const totalPages = Math.max(1, Math.ceil(total / PAGE_SIZE))
  const filtered = locationFilter !== 'all' || profileFilter !== 'all' || statusFilter !== 'all'
  const missingRefs = refsReady && (locations.length === 0 || profiles.length === 0)

  return (
    <main className={styles.main}>
      <div className={styles.head}>
        <div>
          <h1 className={styles.title}>Expositions</h1>
          <p className={styles.subtitle}>
            Profils d'exposition appliqués à vos lieux sur une plage horaire, et calcul de la dose.
            {total > 0 && (
              <>
                {' '}
                <span className={styles.mono}>{total}</span> au total.
              </>
            )}
          </p>
        </div>
        <Button onClick={() => setCreating(true)} disabled={!canWrite || locations.length === 0 || profiles.length === 0}>
          Nouvelle exposition
        </Button>
      </div>

      {!canWrite && (
        <div className={styles.readonly} role="status">
          Votre compte est en <strong>lecture seule</strong> : création, modification, calcul de dose et suppression sont
          désactivés.
        </div>
      )}

      {canWrite && missingRefs && (
        <div className={styles.readonly} role="status">
          Il faut au moins un <strong>lieu suivi</strong> et un <strong>profil d'exposition</strong> avant de créer une
          exposition (onglets « Lieux suivis » et « Profils d'exposition »).
        </div>
      )}

      <div className={styles.filters}>
        <select
          className={styles.select}
          value={locationFilter}
          onChange={(e) => setLocationFilter(e.target.value === 'all' ? 'all' : Number(e.target.value))}
          aria-label="Filtrer par lieu"
        >
          <option value="all">Tous les lieux</option>
          {locations.map((l) => (
            <option key={l.id} value={l.id}>
              {l.name}
            </option>
          ))}
        </select>
        <select
          className={styles.select}
          value={profileFilter}
          onChange={(e) => setProfileFilter(e.target.value === 'all' ? 'all' : Number(e.target.value))}
          aria-label="Filtrer par profil"
        >
          <option value="all">Tous les profils</option>
          {profiles.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
        <div className={styles.segmented} role="group" aria-label="Filtrer par statut">
          {(['all', 'active', 'inactive'] as const).map((f) => (
            <button
              key={f}
              type="button"
              className={`${styles.seg} ${statusFilter === f ? styles.segOn : ''}`}
              aria-pressed={statusFilter === f}
              onClick={() => setStatusFilter(f)}
            >
              {f === 'all' ? 'Toutes' : f === 'active' ? 'Actives' : 'Inactives'}
            </button>
          ))}
        </div>
      </div>

      {status === 'loading' && (
        <div className={styles.stateBox} aria-live="polite">
          <span className={styles.spinner} aria-hidden="true" /> Chargement des expositions…
        </div>
      )}

      {status === 'error' && (
        <div role="alert" className={styles.alertError}>
          <strong>Impossible de charger les expositions.</strong>
          <span>{error}</span>
          <button type="button" className={styles.retry} onClick={reload}>
            Réessayer
          </button>
        </div>
      )}

      {status === 'success' && rows.length === 0 && (
        <div className={styles.stateBox}>
          {filtered ? 'Aucune exposition ne correspond à ces critères.' : 'Aucune exposition pour le moment.'}
          {canWrite && !filtered && !missingRefs && <> Créez-en une avec « Nouvelle exposition ».</>}
        </div>
      )}

      {status === 'success' && rows.length > 0 && (
        <div className={styles.tableWrap}>
          <table className={styles.table}>
            <thead>
              <tr>
                <th scope="col">Lieu</th>
                <th scope="col">Profil</th>
                <th scope="col">Plage horaire</th>
                <th scope="col">Jours</th>
                <th scope="col">Statut</th>
                <th scope="col">
                  <span className="sr-only">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.map((t) => (
                <tr key={t.id}>
                  <td className={styles.name}>{t.tracked_location_name}</td>
                  <td>
                    {t.exposure_profile_name}
                    <div className={styles.sub}>{exposureCodeLabel(t.exposure_profile_code)}</div>
                  </td>
                  <td className={styles.mono}>
                    {hhmm(t.start_time)}–{hhmm(t.end_time)}
                    <div className={styles.sub}>{t.timezone}</div>
                  </td>
                  <td>{daysMaskLabel(t.days_mask)}</td>
                  <td>
                    {t.is_active ? (
                      <Badge tone="success" dot>
                        Active
                      </Badge>
                    ) : (
                      <Badge tone="neutral" dot>
                        Inactive
                      </Badge>
                    )}
                  </td>
                  <td className={styles.actions}>
                    <Button variant="ghost" onClick={() => setDoseOf(t)}>
                      Dose
                    </Button>
                    {canWrite && (
                      <Button variant="ghost" onClick={() => setEditing(t)}>
                        Éditer
                      </Button>
                    )}
                    {canWrite && (
                      <Button variant="ghost" onClick={() => setDeleting(t)}>
                        Supprimer
                      </Button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {status === 'success' && totalPages > 1 && (
        <div className={styles.pagination}>
          <Button variant="ghost" onClick={() => setPageNum((p) => Math.max(1, p - 1))} disabled={pageNum <= 1}>
            Précédent
          </Button>
          <span className={styles.pageInfo}>
            Page <span className={styles.mono}>{pageNum}</span> / {totalPages}
          </span>
          <Button
            variant="ghost"
            onClick={() => setPageNum((p) => Math.min(totalPages, p + 1))}
            disabled={pageNum >= totalPages}
          >
            Suivant
          </Button>
        </div>
      )}

      {creating && (
        <TlpFormModal
          mode="create"
          locations={locations}
          profiles={profiles}
          onClose={() => setCreating(false)}
          onSaved={onMutated}
        />
      )}
      {editing && (
        <TlpFormModal
          mode="edit"
          tlp={editing}
          locations={locations}
          profiles={profiles}
          onClose={() => setEditing(null)}
          onSaved={onMutated}
        />
      )}
      {deleting && <DeleteTlpModal tlp={deleting} onClose={() => setDeleting(null)} onDeleted={onMutated} />}
      {doseOf && <DoseModal tlp={doseOf} canWrite={canWrite} onClose={() => setDoseOf(null)} />}
    </main>
  )
}

interface FormModalProps {
  mode: 'create' | 'edit'
  tlp?: TrackedLocationProfile
  locations: TrackedLocation[]
  profiles: ExposureProfile[]
  onClose: () => void
  onSaved: () => void
}

function TlpFormModal({ mode, tlp, locations, profiles, onClose, onSaved }: FormModalProps) {
  const [locationId, setLocationId] = useState<number | ''>(tlp?.tracked_location_id ?? locations[0]?.id ?? '')
  const [profileId, setProfileId] = useState<number | ''>(tlp?.exposure_profile_id ?? profiles[0]?.id ?? '')
  const [startTime, setStartTime] = useState(tlp ? hhmm(tlp.start_time) : '08:00')
  const [endTime, setEndTime] = useState(tlp ? hhmm(tlp.end_time) : '18:00')
  const [daysMask, setDaysMask] = useState<number>(tlp?.days_mask ?? 31)
  const [timezone, setTimezone] = useState(tlp?.timezone ?? 'Europe/Paris')
  const [isActive, setIsActive] = useState(tlp?.is_active ?? true)
  const [submitting, setSubmitting] = useState(false)
  const [formError, setFormError] = useState<string | null>(null)

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault()
    setFormError(null)

    if (!startTime || !endTime) {
      setFormError('Renseignez la plage horaire.')
      return
    }
    if (startTime >= endTime) {
      setFormError("L'heure de fin doit être strictement après l'heure de début.")
      return
    }
    if (daysMask < 1) {
      setFormError('Sélectionnez au moins un jour.')
      return
    }

    setSubmitting(true)
    try {
      if (mode === 'create') {
        if (locationId === '' || profileId === '') {
          setFormError('Choisissez un lieu et un profil.')
          setSubmitting(false)
          return
        }
        const input: CreateTlpInput = {
          tracked_location_id: locationId,
          exposure_profile_id: profileId,
          start_time: startTime,
          end_time: endTime,
          days_mask: daysMask,
          timezone,
          is_active: isActive,
        }
        await api.trackedLocationProfiles.create(input)
      } else if (tlp) {
        // Lieu et profil IMMUABLES (absents du PATCH).
        const input: UpdateTlpInput = {
          start_time: startTime,
          end_time: endTime,
          days_mask: daysMask,
          timezone,
          is_active: isActive,
        }
        await api.trackedLocationProfiles.update(tlp.id, input)
      }
      onSaved()
    } catch (err) {
      setFormError(apiErrorMessage(err))
    } finally {
      setSubmitting(false)
    }
  }

  const title = mode === 'create' ? 'Nouvelle exposition' : 'Modifier l\'exposition'

  return (
    <Modal
      open
      title={title}
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" type="button" onClick={onClose} disabled={submitting}>
            Annuler
          </Button>
          <Button type="submit" form="tlp-form" loading={submitting}>
            {mode === 'create' ? 'Créer' : 'Enregistrer'}
          </Button>
        </>
      }
    >
      <form id="tlp-form" className={styles.form} onSubmit={onSubmit} noValidate>
        {formError && (
          <div role="alert" className={styles.formError}>
            {formError}
          </div>
        )}

        {mode === 'create' ? (
          <label className={styles.field}>
            <span className={styles.fieldLabel}>Lieu suivi</span>
            <select className={styles.select} value={locationId} onChange={(e) => setLocationId(Number(e.target.value))} required autoFocus>
              {locations.map((l) => (
                <option key={l.id} value={l.id}>
                  {l.name}
                </option>
              ))}
            </select>
          </label>
        ) : (
          <div className={styles.readonlyField}>
            <span className={styles.fieldLabel}>Lieu suivi</span>
            <span className={styles.readonlyValue}>
              {tlp?.tracked_location_name} <em>(non modifiable)</em>
            </span>
          </div>
        )}

        {mode === 'create' ? (
          <label className={styles.field}>
            <span className={styles.fieldLabel}>Profil d'exposition</span>
            <select className={styles.select} value={profileId} onChange={(e) => setProfileId(Number(e.target.value))} required>
              {profiles.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
          </label>
        ) : (
          <div className={styles.readonlyField}>
            <span className={styles.fieldLabel}>Profil d'exposition</span>
            <span className={styles.readonlyValue}>
              {tlp?.exposure_profile_name} <em>(non modifiable)</em>
            </span>
          </div>
        )}

        <div className={styles.row}>
          <label className={styles.field}>
            <span className={styles.fieldLabel}>Début</span>
            <input className={styles.timeInput} type="time" value={startTime} onChange={(e) => setStartTime(e.target.value)} required />
          </label>
          <label className={styles.field}>
            <span className={styles.fieldLabel}>Fin</span>
            <input className={styles.timeInput} type="time" value={endTime} onChange={(e) => setEndTime(e.target.value)} required />
          </label>
        </div>

        <fieldset className={styles.daysFieldset}>
          <legend className={styles.fieldLabel}>Jours actifs</legend>
          <div className={styles.days}>
            {WEEKDAYS.map((d) => (
              <label key={d.bit} className={`${styles.day} ${isDaySet(daysMask, d.bit) ? styles.dayOn : ''}`} title={d.label}>
                <input
                  type="checkbox"
                  className={styles.dayCheck}
                  aria-label={d.label}
                  checked={isDaySet(daysMask, d.bit)}
                  onChange={() => setDaysMask((m) => toggleDay(m, d.bit))}
                />
                {d.short}
              </label>
            ))}
          </div>
        </fieldset>

        <label className={styles.field}>
          <span className={styles.fieldLabel}>Fuseau horaire</span>
          <select className={styles.select} value={timezone} onChange={(e) => setTimezone(e.target.value)}>
            {COMMON_TIMEZONES.map((tz) => (
              <option key={tz} value={tz}>
                {tz}
              </option>
            ))}
          </select>
        </label>

        <label className={styles.checkRow}>
          <input type="checkbox" checked={isActive} onChange={(e) => setIsActive(e.target.checked)} />
          <span>Exposition active (prise en compte par le calcul de dose)</span>
        </label>
      </form>
    </Modal>
  )
}

function DeleteTlpModal({
  tlp,
  onClose,
  onDeleted,
}: {
  tlp: TrackedLocationProfile
  onClose: () => void
  onDeleted: () => void
}) {
  const [submitting, setSubmitting] = useState(false)
  const [err, setErr] = useState<string | null>(null)

  const onConfirm = async () => {
    setSubmitting(true)
    setErr(null)
    try {
      await api.trackedLocationProfiles.remove(tlp.id)
      onDeleted()
    } catch (e) {
      setErr(apiErrorMessage(e))
      setSubmitting(false)
    }
  }

  return (
    <Modal
      open
      title="Supprimer l'exposition"
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" type="button" onClick={onClose} disabled={submitting}>
            Annuler
          </Button>
          <Button variant="danger" type="button" onClick={() => void onConfirm()} loading={submitting}>
            Supprimer
          </Button>
        </>
      }
    >
      {err && (
        <div role="alert" className={styles.formError}>
          {err}
        </div>
      )}
      <p>
        Retirer le profil <strong>{tlp.exposure_profile_name}</strong> du lieu{' '}
        <strong>{tlp.tracked_location_name}</strong> ? Les résultats de dose calculés seront également supprimés. Cette
        action est irréversible.
      </p>
    </Modal>
  )
}

function DoseModal({
  tlp,
  canWrite,
  onClose,
}: {
  tlp: TrackedLocationProfile
  canWrite: boolean
  onClose: () => void
}) {
  const today = new Date()
  const monthAgo = new Date(today.getTime() - 30 * 86400000)

  const [results, setResults] = useState<ExposureResult[]>([])
  const [status, setStatus] = useState<'loading' | 'success' | 'error'>('loading')
  const [loadErr, setLoadErr] = useState<string | null>(null)

  const [from, setFrom] = useState(isoDate(monthAgo))
  const [to, setTo] = useState(isoDate(today))
  const [computing, setComputing] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)
  const [computeErr, setComputeErr] = useState<string | null>(null)

  const reload = useCallback(async () => {
    setStatus('loading')
    setLoadErr(null)
    try {
      const r = await api.trackedLocationProfiles.results(tlp.id)
      setResults(r)
      setStatus('success')
    } catch (e) {
      setLoadErr(apiErrorMessage(e))
      setStatus('error')
    }
  }, [tlp.id])

  useEffect(() => {
    void reload()
  }, [reload])

  const onCompute = async (e: FormEvent) => {
    e.preventDefault()
    setComputeErr(null)
    setNotice(null)
    if (!from || !to || from > to) {
      setComputeErr('Plage de dates invalide (début ≤ fin).')
      return
    }
    setComputing(true)
    try {
      const r = await api.trackedLocationProfiles.computeDose(tlp.id, { period_start: from, period_end: to })
      setResults(r)
      setStatus('success')
      setNotice(
        r.length === 0
          ? 'Calcul terminé : aucun seuil applicable (ce profil n\'a pas de seuil 1h/8h/24h).'
          : `Calcul terminé : ${r.length} résultat(s) sur la période.`,
      )
    } catch (err) {
      setComputeErr(apiErrorMessage(err))
    } finally {
      setComputing(false)
    }
  }

  return (
    <Modal
      open
      title={`Dose — ${tlp.tracked_location_name} × ${tlp.exposure_profile_name}`}
      onClose={onClose}
      footer={
        <Button variant="ghost" type="button" onClick={onClose}>
          Fermer
        </Button>
      }
    >
      {canWrite && (
        <form className={styles.doseForm} onSubmit={onCompute} noValidate>
          <div className={styles.thrFormTitle}>Calculer la dose sur une période</div>
          {computeErr && (
            <div role="alert" className={styles.formError}>
              {computeErr}
            </div>
          )}
          <div className={styles.doseRow}>
            <Input label="Du" type="date" value={from} onChange={(e) => setFrom(e.target.value)} max={to} />
            <Input label="Au" type="date" value={to} onChange={(e) => setTo(e.target.value)} min={from} />
            <Button type="submit" loading={computing}>
              Calculer
            </Button>
          </div>
          <p className={styles.hint}>Heures où la concentration glissante (MAX des stations) dépasse le seuil du profil.</p>
        </form>
      )}

      {notice && (
        <div className={styles.notice} role="status">
          {notice}
        </div>
      )}

      {status === 'loading' && (
        <div className={styles.stateBox} aria-live="polite">
          <span className={styles.spinner} aria-hidden="true" /> Chargement…
        </div>
      )}
      {status === 'error' && (
        <div role="alert" className={styles.formError}>
          {loadErr}
        </div>
      )}
      {status === 'success' &&
        (results.length === 0 ? (
          <p className={styles.muted}>Aucun résultat de dose. {canWrite && 'Lancez un calcul ci-dessus.'}</p>
        ) : (
          <div className={styles.doseTableWrap}>
          <table className={styles.doseTable}>
            <thead>
              <tr>
                <th scope="col">Polluant</th>
                <th scope="col">Seuil</th>
                <th scope="col">Période</th>
                <th scope="col" className={styles.numCol}>
                  Heures &gt; seuil
                </th>
                <th scope="col" className={styles.numCol}>
                  Couverture
                </th>
              </tr>
            </thead>
            <tbody>
              {results.map((r) => (
                <tr key={r.id}>
                  <td>
                    {paramLabel(r.parameter)} <span className={styles.mono}>({r.averaging_period})</span>
                  </td>
                  <td className={styles.mono}>{r.threshold_value}</td>
                  <td className={styles.mono}>
                    {r.period_start} → {r.period_end}
                  </td>
                  <td className={`${styles.mono} ${styles.numCol}`}>{r.hours_over_threshold}</td>
                  <td className={`${styles.mono} ${styles.numCol}`}>{r.sample_count} h</td>
                </tr>
              ))}
            </tbody>
          </table>
          </div>
        ))}
    </Modal>
  )
}
