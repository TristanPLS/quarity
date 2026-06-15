# Log — agent-front — 2026-06-15 — code-split Leaflet (Vague 0 : 4a)

## CEST — Lazy-load de la carte AQI (plan-finition-v1, cluster 4)

- **Agent / rôle** : agent-front
- **Jalon / tâche** : finition v1.0 — Vague 0, PR `perf/front-code-split-leaflet` (4a).
- **Contexte** : `leaflet` + `react-leaflet` + `leaflet.css` (~156 Kio JS + 16 Kio CSS) étaient dans le bundle d'entrée (812 Kio), alors que la carte n'est rendue que sur la vue AQI quand au moins un lieu suivi a des données.
- **Actions** :
  - Nouveau `front/src/features/aqi/AqiMapLazy.tsx` : `lazy(() => import('./AqiMap').then((m) => ({ default: m.AqiMap })))` enveloppé dans `<Suspense>` avec fallback « Chargement de la carte… » (réutilise `stateBox`/`spinner` d'`AqiOverview.module.css`). API identique à `AqiMap` (`{ locations: LocationAqi[] }`).
  - `AqiOverviewSection.tsx` : import `AqiMapLazy` au lieu d'`AqiMap`, usage `<AqiMapLazy locations={locations} />` (unique point de rendu de la carte).
- **Fichiers touchés** :
  - `front/src/features/aqi/AqiMapLazy.tsx` (créé)
  - `front/src/features/aqi/AqiOverviewSection.tsx` (modifié)
- **Résultat (build)** :
  - Bundle d'entrée `index.js` : **812.30 Kio → 655.92 Kio** (−156 Kio ; gzip 235.8 → 189.8).
  - Chunk lazy `AqiMap.js` : **156.00 Kio** (gzip 46) — chargé à la demande.
  - `AqiMap.css` (leaflet.css) : **15.89 Kio** séparé ; `index.css` 64.05 → 48.38 Kio.
- **Vérifs** : `npm run lint` → 0 warning · `npm run typecheck` clean · `npm run test:run` → 14 tests verts · `npm run build` OK. Pattern `React.lazy` standard, props identiques → carte inchangée au rendu (smoke visuel recommandé post-merge si stack up ; non bloquant).
- **Note** : le warning Vite « chunk > 500 Kio » persiste sur l'entrée (655 Kio) — c'est `recharts`, hors périmètre 4a.
- **Prochaine étape Vague 0** : `perf/exposure-results-index` (5d, dernière de la Vague 0).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Script : quarity/commit-perf-front-code-split-leaflet.cmd
  > Branche cible : perf/front-code-split-leaflet (depuis origin/dev)
  > fetch + switch -c, git add AqiMapLazy.tsx + AqiOverviewSection.tsx + ce log, commit ASCII, push, gh pr create vers dev.
  > Puis : attendre la CI verte, squash merge, maj-dev.cmd.
  > ─────────────────────────────────────────────
