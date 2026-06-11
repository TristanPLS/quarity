# B11a — Client WebSocket d'alertes temps réel (front)

**Date** : 2026-06-11
**Axe** : front
**Branche cible** : `feature/front-b11a-client-ws-alertes` → `dev` (CI verte, squash merge)
**Part de** : `origin/dev` @ `47a2b9d` (fix B10b, PR #48)

Première tranche de **B11** (dernière mission). B11 est découpé : **B11a = client WebSocket**
(celle-ci — le payoff temps réel), **B11b = CRUD front** lieux/règles, **B11c = profils
d'exposition front + responsive + 1ʳᵉ infra de test front**.

## Livré

- **`features/alerts/useAlertsSocket.ts`** : hook client WebSocket.
  - Connexion `ws(s)://<host>/api/ws?token=<accessToken>` — **même origine** (nginx proxifie
    `/api/ws` vers le back, Upgrade déjà câblé) ; le jeton d'accès (en MÉMOIRE via `tokenStore`)
    passe en **query string** (une WS navigateur ne peut pas poser d'`Authorization`).
  - Parse `{ type: "alert", event }` (miroir de `InsertedEvent`) → liste des alertes (50 max,
    plus récente en tête). Message illisible ignoré (jamais de crash).
  - **Reconnexion** : backoff exponentiel borné (1 s → 30 s), **jeton relu à CHAQUE tentative**
    (après un refresh HTTP, la WS se rouvre avec le jeton frais — le back ne renouvelle pas le
    jeton en vol, cf. B8). Tout (socket + timer) nettoyé au démontage (`stoppedRef`).
- **`features/alerts/AlertsPanel.tsx`** (+ `.module.css`) : panneau dashboard temps réel —
  indicateur de connexion (point pulsant « En direct » / « Reconnexion… »), liste live des
  dépassements (polluant, valeur + unité, `comparator threshold`, station, heure `fired_at`),
  **code couleur par sévérité** (info/warning/critical). Branché en tête du dashboard.
- **CSP** : `connect-src 'self'` → **`connect-src 'self' ws: wss:`** dans **les 2 blocs** de
  `front/nginx.conf` — **le reliquat B8 (« étendre connect-src pour le WebSocket ») est soldé**.

**C'est le consommateur qui manquait** à toute la chaîne **B7 (matching) → B8 (pub/sub + WS)
→ B8b (durcissement)** : jusqu'ici le push temps réel n'était lu par personne.

## Fichiers

- `front/src/api/types.ts` — types `AlertEvent` + `AlertMessage`.
- `front/src/features/alerts/useAlertsSocket.ts` (NOUVEAU).
- `front/src/features/alerts/AlertsPanel.tsx` + `AlertsPanel.module.css` (NOUVEAUX).
- `front/src/pages/DashboardPage.tsx` — rend `<AlertsPanel />` en tête du `main`.
- `front/nginx.conf` — CSP `connect-src` (×2) + commentaire.

## Vérifications

- `npm run typecheck` (tsc --noEmit) ✅ · `npm run build` (vite) ✅ — **en local**.
- ✅ **Smoke-test runtime fait le 2026-06-12** : dashboard ouvert → indicateur **« En direct »**
  (vert) ; insertion d'une mesure pm25=50 sur la station 1001 + `POST /api/alert-rules/3/run`
  (force-check B7) → l'alerte **est apparue en direct** dans le panneau, **sans rechargement**.
  Capture : `docs/captures/2026-06-12__alerte.png`.
- Retouches visuelles pendant le smoke-test : panneau réécrit avec les **vrais tokens**
  (`styles/tokens.css`) — **console sombre** assumée (`--c-surface-dark`), **titre blanc**,
  **texte clair** (`--c-text-invert`) partout → lisible sur le dashboard clair ; tiret long « — »
  remplacé par le tiret « - ». L'unité s'affiche `µg/m³` (les vraies mesures sont en UTF-8 correct ;
  seul mon `INSERT` de test via le shell Windows avait mangé les octets de tête UTF-8).
- ⚠ Limite assumée de B11a : **pas de backfill** des alertes au chargement (un hard-refresh vide
  le panneau jusqu'au prochain push). Un `GET /api/alert-events` au montage serait un plus → noté pour B11b.
- 0 test front (framework de test toujours absent — prévu en B11c).

## Bloc git (exécuté par Tristan — charte)

Script : `quarity/commit-front-b11a-client-ws-alertes.cmd`
- `git fetch origin` ; `git switch -c feature/front-b11a-client-ws-alertes --no-track origin/dev`
- `git add` (liste explicite) ; commit ASCII ; `git push -u` ; `gh pr create --base dev`
