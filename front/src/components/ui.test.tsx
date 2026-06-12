import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { AqiBadge, aqiLevelFromValue, Badge } from './ui'

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
