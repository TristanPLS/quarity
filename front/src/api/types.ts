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
