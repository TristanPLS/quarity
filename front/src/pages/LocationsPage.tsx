import { useCallback, useEffect, useMemo, useState, type FormEvent } from 'react'
import { api } from '../api/client'
import {
  apiErrorMessage,
  type CreateTrackedLocationInput,
  type TrackedLocation,
  type UpdateTrackedLocationInput,
} from '../api/types'
import { useCanWrite } from '../auth/usePermissions'
import { Badge, Button, ErrorState, Input, Spinner, StateBox } from '../components/ui'
import { Modal } from '../components/Modal'
import { useTrackedLocations, type LocationsQuery } from '../features/locations/useTrackedLocations'
import styles from './LocationsPage.module.css'

const PAGE_SIZE = 25

type ActiveFilter = 'all' | 'active' | 'paused'

/** Gestion CRUD des lieux suivis de l'org (B11b) — consomme les endpoints B6. */
export function LocationsPage() {
  const canWrite = useCanWrite()
  const { status, page, error, load } = useTrackedLocations()

  const [q, setQ] = useState('')
  const [active, setActive] = useState<ActiveFilter>('all')
  const [pageNum, setPageNum] = useState(1)

  // Modales : `creating`, le lieu en cours d'édition, le lieu en cours de suppression.
  const [creating, setCreating] = useState(false)
  const [editing, setEditing] = useState<TrackedLocation | null>(null)
  const [deleting, setDeleting] = useState<TrackedLocation | null>(null)

  const query = useMemo<LocationsQuery>(
    () => ({
      q: q.trim() || undefined,
      is_active: active === 'all' ? undefined : active === 'active',
      page: pageNum,
      page_size: PAGE_SIZE,
      sort: 'name',
    }),
    [q, active, pageNum],
  )

  const [refreshTick, setRefreshTick] = useState(0)

  // Recharge sur changement de filtre/page, ou sur demande explicite via `refreshTick`
  // (mutation, réessai) — ce dernier garantit un rechargement même quand `query` est
  // inchangé (ex. mutation alors qu'on est déjà page 1). Léger debounce pour la frappe.
  useEffect(() => {
    const t = setTimeout(() => {
      void load(query).catch(() => {})
    }, 250)
    return () => clearTimeout(t)
  }, [load, query, refreshTick])

  const reload = useCallback(() => setRefreshTick((t) => t + 1), [])

  // Tout changement de filtre ramène à la première page.
  useEffect(() => {
    setPageNum(1)
  }, [q, active])

  const onMutated = () => {
    setCreating(false)
    setEditing(null)
    setDeleting(null)
    setPageNum(1) // une mutation réaffiche la première page…
    setRefreshTick((t) => t + 1) // …et force le rechargement même si on y était déjà.
  }

  const rows = page?.data ?? []
  const total = page?.total ?? 0
  const totalPages = Math.max(1, Math.ceil(total / PAGE_SIZE))
  const filtered = q.trim() !== '' || active !== 'all'

  return (
    <main className={styles.main}>
      <div className={styles.head}>
        <div>
          <h1 className={styles.title}>Lieux suivis</h1>
          <p className={styles.subtitle}>
            Les lieux de votre organisation et leurs stations OpenAQ.
            {total > 0 && (
              <>
                {' '}
                <span className={styles.mono}>{total}</span> au total.
              </>
            )}
          </p>
        </div>
        <Button onClick={() => setCreating(true)} disabled={!canWrite}>
          Nouveau lieu
        </Button>
      </div>

      {!canWrite && (
        <div className={styles.readonly} role="status">
          Votre compte est en <strong>lecture seule</strong> : création, modification et suppression sont désactivées.
        </div>
      )}

      <div className={styles.filters}>
        <input
          type="search"
          className={styles.search}
          placeholder="Rechercher par nom…"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          aria-label="Rechercher un lieu"
        />
        <div className={styles.segmented} role="group" aria-label="Filtrer par état">
          {(['all', 'active', 'paused'] as const).map((f) => (
            <button
              key={f}
              type="button"
              className={`${styles.seg} ${active === f ? styles.segOn : ''}`}
              aria-pressed={active === f}
              onClick={() => setActive(f)}
            >
              {f === 'all' ? 'Tous' : f === 'active' ? 'Actifs' : 'En pause'}
            </button>
          ))}
        </div>
      </div>

      {status === 'loading' && (
        <StateBox ariaLive="polite">
          <Spinner /> Chargement des lieux…
        </StateBox>
      )}

      {status === 'error' && (
        <ErrorState title="Impossible de charger les lieux." message={error} onRetry={reload} />
      )}

      {status === 'success' && rows.length === 0 && (
        <StateBox>
          {filtered ? 'Aucun lieu ne correspond à ces critères.' : 'Aucun lieu suivi pour le moment.'}
          {canWrite && !filtered && <> Créez-en un avec « Nouveau lieu ».</>}
        </StateBox>
      )}

      {status === 'success' && rows.length > 0 && (
        <div className={styles.tableWrap}>
          <table className={styles.table}>
            <thead>
              <tr>
                <th scope="col">Nom</th>
                <th scope="col" className={styles.numCol}>
                  Stations
                </th>
                <th scope="col" className={styles.numCol}>
                  Règles actives
                </th>
                <th scope="col">État</th>
                <th scope="col">
                  <span className="sr-only">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.map((loc) => (
                <tr key={loc.id}>
                  <td>
                    <div className={styles.name}>{loc.name}</div>
                    {loc.description && <div className={styles.desc}>{loc.description}</div>}
                  </td>
                  <td className={`${styles.mono} ${styles.numCol}`}>{loc.station_count}</td>
                  <td className={`${styles.mono} ${styles.numCol}`}>{loc.active_rule_count}</td>
                  <td>
                    {loc.is_active ? (
                      <Badge tone="success" dot>
                        Actif
                      </Badge>
                    ) : (
                      <Badge tone="neutral" dot>
                        En pause
                      </Badge>
                    )}
                  </td>
                  <td className={styles.actions}>
                    <Button variant="ghost" onClick={() => setEditing(loc)} disabled={!canWrite}>
                      Éditer
                    </Button>
                    <Button variant="ghost" onClick={() => setDeleting(loc)} disabled={!canWrite}>
                      Supprimer
                    </Button>
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

      {creating && <LocationFormModal mode="create" onClose={() => setCreating(false)} onSaved={onMutated} />}
      {editing && (
        <LocationFormModal mode="edit" location={editing} onClose={() => setEditing(null)} onSaved={onMutated} />
      )}
      {deleting && (
        <DeleteLocationModal location={deleting} onClose={() => setDeleting(null)} onDeleted={onMutated} />
      )}
    </main>
  )
}

/** Sépare une saisie « 1001, 1002 » en identifiants numériques (virgules ou espaces). */
function parseStationIds(raw: string): number[] {
  return raw
    .split(/[\s,]+/)
    .filter(Boolean)
    .map(Number)
}

interface FormModalProps {
  mode: 'create' | 'edit'
  location?: TrackedLocation
  onClose: () => void
  onSaved: () => void
}

function LocationFormModal({ mode, location, onClose, onSaved }: FormModalProps) {
  const [name, setName] = useState(location?.name ?? '')
  const [description, setDescription] = useState(location?.description ?? '')
  const [stations, setStations] = useState('') // création uniquement (stations immuables ensuite)
  const [isActive, setIsActive] = useState(location?.is_active ?? true)
  const [submitting, setSubmitting] = useState(false)
  const [formError, setFormError] = useState<string | null>(null)

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault()
    setFormError(null)

    if (!name.trim()) {
      setFormError('Le nom est obligatoire.')
      return
    }

    let ids: number[] = []
    if (mode === 'create') {
      ids = parseStationIds(stations)
      if (ids.length === 0) {
        setFormError('Indiquez au moins une station OpenAQ (ex. 1001).')
        return
      }
      if (ids.some((n) => !Number.isInteger(n) || n <= 0)) {
        setFormError('Les identifiants de station doivent être des entiers positifs.')
        return
      }
      if (ids.length > 50) {
        setFormError('50 stations maximum.')
        return
      }
    }

    setSubmitting(true)
    try {
      if (mode === 'create') {
        const input: CreateTrackedLocationInput = {
          name: name.trim(),
          description: description.trim() || undefined,
          openaq_location_ids: ids,
        }
        await api.trackedLocations.create(input)
      } else if (location) {
        // PATCH : on envoie `description` même vide ('') car le back l'efface alors
        // (NULLIF(trim, '')). À la création on OMET le champ vide (undefined) ; ici on
        // doit l'envoyer pour permettre d'effacer une description existante — la
        // divergence create/edit est donc volontaire, pas une incohérence.
        const input: UpdateTrackedLocationInput = {
          name: name.trim(),
          description: description.trim(),
          is_active: isActive,
        }
        await api.trackedLocations.update(location.id, input)
      }
      onSaved()
    } catch (err) {
      setFormError(apiErrorMessage(err))
    } finally {
      setSubmitting(false)
    }
  }

  const title = mode === 'create' ? 'Nouveau lieu suivi' : `Modifier « ${location?.name} »`

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
          <Button type="submit" form="location-form" loading={submitting}>
            {mode === 'create' ? 'Créer le lieu' : 'Enregistrer'}
          </Button>
        </>
      }
    >
      <form id="location-form" className={styles.form} onSubmit={onSubmit} noValidate>
        {formError && (
          <div role="alert" className={styles.formError}>
            {formError}
          </div>
        )}
        <Input
          label="Nom"
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
          maxLength={200}
          placeholder="Écoles du centre-ville"
          autoFocus
        />
        <Input
          label="Description (optionnel)"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          maxLength={2000}
        />
        {mode === 'create' ? (
          <Input
            label="Stations OpenAQ"
            value={stations}
            onChange={(e) => setStations(e.target.value)}
            mono
            placeholder="1001, 1002"
            hint="Identifiants OpenAQ séparés par des virgules (1 à 50). La première devient la station de référence."
          />
        ) : (
          <label className={styles.checkRow}>
            <input type="checkbox" checked={isActive} onChange={(e) => setIsActive(e.target.checked)} />
            <span>Surveillance active (décocher met le lieu en pause)</span>
          </label>
        )}
      </form>
    </Modal>
  )
}

function DeleteLocationModal({
  location,
  onClose,
  onDeleted,
}: {
  location: TrackedLocation
  onClose: () => void
  onDeleted: () => void
}) {
  const [submitting, setSubmitting] = useState(false)
  const [err, setErr] = useState<string | null>(null)

  const onConfirm = async () => {
    setSubmitting(true)
    setErr(null)
    try {
      await api.trackedLocations.remove(location.id)
      onDeleted()
    } catch (e) {
      setErr(apiErrorMessage(e))
      setSubmitting(false)
    }
  }

  return (
    <Modal
      open
      title="Supprimer le lieu"
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
        Supprimer définitivement <strong>{location.name}</strong> ? Ses stations liées et ses{' '}
        <strong>{location.active_rule_count > 0 ? `${location.active_rule_count} règle(s) active(s)` : 'règles'}</strong>{' '}
        d'alerte seront également supprimées (cascade). Les alertes déjà déclenchées sont conservées. Cette action est
        irréversible.
      </p>
    </Modal>
  )
}
