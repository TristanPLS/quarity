// Échelle US EPA (constantes + helper) — séparée de `ui.tsx` pour que ce dernier
// n'exporte QUE des composants (règle react-refresh/only-export-components : une
// fonction exportée à côté de composants casse le Fast Refresh). `AqiBadge` reste
// dans `ui.tsx` et réimporte d'ici.

/* ===== AQI (échelle EPA — couleur JAMAIS seule) ===== */
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
