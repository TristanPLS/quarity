import { useEffect, useId, useRef, type MouseEvent as ReactMouseEvent, type ReactNode } from 'react'
import styles from './Modal.module.css'

interface ModalProps {
  open: boolean
  title: string
  /** Appelé sur Échap, clic backdrop ou bouton fermer. */
  onClose: () => void
  children: ReactNode
  /** Pied de modale (boutons d'action). */
  footer?: ReactNode
}

/**
 * Modale accessible bâtie sur `<dialog>` natif : le navigateur gère le focus-trap,
 * la touche Échap et le backdrop. `open` pilote `showModal()`/`close()` ; en pratique
 * la modale est aussi montée/démontée par le parent (état de formulaire frais à chaque
 * ouverture).
 */
export function Modal({ open, title, onClose, children, footer }: ModalProps) {
  const ref = useRef<HTMLDialogElement>(null)
  const titleId = useId()

  useEffect(() => {
    const dlg = ref.current
    if (!dlg) return
    if (open && !dlg.open) {
      dlg.showModal()
      // `showModal()` place sinon le focus sur le bouton fermer : on vise le 1er champ.
      dlg.querySelector<HTMLElement>('input, select, textarea')?.focus()
    }
    if (!open && dlg.open) dlg.close()
  }, [open])

  // Échap déclenche l'event natif 'cancel' : on le route vers onClose et on empêche
  // la fermeture native pour garder le DOM synchronisé avec l'état React du parent.
  useEffect(() => {
    const dlg = ref.current
    if (!dlg) return
    const onCancel = (e: Event) => {
      e.preventDefault()
      onClose()
    }
    dlg.addEventListener('cancel', onCancel)
    return () => dlg.removeEventListener('cancel', onCancel)
  }, [onClose])

  // Clic sur le backdrop : la cible est le <dialog> lui-même (hors `.inner`).
  const onBackdropClick = (e: ReactMouseEvent<HTMLDialogElement>) => {
    if (e.target === ref.current) onClose()
  }

  return (
    <dialog ref={ref} className={styles.dialog} onClick={onBackdropClick} aria-labelledby={titleId}>
      <div className={styles.inner}>
        <header className={styles.header}>
          <h2 id={titleId} className={styles.title}>
            {title}
          </h2>
          <button type="button" className={styles.close} onClick={onClose} aria-label="Fermer">
            ×
          </button>
        </header>
        <div className={styles.body}>{children}</div>
        {footer && <footer className={styles.footer}>{footer}</footer>}
      </div>
    </dialog>
  )
}
