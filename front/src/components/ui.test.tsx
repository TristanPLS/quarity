import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { AqiBadge, Badge, ErrorState, Spinner, StateBox } from './ui'
import { aqiLevelFromValue } from './aqi'

describe('aqiLevelFromValue', () => {
  it('mappe une valeur AQI sur le bon palier US EPA', () => {
    expect(aqiLevelFromValue(0)).toBe(1)
    expect(aqiLevelFromValue(50)).toBe(1)
    expect(aqiLevelFromValue(51)).toBe(2)
    expect(aqiLevelFromValue(150)).toBe(3)
    expect(aqiLevelFromValue(200)).toBe(4)
    expect(aqiLevelFromValue(300)).toBe(5)
    expect(aqiLevelFromValue(500)).toBe(6)
  })
})

describe('Badge', () => {
  it('affiche son contenu', () => {
    render(<Badge tone="success">Actif</Badge>)
    expect(screen.getByText('Actif')).toBeTruthy()
  })
})

describe('AqiBadge', () => {
  it('affiche la valeur et un libellé accessible décrivant la qualité', () => {
    render(<AqiBadge level={1} value={42} />)
    expect(screen.getByText('42')).toBeTruthy()
    const status = screen.getByRole('status')
    expect(status.getAttribute('aria-label')).toContain('Bon')
  })
})

describe('Spinner', () => {
  it('est décoratif (aria-hidden)', () => {
    const { container } = render(<Spinner />)
    expect(container.querySelector('span')?.getAttribute('aria-hidden')).toBe('true')
  })
})

describe('StateBox', () => {
  it('affiche ses enfants et annonce via aria-live quand demandé', () => {
    render(<StateBox ariaLive="polite">Chargement…</StateBox>)
    const box = screen.getByText('Chargement…')
    expect(box.getAttribute('aria-live')).toBe('polite')
  })
})

describe('ErrorState', () => {
  it('affiche titre + message dans un role alert', () => {
    render(<ErrorState title="Oups" message="détail" />)
    const alert = screen.getByRole('alert')
    expect(alert.textContent).toContain('Oups')
    expect(alert.textContent).toContain('détail')
  })

  it('appelle onRetry au clic sur Réessayer', () => {
    let called = 0
    render(<ErrorState title="Oups" onRetry={() => { called += 1 }} />)
    fireEvent.click(screen.getByRole('button', { name: 'Réessayer' }))
    expect(called).toBe(1)
  })

  it('sans onRetry : aucun bouton', () => {
    render(<ErrorState title="Oups" />)
    expect(screen.queryByRole('button')).toBeNull()
  })
})
