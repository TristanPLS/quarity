import { forwardRef, useId, type ButtonHTMLAttributes, type HTMLAttributes, type InputHTMLAttributes, type ReactNode } from 'react'
import { AQI_SCALE, type AqiLevel } from './aqi'
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

/* ===== États de page partagés (chargement / vide / erreur) ===== */
/** Spinner accessible (`aria-hidden`). `lg` = états de page (18px) ; `sm` = inline. */
export function Spinner({ size = 'lg' }: { size?: 'sm' | 'lg' }) {
  return <span className={size === 'sm' ? styles.spinner : styles.spinnerLg} aria-hidden="true" />
}

/** Encart d'état neutre (chargement / vide). `ariaLive` pour annoncer un chargement. */
export function StateBox({
  children,
  ariaLive,
  className = '',
}: {
  children: ReactNode
  ariaLive?: 'polite' | 'assertive'
  className?: string
}) {
  return (
    <div className={[styles.stateBox, className].filter(Boolean).join(' ')} aria-live={ariaLive}>
      {children}
    </div>
  )
}

/** Encart d'erreur (`role="alert"`) avec titre, message et bouton Réessayer optionnel. */
export function ErrorState({
  title,
  message,
  onRetry,
  className = '',
}: {
  title: string
  message?: ReactNode
  onRetry?: () => void
  className?: string
}) {
  return (
    <div role="alert" className={[styles.alertError, className].filter(Boolean).join(' ')}>
      <strong>{title}</strong>
      {message != null && <span>{message}</span>}
      {onRetry && (
        <button type="button" className={styles.retry} onClick={onRetry}>
          Réessayer
        </button>
      )}
    </div>
  )
}

/* ===== AqiBadge (échelle EPA — couleur JAMAIS seule ; barème dans ./aqi) ===== */
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
