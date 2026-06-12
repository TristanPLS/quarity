import { NavLink, Outlet } from 'react-router'
import { useAuth } from '../auth/AuthContext'
import { Button } from './ui'
import styles from './AppShell.module.css'

const NAV = [
  { to: '/dashboard', label: 'Tableau de bord' },
  { to: '/locations', label: 'Lieux suivis' },
]

/**
 * Coquille des pages protégées : barre supérieure (marque + navigation + session)
 * commune, contenu de chaque page rendu via `<Outlet/>`. Imbriquée sous
 * `<ProtectedRoute/>` — l'auth est déjà garantie quand ce composant s'affiche.
 */
export function AppShell() {
  const { user, logout } = useAuth()

  return (
    <div className={styles.app}>
      <header className={styles.topbar}>
        <div className={styles.left}>
          <div className={styles.brand}>
            <span className={styles.logoRing} aria-hidden="true" />
            <span className={styles.brandName}>Quarity</span>
          </div>
          <nav className={styles.nav} aria-label="Navigation principale">
            {NAV.map((item) => (
              <NavLink
                key={item.to}
                to={item.to}
                className={({ isActive }) => `${styles.navLink} ${isActive ? styles.navActive : ''}`}
              >
                {item.label}
              </NavLink>
            ))}
          </nav>
        </div>
        <div className={styles.userBox}>
          {user && (
            <span className={styles.user}>
              {user.email} <span className={styles.role}>· {user.role}</span>
            </span>
          )}
          <Button variant="ghost" className={styles.logout} onClick={() => void logout()}>
            Déconnexion
          </Button>
        </div>
      </header>

      <Outlet />
    </div>
  )
}
