import { useCallback, useEffect, useMemo, useState, type FormEvent } from 'react'
import { api } from '../api/client'
import {
  apiErrorMessage,
  COMPARATORS,
  PARAMETERS,
  paramLabel,
  SEVERITIES,
  SEVERITY_LABELS,
  type AlertRule,
  type Comparator,
  type CreateAlertRuleInput,
  type Parameter,
  type RuleStatus,
  type Severity,
  type TrackedLocation,
  type UpdateAlertRuleInput,
} from '../api/types'
import { useCanWrite } from '../auth/usePermissions'
import { Badge, Button, ErrorState, Input, SelectField, Spinner, StateBox } from '../components/ui'
import { Modal } from '../components/Modal'
import { useAlertRules, type RulesQuery } from '../features/rules/useAlertRules'
import styles from './RulesPage.module.css'

const PAGE_SIZE = 25

type StatusFilter = 'all' | 'active' | 'inactive'
type Notice = { kind: 'info' | 'error'; text: string }
type Busy = { id: number; kind: 'test' | 'toggle' }

/** Libellé court d'une règle : son nom, sinon « lieu · polluant ». */
function ruleLabel(r: AlertRule): string {
  return r.name?.trim() || `${r.tracked_location_name} · ${paramLabel(r.parameter)}`
}

function SeverityBadge({ severity }: { severity: string }) {
  const tone = severity === 'critical' ? 'danger' : severity === 'warning' ? 'warning' : 'info'
  return <Badge tone={tone}>{SEVERITY_LABELS[severity as Severity] ?? severity}</Badge>
}

/** Gestion CRUD des règles d'alerte de l'org (B11b) — consomme les endpoints B6/B7. */
export function RulesPage() {
  const canWrite = useCanWrite()
  const { status, page, error, load } = useAlertRules()

  // Lieux de l'org : alimentent les selects de filtre et de création.
  const [locations, setLocations] = useState<TrackedLocation[]>([])
  const [locationsReady, setLocationsReady] = useState(false)

  const [q, setQ] = useState('')
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('all')
  const [locationFilter, setLocationFilter] = useState<number | 'all'>('all')
  const [severityFilter, setSeverityFilter] = useState<Severity | 'all'>('all')
  const [pageNum, setPageNum] = useState(1)
  const [refreshTick, setRefreshTick] = useState(0)

  const [creating, setCreating] = useState(false)
  const [editing, setEditing] = useState<AlertRule | null>(null)
  const [deleting, setDeleting] = useState<AlertRule | null>(null)
  const [busy, setBusy] = useState<Busy | null>(null)
  const [notice, setNotice] = useState<Notice | null>(null)

  useEffect(() => {
    let cancelled = false
    void api.trackedLocations
      .list({ page_size: 100, sort: 'name' })
      .then((res) => {
        if (!cancelled) setLocations(res.data)
      })
      .catch(() => {})
      .finally(() => {
        if (!cancelled) setLocationsReady(true)
      })
    return () => {
      cancelled = true
    }
  }, [])

  const query = useMemo<RulesQuery>(
    () => ({
      q: q.trim() || undefined,
      tracked_location_id: locationFilter === 'all' ? undefined : locationFilter,
      status: statusFilter === 'all' ? undefined : statusFilter,
      severity: severityFilter === 'all' ? undefined : severityFilter,
      page: pageNum,
      page_size: PAGE_SIZE,
      sort: '-created_at',
    }),
    [q, statusFilter, locationFilter, severityFilter, pageNum],
  )

  // Recharge sur changement de filtre/page ou sur demande explicite (mutation, réessai).
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
  }, [q, statusFilter, locationFilter, severityFilter])

  const onMutated = () => {
    setCreating(false)
    setEditing(null)
    setDeleting(null)
    setPageNum(1)
    setRefreshTick((t) => t + 1)
  }

  const runTest = async (rule: AlertRule) => {
    setBusy({ id: rule.id, kind: 'test' })
    setNotice(null)
    try {
      const o = await api.alertRules.run(rule.id)
      setNotice({
        kind: 'info',
        text: `Règle « ${ruleLabel(rule)} » testée : ${o.evaluated} mesure(s) évaluée(s), ${o.breaches} dépassement(s), ${o.events_created} alerte(s) créée(s).`,
      })
    } catch (e) {
      setNotice({ kind: 'error', text: apiErrorMessage(e) })
    } finally {
      setBusy(null)
    }
  }

  const toggleStatus = async (rule: AlertRule) => {
    const next: RuleStatus = rule.status === 'active' ? 'inactive' : 'active'
    setBusy({ id: rule.id, kind: 'toggle' })
    setNotice(null)
    try {
      await api.alertRules.update(rule.id, { status: next })
      setNotice({ kind: 'info', text: `Règle « ${ruleLabel(rule)} » ${next === 'active' ? 'activée' : 'désactivée'}.` })
      reload()
    } catch (e) {
      setNotice({ kind: 'error', text: apiErrorMessage(e) })
    } finally {
      setBusy(null)
    }
  }

  const rows = page?.data ?? []
  const total = page?.total ?? 0
  const totalPages = Math.max(1, Math.ceil(total / PAGE_SIZE))
  const filtered = q.trim() !== '' || statusFilter !== 'all' || locationFilter !== 'all' || severityFilter !== 'all'
  const noLocations = locationsReady && locations.length === 0

  return (
    <main className={styles.main}>
      <div className={styles.head}>
        <div>
          <h1 className={styles.title}>Règles d'alerte</h1>
          <p className={styles.subtitle}>
            Seuils surveillés sur vos lieux ; un dépassement déclenche une alerte temps réel.
            {total > 0 && (
              <>
                {' '}
                <span className={styles.mono}>{total}</span> au total.
              </>
            )}
          </p>
        </div>
        <Button onClick={() => setCreating(true)} disabled={!canWrite || locations.length === 0}>
          Nouvelle règle
        </Button>
      </div>

      {!canWrite && (
        <div className={styles.readonly} role="status">
          Votre compte est en <strong>lecture seule</strong> : création, modification, test et suppression sont désactivés.
        </div>
      )}

      {canWrite && noLocations && (
        <div className={styles.readonly} role="status">
          Aucun lieu suivi : créez d'abord un lieu dans l'onglet « Lieux suivis » avant d'y adosser des règles.
        </div>
      )}

      {notice && (
        <div
          className={`${styles.notice} ${notice.kind === 'error' ? styles.noticeError : styles.noticeInfo}`}
          role={notice.kind === 'error' ? 'alert' : 'status'}
        >
          <span>{notice.text}</span>
          <button type="button" className={styles.noticeClose} onClick={() => setNotice(null)} aria-label="Fermer">
            ×
          </button>
        </div>
      )}

      <div className={styles.filters}>
        <input
          type="search"
          className={styles.search}
          placeholder="Rechercher (règle ou lieu)…"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          aria-label="Rechercher une règle"
        />
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
          value={severityFilter}
          onChange={(e) => setSeverityFilter(e.target.value as Severity | 'all')}
          aria-label="Filtrer par sévérité"
        >
          <option value="all">Toutes sévérités</option>
          {SEVERITIES.map((s) => (
            <option key={s} value={s}>
              {SEVERITY_LABELS[s]}
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
        <StateBox ariaLive="polite">
          <Spinner /> Chargement des règles…
        </StateBox>
      )}

      {status === 'error' && (
        <ErrorState title="Impossible de charger les règles." message={error} onRetry={reload} />
      )}

      {status === 'success' && rows.length === 0 && (
        <StateBox>
          {filtered ? 'Aucune règle ne correspond à ces critères.' : "Aucune règle d'alerte pour le moment."}
          {canWrite && !filtered && !noLocations && <> Créez-en une avec « Nouvelle règle ».</>}
        </StateBox>
      )}

      {status === 'success' && rows.length > 0 && (
        <div className={styles.tableWrap}>
          <table className={styles.table}>
            <thead>
              <tr>
                <th scope="col">Règle / Lieu</th>
                <th scope="col">Condition</th>
                <th scope="col">Sévérité</th>
                <th scope="col">Statut</th>
                <th scope="col">
                  <span className="sr-only">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.id}>
                  <td>
                    <div className={styles.name}>{r.name?.trim() || paramLabel(r.parameter)}</div>
                    <div className={styles.sub}>{r.tracked_location_name}</div>
                  </td>
                  <td className={styles.mono}>
                    {paramLabel(r.parameter)} {r.comparator} {r.threshold_value}
                  </td>
                  <td>
                    <SeverityBadge severity={r.severity} />
                  </td>
                  <td>
                    {r.status === 'active' ? (
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
                    <Button
                      variant="ghost"
                      onClick={() => void runTest(r)}
                      disabled={!canWrite || r.status !== 'active' || busy?.id === r.id}
                      loading={busy?.id === r.id && busy.kind === 'test'}
                      title={r.status !== 'active' ? 'Règle inactive — rien à évaluer' : 'Réévaluer maintenant'}
                    >
                      Tester
                    </Button>
                    <Button
                      variant="ghost"
                      onClick={() => void toggleStatus(r)}
                      disabled={!canWrite || busy?.id === r.id}
                      loading={busy?.id === r.id && busy.kind === 'toggle'}
                    >
                      {r.status === 'active' ? 'Désactiver' : 'Activer'}
                    </Button>
                    <Button variant="ghost" onClick={() => setEditing(r)} disabled={!canWrite || busy?.id === r.id}>
                      Éditer
                    </Button>
                    <Button variant="ghost" onClick={() => setDeleting(r)} disabled={!canWrite || busy?.id === r.id}>
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

      {creating && (
        <RuleFormModal mode="create" locations={locations} onClose={() => setCreating(false)} onSaved={onMutated} />
      )}
      {editing && (
        <RuleFormModal mode="edit" rule={editing} locations={locations} onClose={() => setEditing(null)} onSaved={onMutated} />
      )}
      {deleting && <DeleteRuleModal rule={deleting} onClose={() => setDeleting(null)} onDeleted={onMutated} />}
    </main>
  )
}

interface RuleFormProps {
  mode: 'create' | 'edit'
  rule?: AlertRule
  locations: TrackedLocation[]
  onClose: () => void
  onSaved: () => void
}

function RuleFormModal({ mode, rule, locations, onClose, onSaved }: RuleFormProps) {
  const [locationId, setLocationId] = useState<number | ''>(rule?.tracked_location_id ?? locations[0]?.id ?? '')
  const [parameter, setParameter] = useState<Parameter>((rule?.parameter as Parameter) ?? 'pm25')
  const [comparator, setComparator] = useState<Comparator>((rule?.comparator as Comparator) ?? '>')
  const [threshold, setThreshold] = useState<string>(rule ? String(rule.threshold_value) : '')
  const [severity, setSeverity] = useState<Severity>((rule?.severity as Severity) ?? 'warning')
  const [statusValue, setStatusValue] = useState<RuleStatus>((rule?.status as RuleStatus) ?? 'active')
  const [name, setName] = useState(rule?.name ?? '')
  const [submitting, setSubmitting] = useState(false)
  const [formError, setFormError] = useState<string | null>(null)

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault()
    setFormError(null)

    const thr = Number(threshold)
    if (threshold.trim() === '' || !Number.isFinite(thr)) {
      setFormError('Le seuil est obligatoire (un nombre).')
      return
    }
    if (thr < 0) {
      setFormError('Le seuil doit être positif ou nul.')
      return
    }

    setSubmitting(true)
    try {
      if (mode === 'create') {
        if (locationId === '') {
          setFormError('Choisissez un lieu suivi.')
          setSubmitting(false)
          return
        }
        const input: CreateAlertRuleInput = {
          tracked_location_id: locationId,
          parameter,
          comparator,
          threshold_value: thr,
          severity,
          name: name.trim() || undefined,
        }
        await api.alertRules.create(input)
      } else if (rule) {
        // `tracked_location_id` et `parameter` sont IMMUABLES (absents du PATCH).
        // `name: ''` efface le libellé côté back.
        const input: UpdateAlertRuleInput = {
          comparator,
          threshold_value: thr,
          severity,
          status: statusValue,
          name: name.trim(),
        }
        await api.alertRules.update(rule.id, input)
      }
      onSaved()
    } catch (err) {
      setFormError(apiErrorMessage(err))
    } finally {
      setSubmitting(false)
    }
  }

  const title = mode === 'create' ? "Nouvelle règle d'alerte" : 'Modifier la règle'

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
          <Button type="submit" form="rule-form" loading={submitting}>
            {mode === 'create' ? 'Créer la règle' : 'Enregistrer'}
          </Button>
        </>
      }
    >
      <form id="rule-form" className={styles.form} onSubmit={onSubmit} noValidate>
        {formError && (
          <div role="alert" className={styles.formError}>
            {formError}
          </div>
        )}

        {mode === 'create' ? (
          <SelectField
            label="Lieu suivi"
            value={locationId}
            onChange={(e) => setLocationId(Number(e.target.value))}
            required
            autoFocus
          >
            {locations.map((l) => (
              <option key={l.id} value={l.id}>
                {l.name}
                {l.is_active ? '' : ' (en pause)'}
              </option>
            ))}
          </SelectField>
        ) : (
          <div className={styles.readonlyField}>
            <span className={styles.fieldLabel}>Lieu suivi</span>
            <span className={styles.readonlyValue}>
              {rule?.tracked_location_name} <em>(non modifiable)</em>
            </span>
          </div>
        )}

        {mode === 'create' ? (
          <SelectField label="Polluant" value={parameter} onChange={(e) => setParameter(e.target.value as Parameter)}>
            {PARAMETERS.map((p) => (
              <option key={p} value={p}>
                {paramLabel(p)}
              </option>
            ))}
          </SelectField>
        ) : (
          <div className={styles.readonlyField}>
            <span className={styles.fieldLabel}>Polluant</span>
            <span className={styles.readonlyValue}>
              {paramLabel(rule?.parameter ?? '')} <em>(non modifiable)</em>
            </span>
          </div>
        )}

        <div className={styles.row}>
          <SelectField
            label="Condition"
            value={comparator}
            onChange={(e) => setComparator(e.target.value as Comparator)}
            autoFocus={mode === 'edit'}
          >
            {COMPARATORS.map((c) => (
              <option key={c} value={c}>
                {c}
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
            required
          />
        </div>

        <SelectField label="Sévérité" value={severity} onChange={(e) => setSeverity(e.target.value as Severity)}>
          {SEVERITIES.map((s) => (
            <option key={s} value={s}>
              {SEVERITY_LABELS[s]}
            </option>
          ))}
        </SelectField>

        {mode === 'edit' && (
          <SelectField label="Statut" value={statusValue} onChange={(e) => setStatusValue(e.target.value as RuleStatus)}>
            <option value="active">Active</option>
            <option value="inactive">Inactive</option>
          </SelectField>
        )}

        <Input
          label="Nom (optionnel)"
          value={name}
          onChange={(e) => setName(e.target.value)}
          maxLength={200}
          placeholder="Ex. Pic de PM2.5 - école"
        />
      </form>
    </Modal>
  )
}

function DeleteRuleModal({
  rule,
  onClose,
  onDeleted,
}: {
  rule: AlertRule
  onClose: () => void
  onDeleted: () => void
}) {
  const [submitting, setSubmitting] = useState(false)
  const [err, setErr] = useState<string | null>(null)

  const onConfirm = async () => {
    setSubmitting(true)
    setErr(null)
    try {
      await api.alertRules.remove(rule.id)
      onDeleted()
    } catch (e) {
      setErr(apiErrorMessage(e))
      setSubmitting(false)
    }
  }

  return (
    <Modal
      open
      title="Supprimer la règle"
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
        Supprimer définitivement la règle <strong>{ruleLabel(rule)}</strong> (
        <span className={styles.mono}>
          {paramLabel(rule.parameter)} {rule.comparator} {rule.threshold_value}
        </span>
        ) ? Les alertes déjà déclenchées sont conservées. Cette action est irréversible.
      </p>
    </Modal>
  )
}
