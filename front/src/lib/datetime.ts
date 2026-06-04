/**
 * Parse 'YYYY-MM-DD HH:MM:SS.mmm' (sans timezone, supposé UTC) en ms epoch.
 * Robuste cross-navigateur (n'utilise pas Date.parse sur la chaîne brute).
 */
export function parseMeasuredAt(raw: string): number {
  const m = raw.trim().match(/^(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,3}))?/)
  if (!m) return NaN
  const [, y, mo, d, h, mi, s, ms] = m
  return Date.UTC(Number(y), Number(mo) - 1, Number(d), Number(h), Number(mi), Number(s), ms ? Number(ms.padEnd(3, '0')) : 0)
}

/** Format court pour les ticks de l'axe X. */
export function formatTick(ms: number): string {
  return new Intl.DateTimeFormat('fr-FR', {
    day: '2-digit',
    month: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    timeZone: 'UTC',
  }).format(new Date(ms))
}

/** Format complet pour tooltip + tableau. */
export function formatFull(ms: number): string {
  return new Intl.DateTimeFormat('fr-FR', {
    dateStyle: 'short',
    timeStyle: 'medium',
    timeZone: 'UTC',
  }).format(new Date(ms))
}
