import { useState, type FormEvent } from 'react'
import { useNavigate, useLocation, Navigate } from 'react-router'
import { useAuth } from '../auth/AuthContext'
import { ApiError } from '../api/types'
import { Button, Input } from '../components/ui'
import styles from './LoginPage.module.css'

interface LocationState {
  from?: { pathname: string }
}

export function LoginPage() {
  const { login, isAuthenticated } = useAuth()
  const navigate = useNavigate()
  const location = useLocation()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)

  const from = (location.state as LocationState | null)?.from?.pathname ?? '/dashboard'
  if (isAuthenticated) return <Navigate to={from} replace />

  async function handleSubmit(e: FormEvent) {
    e.preventDefault()
    setError(null)
    setSubmitting(true)
    try {
      await login(email, password)
      navigate(from, { replace: true })
    } catch (err) {
      setError(err instanceof ApiError && err.status === 401 ? 'Identifiants invalides.' : 'Connexion impossible. Réessayez.')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className={styles.wrap}>
      <aside className={styles.brand}>
        <div className={styles.logoRing} aria-hidden="true" />
        <h1 className={styles.brandName}>Quarity</h1>
        <p className={styles.tagline}>Surveillance et alerte temps réel sur la qualité de l'air mondiale.</p>
      </aside>

      <main className={styles.panel}>
        <form className={styles.card} onSubmit={handleSubmit} noValidate>
          <h2 className={styles.title}>Connexion</h2>
          {error && (
            <div role="alert" className={styles.error}>
              {error}
            </div>
          )}
          <Input
            label="Email"
            type="email"
            autoComplete="username"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            required
            placeholder="vous@organisation.fr"
          />
          <Input
            label="Mot de passe"
            type="password"
            autoComplete="current-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
          />
          <Button type="submit" fullWidth loading={submitting}>
            Se connecter
          </Button>
          <p className={styles.demo}>
            Démo : <code>sophie@agglo-riviera.fr</code> · <code>Quarity2026!</code>
          </p>
        </form>
      </main>
    </div>
  )
}
