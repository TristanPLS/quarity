import { describe, it, expect } from 'vitest'
import {
  ApiError,
  apiErrorCode,
  apiErrorMessage,
  daysMaskLabel,
  exposureCodeLabel,
  isApiError,
  paramLabel,
} from './types'

describe('apiErrorMessage', () => {
  it('substitue un libellé FR pour 403 (le back renvoie un code technique)', () => {
    const e = new ApiError(403, { error: 'read_only_role', message: 'read_only_role' })
    expect(apiErrorMessage(e)).toContain('lecture seule')
  })

  it('privilégie le message FR fourni par le back pour 409', () => {
    const e = new ApiError(409, { error: 'conflict', message: 'ce nom existe déjà' })
    expect(apiErrorMessage(e)).toBe('ce nom existe déjà')
  })

  it('repli générique pour 409 sans corps', () => {
    expect(apiErrorMessage(new ApiError(409, null))).toContain('Conflit')
  })

  it('message réseau pour une erreur hors ApiError', () => {
    expect(apiErrorMessage(new Error('boom'))).toContain('réseau')
  })
})

describe('apiErrorCode', () => {
  it('extrait le code stable du back', () => {
    expect(apiErrorCode(new ApiError(409, { error: 'conflict', message: 'x' }))).toBe('conflict')
  })

  it('null hors ApiError', () => {
    expect(apiErrorCode(new Error('x'))).toBeNull()
  })
})

describe('isApiError', () => {
  it('discrimine les ApiError', () => {
    expect(isApiError(new ApiError(500, null))).toBe(true)
    expect(isApiError(new Error('x'))).toBe(false)
  })
})

describe('daysMaskLabel', () => {
  it('raccourcis usuels', () => {
    expect(daysMaskLabel(127)).toBe('Tous les jours')
    expect(daysMaskLabel(31)).toBe('Lun→Ven')
    expect(daysMaskLabel(96)).toBe('Week-end')
  })

  it('liste les jours pour un masque arbitraire (bit0=Lun, bit2=Mer, bit4=Ven)', () => {
    expect(daysMaskLabel(1 + 4 + 16)).toBe('Lun, Mer, Ven')
  })
})

describe('libellés', () => {
  it('paramLabel connaît les polluants et majuscule le reste', () => {
    expect(paramLabel('pm25')).toBe('PM2.5')
    expect(paramLabel('inconnu')).toBe('INCONNU')
  })

  it('exposureCodeLabel connaît les codes et renvoie le brut sinon', () => {
    expect(exposureCodeLabel('enfants')).toBe('Enfants')
    expect(exposureCodeLabel('autre')).toBe('autre')
  })
})
