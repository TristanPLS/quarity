# Log — agent-front — 2026-06-15 — cosmetic + dead-code (Vague 0 : 1a+1b+1c)

## CEST — Cosmétique front / code mort (plan-finition-v1, cluster 1)

- **Agent / rôle** : agent-front
- **Jalon / tâche** : finition v1.0 — Vague 0, PR `chore/front-cosmetic-deadcode` (regroupe 1a + 1b + 1c).
- **Contexte** : 3 nettoyages low-risk identifiés par l'analyse multi-agents du 06-14.
  - **1a** : `ui.tsx` exportait à la fois des composants ET le helper `aqiLevelFromValue` (fonction) → unique warning `react-refresh/only-export-components` restant (la config a `allowConstantExport: true`, donc `AQI_SCALE` const ne déclenchait PAS ; seule la fonction le faisait). C'était le résidu annoncé dans le log ESLint du 06-13.
  - **1b** : `tokenStore.setAccess` / `setRefresh` = code mort (0 appelant ; remplacés par `setTokens`/`clear`).
  - **1c** : `DashboardPage` redéclarait un `PARAM_LABELS` local, doublon de `paramLabel`/`PARAM_LABELS` de `api/types.ts` (dernier module retardataire).
- **Écart vs plan corrigé** : les chemins du plan étaient périmés — `tokenStore.ts` est sous `src/api/` (pas `src/store/`), et `AqiMap`/`AqiOverviewSection` sous `src/features/aqi/` (pas `src/components/`). Numéros de ligne corrects. Code = source de vérité.
- **Actions** :
  - **1a** : nouveau `front/src/components/aqi.ts` (`type AqiLevel` + `const AQI_SCALE` + `function aqiLevelFromValue`, verbatim). `ui.tsx` importe `AQI_SCALE`/`AqiLevel` depuis `./aqi` pour `AqiBadge` (qui reste dans `ui.tsx`) et N'exporte plus ces symboles → le fichier n'exporte QUE des composants. Re-routage des 3 importateurs : `AqiMap.tsx` (les 3 symboles → `../../components/aqi`), `AqiOverviewSection.tsx` et `ui.test.tsx` (split : `AqiBadge`/`Badge` depuis `ui`, `aqiLevelFromValue` depuis `aqi`).
  - **1b** : suppression de `setAccess`/`setRefresh` dans `api/tokenStore.ts`. `getAccess`/`getRefresh`/`setTokens`/`clear`/`hasSession` conservés.
  - **1c** : `DashboardPage.tsx` importe `paramLabel` depuis `../api/types`, suppression de la const locale `PARAM_LABELS`, remplacement des 4 accès (`PARAM_LABELS[x]` → `paramLabel(x)`). `Parameter`/`PARAMETERS` restent utilisés (l.115/117), pas d'import orphelin.
- **Fichiers touchés** :
  - `front/src/components/aqi.ts` (créé)
  - `front/src/components/ui.tsx` (modifié)
  - `front/src/components/ui.test.tsx` (modifié)
  - `front/src/features/aqi/AqiMap.tsx` (modifié)
  - `front/src/features/aqi/AqiOverviewSection.tsx` (modifié)
  - `front/src/api/tokenStore.ts` (modifié)
  - `front/src/pages/DashboardPage.tsx` (modifié)
- **Résultat** : OK — comportement strictement inchangé (mêmes valeurs, mêmes libellés).
- **Vérifs** : `npm run lint` → **0 warning** (le `react-refresh` résiduel a disparu) · `npm run typecheck` clean · `npm run test:run` → **14 tests verts** · `npm run build` OK (bundle 812 kB, inchangé — le code-split Leaflet est la tâche 4a, à part).
- **Prochaine étape** : reste de la Vague 0 (`chore/front-design-tokens`, `fix/measurements-page-size-validation`, `perf/front-code-split-leaflet`, `perf/exposure-results-index`).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Script : quarity/commit-chore-front-cosmetic-deadcode.cmd
  > Branche cible : chore/front-cosmetic-deadcode (depuis origin/dev)
  > fetch + switch -c, git add des 7 fichiers front + ce log, commit ASCII, push, gh pr create vers dev.
  > Puis : attendre la CI verte (lint + typecheck + build + vitest), squash merge.
  > ─────────────────────────────────────────────
