# Log agent — resync roadmap + README (axe back Jalon 3)

| | |
|---|---|
| **Date** | 2026-06-10 |
| **Rôle** | agent-doc |
| **Sujet** | Resync de la documentation produit sur la réalité du code (B6/B7 livrés) + versionnement des logs untracked |
| **Déclencheur** | Audit complet du 2026-06-10 (P0-2, P0-4) + analyse d'ouverture de session : la roadmap sous-vend l'axe back, le README annonce Moka « pas encore branché », 2 logs du 06-07 jamais versionnés |
| **Charte** | Zéro action git par l'agent — fichiers édités par l'agent, séquence git déléguée à un script `.cmd` exécuté par l'humain |

## Contexte (état vérifié sur `origin/dev` après fetch)

- **B6** mergé : PR #34 (squash `b962b6e`, identité propre `Tristan P.`).
- **B7** mergé : PR #35 (squash `70a3bb0`, identité propre `Tristan P.`), le 2026-06-10 à 02h07.
- La branche `feature/back-b7-matching-loop` (commit `b98bf26`, identité parasite `M-Tristan-art`) est désormais un **vestige mergé** : son contenu est dans `dev` via le squash propre. L'amend/force-push envisagé est donc **sans objet** (ne change rien à ce que `git log origin/dev` montre).
- En revanche, `roadmap.md` et `README.md` sur `origin/dev` n'avaient **pas** été resync ; les 2 logs du 06-07 étaient toujours untracked.

## Actions (éditions de fichiers — aucune commande git lancée par l'agent)

1. **`roadmap.md`** — axe back Jalon 3 :
   - CRUD 4 ressources → `[X]` (B6, PR #34, 20 endpoints, 55 tests e2e).
   - Auth login/refresh/logout/me → `[X]` ; `register` explicitement écarté (choix B2B/B2G : provisioning par admin via `POST /api/users`).
   - Transverses → laissé `[ ]` mais annoté : `measurements/timeseries` (J2) et `alert-rules/{id}/run` (B7) livrés ; `locations`/`aqi`/`rankings`/`exposure/compute` restants.
   - Codes HTTP, pagination/tri/filtre, OpenAPI → `[X]` (B6).
   - Moka → `[X]` (B7, PR #35, cache L1 + idempotence 0006).
   - Redis pub/sub WebSocket → laissé `[ ]` (B8 restant).
   - Rate-limit Redis → `[X]` (login email+IP ; quota clé API à étendre en B9).
   - Sécurité : Argon2id, validator, paramètres préparés, CORS strict, headers tower-http, pas de secret → `[X]` (vérifiés par l'audit).
2. **`README.md:15`** — ligne Moka : « prévu Jalon 3 — B7, pas encore branché » → « livré le 2026-06-10 — B7, PR #35 ».
3. **Logs** : ajout des 2 fichiers du 06-07 restés untracked (`reference-projet`, `analyse-organisation`) au prochain commit.

## Vérifications

- Cases cochées uniquement pour des items **vérifiés livrés** par l'audit du 2026-06-10 (0 faille critique, claims exacts). Les items partiels (transverses, pub/sub, quota clé API) restent `[ ]` ou annotés, pas de sur-vente.
- `roadmap.md` et `README.md` étaient identiques entre `origin/dev` et la branche feature (vérifié `git diff origin/dev b98bf26 -- roadmap.md README.md` = vide) : les éditions s'appliqueront sans conflit sur une branche partant de `origin/dev`.

## Commande git à lancer par l'humain

- `fix-identite-git.cmd` — corrige l'identité git (locale au dépôt ; note sur le global parasite).
- `docs-resync-roadmap-0610.cmd` — branche `docs/resync-roadmap-readme-0610` depuis `origin/dev`, commit (roadmap + README + 3 logs), push, PR vers `dev`.

## Reste ouvert (hors périmètre de ce log)

- **P0-5 captures** : action manuelle (Swagger 20 endpoints B6, listing paginé, 404 cross-tenant) — non scriptable par l'agent.
- **P0-3 global** : la config parasite `M-Tristan-art` vit dans le gitconfig **global** (`C:\Users\X-TREM INFO\.gitconfig`) — décision humaine de corriger le global ou non.
