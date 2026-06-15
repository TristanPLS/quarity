# Log — agent-front — 2026-06-15 — design tokens (Vague 0 : 2b+2c+2e)

## CEST — Tokeniser bordure erreur + warning sombre + focus-visible (plan-finition-v1, cluster 2)

- **Agent / rôle** : agent-front
- **Jalon / tâche** : finition v1.0 — Vague 0, PR `chore/front-design-tokens` (regroupe 2b + 2c + 2e). À faire AVANT 2a (la classe partagée `.alertError` consommera `--c-danger-border`).
- **Décisions actées (Tristan)** : 2b = `--c-danger-border = #F5C2C7` (iso-rendu, zéro régression visuelle). 2c = nouveau token dédié `--c-warning-on-dark = #F2A23C` (confirmé le 06-15 ; ne PAS substituer par `--c-warning` #9A5B00 = illisible sur sombre).
- **Actions** :
  - **2b** : ajout de `--c-danger-border: #F5C2C7` dans `styles/tokens.css` (à côté de `--c-danger-bg`) + remplacement des **4** occurrences du hex `#F5C2C7` par `var(--c-danger-border)` : `DashboardPage.module.css:45`, `LocationsPage.module.css` (×2 : `.formError` + `.alertError`), `LoginPage.module.css`.
  - **2c** : ajout de `--c-warning-on-dark: #F2A23C` dans `tokens.css` (à côté de `--c-warning-bg`) ; `AlertsPanel.module.css` (`.warning`) consomme le token au lieu du hex `#f39c2e`. ⚠️ **micro-décalage visuel assumé** (#f39c2e → #F2A23C, deux oranges quasi identiques) — harmonisation décidée.
  - **2e** : ajout de `.seg:focus-visible { outline: 2px solid var(--c-accent); outline-offset: -2px; }` à `LocationsPage.module.css` (après `.seg:hover`), à l'identique d'Exposures/Profiles/Rules (LocationsPage était le seul module sans cet état focus visible).
- **Fichiers touchés** :
  - `front/src/styles/tokens.css` (modifié — 2 tokens)
  - `front/src/features/alerts/AlertsPanel.module.css` (modifié — 2c)
  - `front/src/pages/DashboardPage.module.css` (modifié — 2b)
  - `front/src/pages/LoginPage.module.css` (modifié — 2b)
  - `front/src/pages/LocationsPage.module.css` (modifié — 2b ×2 + 2e)
- **Résultat** : OK. Plus aucun hex `#F5C2C7`/`#f39c2e` hors la définition du token (grep). Rendu inchangé sauf le micro-décalage warning assumé.
- **Vérifs** : `npm run lint` → 0 warning · `npm run typecheck` clean · `npm run test:run` → 14 tests verts · `npm run build` OK.
- **Prochaine étape Vague 0** : `fix/measurements-page-size-validation` (3a) · `perf/front-code-split-leaflet` (4a) · `perf/exposure-results-index` (5d). Puis Vague 1 (dont 2a qui réutilisera `--c-danger-border`).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Script : quarity/commit-chore-front-design-tokens.cmd
  > Branche cible : chore/front-design-tokens (depuis origin/dev)
  > fetch + switch -c, git add des 5 fichiers CSS + ce log, commit ASCII, push, gh pr create vers dev.
  > Puis : attendre la CI verte, squash merge, maj-dev.cmd.
  > ─────────────────────────────────────────────
