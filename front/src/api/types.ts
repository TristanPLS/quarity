// --- Auth ---
export interface TokenResponse {
  access_token: string
  refresh_token: string
  token_type: string
  expires_in: number
}

export interface Me {
  user_id: number
  email: string
  full_name: string | null
  org_id: number
  role: string
  can_write: boolean
}

// --- Mesures ---
export const PARAMETERS = ['pm25', 'pm10', 'no2', 'o3', 'so2', 'co'] as const
export type Parameter = (typeof PARAMETERS)[number]

/// Libellés d'affichage des polluants (avec indices typographiques).
export const PARAM_LABELS: Record<string, string> = {
  pm25: 'PM2.5',
  pm10: 'PM10',
  no2: 'NO₂',
  o3: 'O₃',
  so2: 'SO₂',
  co: 'CO',
}
export const paramLabel = (p: string): string => PARAM_LABELS[p] ?? p.toUpperCase()

export interface Measurement {
  location_id: number
  sensor_id: number
  parameter: string
  unit: string
  measured_at: string // 'YYYY-MM-DD HH:MM:SS.mmm' (UTC)
  value: number
}

export interface MeasurementsResponse {
  page: number
  page_size: number
  count: number
  data: Measurement[]
}

export interface MeasurementsQuery {
  location_id: number
  parameter: Parameter
  from: string
  to: string
  page?: number
  page_size?: number
}

// --- AQI (vue d'ensemble — B10) ---
export interface AqiPollutant {
  parameter: string
  aqi: number
  conc_value: number
  conc_unit: string
  is_valid: boolean
  is_dominant: boolean
}

export interface LocationAqi {
  tracked_location_id: number
  name: string
  latitude: number | null
  longitude: number | null
  /** AQI global = MAX des AQI par polluant/station. `null` si aucune donnée récente. */
  overall_aqi: number | null
  dominant_parameter: string | null
  /** Validité réglementaire du polluant dominant (couverture ≥ 75 %). */
  valid: boolean
  /** `false` = aucune mesure récente (≠ AQI bas). */
  has_data: boolean
  pollutants: AqiPollutant[]
}

export interface AqiOverview {
  computed_at: string
  data: LocationAqi[]
}

// --- Alertes temps réel (B8/B8b → consommées en B11a via WebSocket /api/ws) ---
/** Événement d'alerte poussé sur la WebSocket (miroir de `InsertedEvent` côté back). */
export interface AlertEvent {
  id: number
  alert_rule_id: number
  org_id: number
  tracked_location_id: number
  openaq_location_id: number
  openaq_sensor_id: number
  parameter_code: string
  measured_value: number
  unit: string
  measured_at: string
  threshold_value: number
  comparator: string
  severity: string
  fired_at: string
}

/** Enveloppe des messages WebSocket (`type` discrimine d'éventuels futurs types). */
export interface AlertMessage {
  type: 'alert'
  event: AlertEvent
}

// --- Listing CRUD (pagination/tri/filtre — B6, consommé en B11b) ---
/** Page renvoyée par les listings CRUD : `{ page, page_size, count, total, data }`. */
export interface Page<T> {
  page: number
  page_size: number
  /** Taille de `data` (cette page). */
  count: number
  /** Total filtré, toutes pages confondues. */
  total: number
  data: T[]
}

/**
 * Paramètres communs des listings CRUD (B6).
 * `sort` : attribut d'allowlist, préfixe `-` = descendant (ex. `-created_at`).
 */
export interface ListQuery {
  q?: string
  sort?: string
  page?: number
  page_size?: number
}

// --- Lieux suivis (CRUD — B6, consommé en B11b) ---
export interface TrackedLocation {
  id: number
  org_id: number
  name: string
  description: string | null
  /** `false` = surveillance en pause (le lieu reste listé, ses règles dormantes). */
  is_active: boolean
  /** Nombre de stations OpenAQ liées (≥ 1 par construction). */
  station_count: number
  /** Nombre de règles d'alerte ACTIVES adossées au lieu. */
  active_rule_count: number
  created_at: string
  updated_at: string
}

/** Station OpenAQ liée à un lieu suivi (détail uniquement). */
export interface TrackedStation {
  openaq_location_id: number
  name: string
  city: string | null
  /** ISO-3166-1 alpha-2. */
  country: string
  /** Station de référence du lieu (au plus une). */
  is_primary: boolean
}

/** Détail d'un lieu : le DTO (aplati côté back) + ses stations triées (primaire d'abord). */
export type TrackedLocationDetail = TrackedLocation & {
  stations: TrackedStation[]
}

export interface TrackedLocationFilters {
  is_active?: boolean
}

export interface CreateTrackedLocationInput {
  name: string
  description?: string
  /** Clés naturelles OpenAQ, 1 à 50 — la PREMIÈRE devient station primaire. */
  openaq_location_ids: number[]
  /** Règles créées atomiquement avec le lieu (50 max). */
  rules?: InlineRuleInput[]
}

/** Règle d'alerte créée en même temps que le lieu (`POST /tracked-locations`). */
export interface InlineRuleInput {
  parameter: Parameter
  comparator: Comparator
  threshold_value: number
  severity?: Severity
  name?: string
}

/** PATCH partiel : champs absents = inchangés ; `description: ''` efface la description. */
export interface UpdateTrackedLocationInput {
  name?: string
  description?: string
  is_active?: boolean
}

// --- Règles d'alerte (CRUD — B6, consommé en B11b) ---
export const COMPARATORS = ['>', '>='] as const
export type Comparator = (typeof COMPARATORS)[number]

export const SEVERITIES = ['info', 'warning', 'critical'] as const
export type Severity = (typeof SEVERITIES)[number]

export const RULE_STATUSES = ['active', 'inactive'] as const
export type RuleStatus = (typeof RULE_STATUSES)[number]

/** Libellés d'affichage des sévérités. */
export const SEVERITY_LABELS: Record<Severity, string> = {
  info: 'Info',
  warning: 'Avertissement',
  critical: 'Critique',
}

export interface AlertRule {
  id: number
  org_id: number
  /** Lieu porteur (IMMUABLE après création). */
  tracked_location_id: number
  tracked_location_name: string
  /** Polluant surveillé (IMMUABLE après création). */
  parameter: string
  comparator: string
  threshold_value: number
  severity: string
  /** `active` / `inactive` — seules les règles actives déclenchent des alertes. */
  status: string
  name: string | null
  created_at: string
  updated_at: string
}

export interface AlertRuleFilters {
  tracked_location_id?: number
  status?: RuleStatus
  severity?: Severity
  parameter?: Parameter
}

export interface CreateAlertRuleInput {
  /** Lieu porteur — doit appartenir à l'org du JWT (404 sinon). */
  tracked_location_id: number
  parameter: Parameter
  comparator: Comparator
  threshold_value: number
  /** Défaut back : `warning`. */
  severity?: Severity
  name?: string
}

/** PATCH partiel — `tracked_location_id` et `parameter` sont IMMUABLES (absents). */
export interface UpdateAlertRuleInput {
  comparator?: Comparator
  threshold_value?: number
  severity?: Severity
  status?: RuleStatus
  /** `''` efface le libellé. */
  name?: string
}

/** Fenêtre du force-check (`POST /alert-rules/{id}/run`) — heures, 1..=168, défaut 24. */
export interface RunQuery {
  lookback_hours?: number
}

/** Bilan d'un force-check (B7). Un re-run renvoie `events_created: 0` (idempotence). */
export interface RunOutcome {
  rule_id: number
  evaluated: number
  breaches: number
  events_created: number
}

// --- Profils d'exposition (CRUD — B9a, consommé en B11c) ---
export const EXPOSURE_CODES = ['enfants', 'asthmatiques', 'personnes_agees', 'sportifs', 'general'] as const
export type ExposureCode = (typeof EXPOSURE_CODES)[number]

export const EXPOSURE_CODE_LABELS: Record<ExposureCode, string> = {
  enfants: 'Enfants',
  asthmatiques: 'Asthmatiques',
  personnes_agees: 'Personnes âgées',
  sportifs: 'Sportifs',
  general: 'Population générale',
}
export const exposureCodeLabel = (c: string): string => EXPOSURE_CODE_LABELS[c as ExposureCode] ?? c

export const AVERAGING_PERIODS = ['1h', '8h', '24h', 'annual'] as const
export type AveragingPeriod = (typeof AVERAGING_PERIODS)[number]

export const AVERAGING_PERIOD_LABELS: Record<AveragingPeriod, string> = {
  '1h': '1 h',
  '8h': '8 h',
  '24h': '24 h',
  annual: 'Annuel',
}

export interface ExposureProfile {
  id: number
  /** `null` = profil système partagé (lecture seule pour toutes les orgs). */
  org_id: number | null
  code: string
  name: string
  description: string | null
  is_system: boolean
  created_at: string
  updated_at: string
}

/** Seuil adapté d'un profil (un par polluant × période de moyennage). */
export interface ExposureThreshold {
  id: number
  exposure_profile_id: number
  parameter: string
  /** Unité dérivée du référentiel (immuable, jamais saisie). */
  unit: string
  threshold_value: number
  averaging_period: string
  created_at: string
}

export interface ExposureProfileFilters {
  scope?: 'system' | 'custom'
  code?: ExposureCode
}

export interface CreateExposureProfileInput {
  code: ExposureCode
  name: string
  description?: string
}

/** PATCH partiel — `code` IMMUABLE (absent) ; `description: ''` efface. */
export interface UpdateExposureProfileInput {
  name?: string
  description?: string
}

export interface CreateThresholdInput {
  parameter: Parameter
  threshold_value: number
  /** Défaut back : `1h`. */
  averaging_period?: AveragingPeriod
}

// --- Erreur API normalisée ---
export class ApiError extends Error {
  constructor(
    public status: number,
    public body: unknown,
    message?: string,
  ) {
    super(message ?? `Erreur API ${status}`)
    this.name = 'ApiError'
  }
}

/** Corps d'erreur normalisé du back : `{ error: <code stable>, message: <humain> }`. */
export interface ApiErrorBody {
  error: string
  message: string
}

export function isApiError(e: unknown): e is ApiError {
  return e instanceof ApiError
}

function apiErrorBody(e: ApiError): ApiErrorBody | null {
  const b = e.body
  if (b && typeof b === 'object' && 'error' in b && 'message' in b) return b as ApiErrorBody
  return null
}

/** Code d'erreur stable du back (`conflict`, `read_only_role`, `unprocessable_entity`, …) ou `null`. */
export function apiErrorCode(e: unknown): string | null {
  return isApiError(e) ? (apiErrorBody(e)?.error ?? null) : null
}

/**
 * Message lisible pour l'UX. Pour 401/403/404 le back renvoie un code technique
 * comme message → on substitue un libellé FR ; pour 400/409/422/429 on privilégie
 * le message FR déjà fourni par le back (validateurs, conflits). Réseau → générique.
 */
export function apiErrorMessage(e: unknown): string {
  if (!isApiError(e)) return 'Erreur réseau. Vérifiez votre connexion.'
  switch (e.status) {
    case 401:
      return 'Session expirée. Reconnectez-vous.'
    case 403:
      return 'Action non autorisée : votre compte est en lecture seule.'
    case 404:
      return 'Ressource introuvable.'
  }
  const message = apiErrorBody(e)?.message
  if (message) return message
  switch (e.status) {
    case 409:
      return 'Conflit : cette ressource existe déjà.'
    case 422:
      return 'Données invalides.'
    case 429:
      return 'Trop de requêtes. Réessayez dans un instant.'
    default:
      return e.status >= 500 ? 'Erreur serveur. Réessayez plus tard.' : 'Requête invalide.'
  }
}
