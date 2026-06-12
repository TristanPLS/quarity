import { forwardRef, useId, type ButtonHTMLAttributes, type HTMLAttributes, type InputHTMLAttributes, type ReactNode } from 'react'
import styles from './ui.module.css'

/* ===== Button ===== */
type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger'
interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant
  fullWidth?: boolean
  loading?: boolean
}
export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = 'primary', fullWidth, loading, disabled, children, className = '', ...rest },
  ref,
) {
  const cls = [styles.btn, styles[variant], fullWidth ? styles.full : '', className].filter(Boolean).join(' ')
  return (
    <button ref={ref} className={cls} disabled={disabled || loading} aria-busy={loading || undefined} {...rest}>
      {loading && <span className={styles.spinner} aria-hidden="true" />}
      {children}
    </button>
  )
})

/* ===== Card ===== */
interface CardProps extends Omit<HTMLAttributes<HTMLElement>, 'title'> {
  title?: ReactNode
  actions?: ReactNode
  children: ReactNode
}
export function Card({ title, actions, children, className = '', ...rest }: CardProps) {
  return (
    <section className={`${styles.card} ${className}`} {...rest}>
      {(title || actions) && (
        <header className={styles.cardHeader}>
          {title && <h3 className={styles.cardTitle}>{title}</h3>}
          {actions && <div className={styles.cardActions}>{actions}</div>}
        </header>
      )}
      <div className={styles.cardBody}>{children}</div>
    </section>
  )
}

/* ===== Input ===== */
interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  label: string
  error?: string
  hint?: string
  mono?: boolean
}
export const Input = forwardRef<HTMLInputElement, InputProps>(function Input(
  { label, error, hint, mono, id, className = '', ...rest },
  ref,
) {
  const autoId = useId()
  const inputId = id ?? autoId
  const errId = `${inputId}-err`
  const hintId = `${inputId}-hint`
  return (
    <div className={styles.field}>
      <label htmlFor={inputId} className={styles.label}>{label}</label>
      <input
        ref={ref}
        id={inputId}
        className={[styles.input, mono ? styles.mono : '', error ? styles.invalid : '', className].filter(Boolean).join(' ')}
        aria-invalid={error ? true : undefined}
        aria-describedby={[error ? errId : '', hint ? hintId : ''].filter(Boolean).join(' ') || undefined}
        {...rest}
      />
      {hint && !error && <p id={hintId} className={styles.hint}>{hint}</p>}
      {error && <p id={errId} className={styles.errorText} role="alert">{error}</p>}
    </div>
  )
})

/* ===== Badge (UI générique — jamais un niveau d'air) ===== */
type BadgeTone = 'neutral' | 'info' | 'success' | 'warning' | 'danger'
export function Badge({ tone = 'neutral', dot, children }: { tone?: BadgeTone; dot?: boolean; children: ReactNode }) {
  return (
    <span className={`${styles.badge} ${styles[tone]}`}>
      {dot && <span className={styles.dot} aria-hidden="true" />}
      {children}
    </span>
  )
}

/* ===== AqiBadge (échelle EPA — couleur JAMAIS seule) ===== */
export type AqiLevel = 1 | 2 | 3 | 4 | 5 | 6
export const AQI_SCALE: Record<AqiLevel, { label: string; range: string }> = {
  1: { label: 'Bon', range: '0–50' },
  2: { label: 'Modéré', range: '51–100' },
  3: { label: 'Mauvais pour groupes sensibles', range: '101–150' },
  4: { label: 'Mauvais', range: '151–200' },
  5: { label: 'Très mauvais', range: '201–300' },
  6: { label: 'Dangereux', range: '301+' },
}
export function aqiLevelFromValue(aqi: number): AqiLevel {
  if (aqi <= 50) return 1
  if (aqi <= 100) return 2
  if (aqi <= 150) return 3
  if (aqi <= 200) return 4
  if (aqi <= 300) return 5
  return 6
}
export function AqiBadge({ level, value, unit, size = 'md' }: { level: AqiLevel; value: number | string; unit?: string; size?: 'sm' | 'md' }) {
  const info = AQI_SCALE[level]
  return (
    <span
      className={`${styles.aqi} ${styles[size === 'sm' ? 'aqiSm' : 'aqiMd']} ${styles[`lvl${level}`]}`}
      role="status"
      aria-label={`Qualité de l'air : ${info.label}. Indice ${value}${unit ? ' ' + unit : ''}.`}
    >
      <span className={styles.aqiValue}>
        {value}
        {unit && <span className={styles.aqiUnit}>{unit}</span>}
      </span>
      <span className={styles.aqiLabel}>{info.label}</span>
    </span>
  )
}
