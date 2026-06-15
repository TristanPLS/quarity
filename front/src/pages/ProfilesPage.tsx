import { useCallback, useEffect, useMemo, useState, type FormEvent } from 'react'
import { api } from '../api/client'
import {
  apiErrorMessage,
  AVERAGING_PERIODS,
  AVERAGING_PERIOD_LABELS,
  EXPOSURE_CODES,
  exposureCodeLabel,
  PARAMETERS,
  paramLabel,
  type AveragingPeriod,
  type CreateExposureProfileInput,
  type ExposureCode,
  type ExposureProfile,
  type ExposureThreshold,
  type Parameter,
  type UpdateExposureProfileInput,
} from '../api/types'
import { useCanWrite } from '../auth/usePermissions'
import { Badge, Button, ErrorState, Input, SelectField, Spinner, StateBox } from '../components/ui'
import { Modal } from '../components/Modal'
import { useExposureProfiles, type ProfilesQuery } from '../features/profiles/useExposureProfiles'
import styles from './ProfilesPage.module.css'

const PAGE_SIZE = 25

type ScopeFilter = 'all' | 'system' | 'custom'

/** Gestion des profils d'exposition + seuils (B11c) — consomme le CRUD B9a. */
export function ProfilesPage() {
  const canWrite = useCanWrite()
  const { status, page, error, load } = useExposureProfiles()

  const [q, setQ] = useState('')
  const [scope, setScope] = useState<ScopeFilter>('all')
  const [pageNum, setPageNum] = useState(1)
  const [refreshTick, setRefreshTick] = useState(0)

  const [creating, setCreating] = useState(false)
  const [editing, setEditing] = useState<ExposureProfile | null>(null)
  const [deleting, setDeleting] = useState<ExposureProfile | null>(null)
  const [thresholdsOf, setThresholdsOf] = useState<ExposureProfile | null>(null)

  const query = useMemo<ProfilesQuery>(
    () => ({
      q: q.trim() || undefined,
      scope: scope === 'all' ? undefined : scope,
      page: pageNum,
      page_size: PAGE_SIZE,
      sort: 'name',
    }),
    [q, scope, pageNum],
  )

  useEffect(() => {
    const t = setTimeout(() => {
      void load(query).catch(() => {})
    }, 250)
    return () => clearTimeout(t)
  }, [load, query, refreshTick])

  const reload = useCallback(() => setRefreshTick((t) => t + 1), [])

  useEffect(() => {
    setPageNum(1)
  }, [q, scope])

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
  const filtered = q.trim() !== '' || scope !== 'all'

  return (
    <main className={styles.main}>
      <div className={styles.head}>
        <div>
          <h1 className={styles.title}>Profils d'exposition</h1>
          <p className={styles.subtitle}>
            Profils de population sensible et leurs seuils adaptés. Les profils système sont partagés (lecture seule).
            {total > 0 && (
              <>
                {' '}
                <span className={styles.mono}>{total}</span> au total.
              </>
            )}
          </p>
        </div>
        <Button onClick={() => setCreating(true)} disabled={!canWrite}>
          Nouveau profil
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
          placeholder="Rechercher (nom ou code)…"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          aria-label="Rechercher un profil"
        />
        <div className={styles.segmented} role="group" aria-label="Filtrer par portée">
          {(['all', 'system', 'custom'] as const).map((f) => (
            <button
              key={f}
              type="button"
              className={`${styles.seg} ${scope === f ? styles.segOn : ''}`}
              aria-pressed={scope === f}
              onClick={() => setScope(f)}
            >
              {f === 'all' ? 'Tous' : f === 'system' ? 'Système' : 'Personnalisés'}
            </button>
          ))}
        </div>
      </div>

      {status === 'loading' && (
        <StateBox ariaLive="polite">
          <Spinner /> Chargement des profils…
        </StateBox>
      )}

      {status === 'error' && (
        <ErrorState title="Impossible de charger les profils." message={error} onRetry={reload} />
      )}

      {status === 'success' && rows.length === 0 && (
        <StateBox>
          {filtered ? 'Aucun profil ne correspond à ces critères.' : "Aucun profil d'exposition."}
          {canWrite && !filtered && <> Créez-en un avec « Nouveau profil ».</>}
        </StateBox>
      )}

      {status === 'success' && rows.length > 0 && (
        <div className={styles.tableWrap}>
          <table className={styles.table}>
            <thead>
              <tr>
                <th scope="col">Profil</th>
                <th scope="col">Type</th>
                <th scope="col">Description</th>
                <th scope="col">
                  <span className="sr-only">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.map((p) => {
                const editable = !p.is_system && canWrite
                return (
                  <tr key={p.id}>
                    <td>
                      <div className={styles.name}>{p.name}</div>
                      <div className={styles.sub}>{exposureCodeLabel(p.code)}</div>
                    </td>
                    <td>
                      {p.is_system ? <Badge tone="info">Système</Badge> : <Badge tone="neutral">Personnalisé</Badge>}
                    </td>
                    <td className={styles.desc}>{p.description || <span className={styles.muted}>—</span>}</td>
                    <td className={styles.actions}>
                      <Button variant="ghost" onClick={() => setThresholdsOf(p)}>
                        Seuils
                      </Button>
                      {editable && (
                        <Button variant="ghost" onClick={() => setEditing(p)}>
                          Éditer
                        </Button>
                      )}
                      {editable && (
                        <Button variant="ghost" onClick={() => setDeleting(p)}>
                          Supprimer
                        </Button>
                      )}
                    </td>
                  </tr>
                )
              })}
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

      {creating && <ProfileFormModal mode="create" onClose={() => setCreating(false)} onSaved={onMutated} />}
      {editing && (
        <ProfileFormModal mode="edit" profile={editing} onClose={() => setEditing(null)} onSaved={onMutated} />
      )}
      {deleting && <DeleteProfileModal profile={deleting} onClose={() => setDeleting(null)} onDeleted={onMutated} />}
      {thresholdsOf && (
        <ThresholdsModal profile={thresholdsOf} canWrite={canWrite} onClose={() => setThresholdsOf(null)} />
      )}
    </main>
  )
}

interface FormModalProps {
  mode: 'create' | 'edit'
  profile?: ExposureProfile
  onClose: () => void
  onSaved: () => void
}

function ProfileFormModal({ mode, profile, onClose, onSaved }: FormModalProps) {
  const [code, setCode] = useState<ExposureCode>((profile?.code as ExposureCode) ?? 'general')
  const [name, setName] = useState(profile?.name ?? '')
  const [description, setDescription] = useState(profile?.description ?? '')
  const [submitting, setSubmitting] = useState(false)
  const [formError, setFormError] = useState<string | null>(null)

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault()
    setFormError(null)
    if (!name.trim()) {
      setFormError('Le nom est obligatoire.')
      return
    }
    setSubmitting(true)
    try {
      if (mode === 'create') {
        const input: CreateExposureProfileInput = {
          code,
          name: name.trim(),
          description: description.trim() || undefined,
        }
        await api.exposureProfiles.create(input)
      } else if (profile) {
        // `code` IMMUABLE (absent) ; `description: ''` efface côté back.
        const input: UpdateExposureProfileInput = { name: name.trim(), description: description.trim() }
        await api.exposureProfiles.update(profile.id, input)
      }
      onSaved()
    } catch (err) {
      setFormError(apiErrorMessage(err))
    } finally {
      setSubmitting(false)
    }
  }

  const title = mode === 'create' ? "Nouveau profil d'exposition" : `Modifier « ${profile?.name} »`

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
          <Button type="submit" form="profile-form" loading={submitting}>
            {mode === 'create' ? 'Créer le profil' : 'Enregistrer'}
          </Button>
        </>
      }
    >
      <form id="profile-form" className={styles.form} onSubmit={onSubmit} noValidate>
        {formError && (
          <div role="alert" className={styles.formError}>
            {formError}
          </div>
        )}
        {mode === 'create' ? (
          <SelectField label="Population cible" value={code} onChange={(e) => setCode(e.target.value as ExposureCode)} autoFocus>
            {EXPOSURE_CODES.map((c) => (
              <option key={c} value={c}>
                {exposureCodeLabel(c)}
              </option>
            ))}
          </SelectField>
        ) : (
          <div className={styles.readonlyField}>
            <span className={styles.fieldLabel}>Population cible</span>
            <span className={styles.readonlyValue}>
              {exposureCodeLabel(profile?.code ?? '')} <em>(non modifiable)</em>
            </span>
          </div>
        )}
        <Input
          label="Nom"
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
          maxLength={200}
          placeholder="Ex. Enfants - écoles de l'agglo"
        />
        <Input
          label="Description (optionnel)"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          maxLength={1000}
        />
      </form>
    </Modal>
  )
}

function DeleteProfileModal({
  profile,
  onClose,
  onDeleted,
}: {
  profile: ExposureProfile
  onClose: () => void
  onDeleted: () => void
}) {
  const [submitting, setSubmitting] = useState(false)
  const [err, setErr] = useState<string | null>(null)

  const onConfirm = async () => {
    setSubmitting(true)
    setErr(null)
    try {
      await api.exposureProfiles.remove(profile.id)
      onDeleted()
    } catch (e) {
      setErr(apiErrorMessage(e))
      setSubmitting(false)
    }
  }

  return (
    <Modal
      open
      title="Supprimer le profil"
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
        Supprimer définitivement <strong>{profile.name}</strong> et ses seuils ? Un profil appliqué à un lieu ne peut
        pas être supprimé (retirez d'abord l'association). Cette action est irréversible.
      </p>
    </Modal>
  )
}

function ThresholdsModal({
  profile,
  canWrite,
  onClose,
}: {
  profile: ExposureProfile
  canWrite: boolean
  onClose: () => void
}) {
  const editable = !profile.is_system && canWrite
  const [items, setItems] = useState<ExposureThreshold[]>([])
  const [status, setStatus] = useState<'loading' | 'success' | 'error'>('loading')
  const [loadErr, setLoadErr] = useState<string | null>(null)

  const [parameter, setParameter] = useState<Parameter>('pm25')
  const [threshold, setThreshold] = useState('')
  const [averaging, setAveraging] = useState<AveragingPeriod>('1h')
  const [adding, setAdding] = useState(false)
  const [formErr, setFormErr] = useState<string | null>(null)
  const [busyId, setBusyId] = useState<number | null>(null)

  const reload = useCallback(async () => {
    setStatus('loading')
    setLoadErr(null)
    try {
      const r = await api.exposureProfiles.listThresholds(profile.id)
      setItems(r)
      setStatus('success')
    } catch (e) {
      setLoadErr(apiErrorMessage(e))
      setStatus('error')
    }
  }, [profile.id])

  useEffect(() => {
    void reload()
  }, [reload])

  const onAdd = async (e: FormEvent) => {
    e.preventDefault()
    setFormErr(null)
    const thr = Number(threshold)
    if (threshold.trim() === '' || !Number.isFinite(thr) || thr < 0) {
      setFormErr('Seuil invalide (un nombre ≥ 0).')
      return
    }
    setAdding(true)
    try {
      await api.exposureProfiles.addThreshold(profile.id, {
        parameter,
        threshold_value: thr,
        averaging_period: averaging,
      })
      setThreshold('')
      await reload()
    } catch (err) {
      setFormErr(apiErrorMessage(err))
    } finally {
      setAdding(false)
    }
  }

  const onRemove = async (t: ExposureThreshold) => {
    setBusyId(t.id)
    setFormErr(null)
    try {
      await api.exposureProfiles.removeThreshold(profile.id, t.id)
      await reload()
    } catch (e) {
      setFormErr(apiErrorMessage(e))
    } finally {
      setBusyId(null)
    }
  }

  return (
    <Modal
      open
      title={`Seuils — ${profile.name}`}
      onClose={onClose}
      footer={
        <Button variant="ghost" type="button" onClick={onClose}>
          Fermer
        </Button>
      }
    >
      {status === 'loading' && (
        <StateBox ariaLive="polite">
          <Spinner /> Chargement des seuils…
        </StateBox>
      )}
      {status === 'error' && (
        <div role="alert" className={styles.formError}>
          {loadErr}
        </div>
      )}
      {status === 'success' && (
        <>
          {items.length === 0 ? (
            <p className={styles.muted}>Aucun seuil défini pour ce profil.</p>
          ) : (
            <table className={styles.thrTable}>
              <thead>
                <tr>
                  <th scope="col">Polluant</th>
                  <th scope="col">Seuil</th>
                  <th scope="col">Période</th>
                  {editable && (
                    <th scope="col">
                      <span className="sr-only">Action</span>
                    </th>
                  )}
                </tr>
              </thead>
              <tbody>
                {items.map((t) => (
                  <tr key={t.id}>
                    <td>{paramLabel(t.parameter)}</td>
                    <td className={styles.mono}>
                      {t.threshold_value} {t.unit}
                    </td>
                    <td>{AVERAGING_PERIOD_LABELS[t.averaging_period as AveragingPeriod] ?? t.averaging_period}</td>
                    {editable && (
                      <td className={styles.thrAction}>
                        <Button
                          variant="ghost"
                          onClick={() => void onRemove(t)}
                          disabled={busyId === t.id}
                          loading={busyId === t.id}
                        >
                          Retirer
                        </Button>
                      </td>
                    )}
                  </tr>
                ))}
              </tbody>
            </table>
          )}

          {editable && (
            <form className={styles.thrForm} onSubmit={onAdd} noValidate>
              <div className={styles.thrFormTitle}>Ajouter un seuil</div>
              {formErr && (
                <div role="alert" className={styles.formError}>
                  {formErr}
                </div>
              )}
              <div className={styles.thrFormRow}>
                <SelectField
                  label="Polluant"
                  value={parameter}
                  onChange={(e) => setParameter(e.target.value as Parameter)}
                  disabled={adding}
                >
                  {PARAMETERS.map((p) => (
                    <option key={p} value={p}>
                      {paramLabel(p)}
                    </option>
                  ))}
                </SelectField>
                <Input
                  label="Seuil"
                  type="number"
                  min={0}
                  step="any"
                  mono
                  value={threshold}
                  onChange={(e) => setThreshold(e.target.value)}
                  disabled={adding}
                />
                <SelectField
                  label="Période"
                  value={averaging}
                  onChange={(e) => setAveraging(e.target.value as AveragingPeriod)}
                  disabled={adding}
                >
                  {AVERAGING_PERIODS.map((a) => (
                    <option key={a} value={a}>
                      {AVERAGING_PERIOD_LABELS[a]}
                    </option>
                  ))}
                </SelectField>
                <Button type="submit" loading={adding}>
                  Ajouter
                </Button>
              </div>
            </form>
          )}
          {!editable && profile.is_system && <p className={styles.muted}>Profil système : seuils en lecture seule.</p>}
        </>
      )}
    </Modal>
  )
}
