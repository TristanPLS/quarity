# B10b — Carte des lieux suivis (Leaflet + OpenStreetMap)

**Date** : 2026-06-11
**Axe** : front
**Branche cible** : `feature/front-b10b-carte-aqi` → `dev` (CI verte, squash merge)
**Part de** : `origin/dev` @ `8d34dd4` (B9b-2, PR #46)

2ᵉ feature FRONT (après B10 jauges). **Décision Tristan** : **Leaflet + tuiles OpenStreetMap**
(vraie carte géographique) plutôt qu'un fond tile-less / SVG maison.

## Livré

- **`front/src/features/aqi/AqiMap.tsx`** (`react-leaflet`) : `MapContainer` + `TileLayer` OSM +
  un **`CircleMarker` par lieu**, positionné par les `latitude`/`longitude` exposés par `/api/aqi`,
  **coloré par niveau AQI** (couleurs US EPA alignées sur les tokens `--aqi-1..6` des badges ; gris
  `#9AA0A6` si pas de donnée). **Popup** : nom + AQI + libellé de niveau + polluant dominant.
  `fitBounds` automatique (`maxZoom 13`, padding), `scrollWheelZoom` off. Lieux **sans coordonnées
  omis** de la carte (ils restent dans la grille de jauges).
- **Marqueurs co-localisés décalés** : deux lieux partageant une station (ou au même point) — ex.
  seed Centre-ville + École Jules-Ferry partagent la station 1001 — se posaient EXACTEMENT au même
  endroit, l'un cachant l'autre. Ils sont maintenant **espacés en petit cercle déterministe** (~100 m)
  pour rester tous visibles (popup inchangée). *(Détecté au smoke-test du 2026-06-11.)*
- **Réutilise le fetch B10** : branchée dans `AqiOverviewSection` au-dessus de la grille — **aucun
  second appel réseau** (mêmes `locations` de `useAqiOverview`).
- **CSP** : `img-src` étendue aux **tuiles OSM** (`https://*.tile.openstreetmap.org` + `data:` pour
  les icônes Leaflet) dans **les 2 blocs CSP** de `front/nginx.conf` (serveur + `/assets/`).
- Deps : `leaflet`, `react-leaflet`, `@types/leaflet` + import `leaflet/dist/leaflet.css`.

## Fichiers

- `front/src/features/aqi/AqiMap.tsx` (NOUVEAU) + `AqiMap.module.css` (NOUVEAU).
- `front/src/features/aqi/AqiOverviewSection.tsx` — rend `<AqiMap …>` (succès + lieux).
- `front/nginx.conf` — CSP `img-src` (×2) + commentaire.
- `front/package.json` (+ `package-lock.json`) — leaflet/react-leaflet/@types.

## Vérifications

- `npm run typecheck` (tsc --noEmit) ✅ · `npm run build` (vite) ✅ — **en local**.
- ⚠ Bundle `index.js` = **753 Ko** (> 500 Ko) à cause de Leaflet → avertissement vite (non bloquant) ;
  **reliquat** : code-split (`import()` dynamique de la carte / `manualChunks`).
- ⚠ **À smoke-tester au runtime** par Tristan : `docker compose up -d front` puis ouvrir le dashboard
  avec des lieux ayant des coords (ex. après reseed/ingestion) — vérifier que les tuiles OSM
  s'affichent (CSP) et que les marqueurs sont colorés. La CI ne fait que typecheck + build, pas de rendu.
- 0 test front (framework de test absent — introduit en B11 si retenu).

## Bloc git (exécuté par Tristan — charte)

Script : `quarity/commit-front-b10b-carte-aqi.cmd`
- `git fetch origin` ; `git switch -c feature/front-b10b-carte-aqi --no-track origin/dev`
- `git add` (liste explicite) ; commit ASCII ; `git push -u` ; `gh pr create --base dev`
