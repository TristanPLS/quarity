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
