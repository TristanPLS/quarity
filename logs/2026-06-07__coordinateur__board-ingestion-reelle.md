# Log — coordinateur — 2026-06-07 — board-ingestion-reelle

## 17:25 CEST — Régénération clé OpenAQ + ré-ingestion réelle + remplissage du board + captures

- **Agent / rôle** : coordinateur
- **Jalon / tâche** : solde des restes d'A1 (capture board) et de la dette D3 (secrets) ; le board GitHub Projects, créé à la clôture du Jalon 0, n'avait jamais été peuplé.
- **Contexte** : les volumes Docker ayant été recréés ce jour, les 287 mesures historiques avaient disparu ; la clé OpenAQ (exposée en session, dette D3) restait à régénérer.
- **Actions** :
  - **Clé OpenAQ** : régénérée par l'humain (l'ancienne révoquée), placée dans `.env` (jamais committé).
  - **Ré-ingestion réelle** : `docker compose --profile ingest run --rm ingest` → station #4085 NICE PROMENADE, **231 mesures** (103 NO₂, 64 PM2.5, 64 PM10), 0 ignorée sans unité, station liée à `agglo-riviera`. Vérifié servi par l'API (64 points PM2.5, isolation multi-tenant OK).
  - **Capture données réelles** : `docs/captures/2026-06-07__app-dashboard-nice-4085.png` (navigateur piloté, requête station 4085 — 64 points, min 2.3 / moy 10.1 / max 17.3 µg/m³, 0 erreur console).
  - **Board GitHub Projects** : script `../remplir-board.ps1` (+ wrapper .cmd, hors dépôt) préparé par l'agent et **exécuté par l'humain** (écritures `gh` — charte respectée) : **28 items** créés depuis `docs/backlog.md` et rangés par statut — 5 Terminé (A1, A3, A4, A7, migrations sqlx), 6 À faire (A2, A5, A6, D1–D3), 17 Backlog (B1–B13, C1–C4). Répartition contre-vérifiée via `gh project item-list` (lecture seule).
  - **Capture board** : `docs/captures/2026-06-07__board-github.png` (prise par l'humain, relue — 6 colonnes visibles).
  - `docs/backlog.md` : D3 marquée **soldée** ; ligne A1 complétée (captures réelles + board) ; ligne Jalon 0 annotée (board rempli le 07/06).
- **Fichiers touchés** :
  - `docs/captures/2026-06-07__app-dashboard-nice-4085.png` (créé)
  - `docs/captures/2026-06-07__board-github.png` (créé — par l'humain)
  - `docs/backlog.md` (modifié — 3 retouches)
  - `logs/2026-06-07__coordinateur__board-ingestion-reelle.md` (créé — ce log)
  - `../remplir-board.ps1` + `../remplir-board.cmd` (créés — hors dépôt, jetables, supprimés par le script de commit)
- **Résultat** : OK — D3 entièrement soldée, board vivant (28 items), données réelles de bout en bout restaurées.
- **Vérifs** : ingestion relue (sortie complète, 0 skip) ; API interrogée avec le compte démo (count=64, valeurs cohérentes) ; board compté par statut via gh en lecture seule (6/17/5 = attendu) ; les 2 captures relues visuellement.
- **Prochaine étape** : A2 (OpenAPI/utoipa + capture Swagger), puis A5/A6 ; envisager la purge des ~14 branches déjà squash-mergées (local + origin).
- **Action Git suggérée à l'humain** :
  > Exécuter le script `..\commit-board-captures.cmd` (branche `docs/captures-board-donnees-reelles` + commit + push + PR), puis merger à la CI verte et relancer `..\maj-dev.cmd`.
