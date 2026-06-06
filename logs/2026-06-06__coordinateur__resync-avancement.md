# Log — coordinateur — 2026-06-06 — resync-avancement

## 23:50 CEST — Resynchronisation de la doc d'avancement avec l'état réel du code

- **Agent / rôle** : coordinateur
- **Jalon / tâche** : transverse — resynchronisation backlog/roadmap/README/pitch (recommandation n°1 de la revue du 2026-06-06)
- **Contexte** : la revue du 2026-06-06 a constaté des écarts entre la doc d'avancement et l'état réel du code (Jalon 0 clos, A3/A7 livrés le 05/06, D1/D5 corrigées, items Jalons 1-2 de la roadmap jamais cochés, Swagger annoncé mais non implémenté).
- **Actions** :
  - `docs/backlog.md` : date de mise à jour passée au 2026-06-06 ; clôture du Jalon 0 actée (board, protections master/dev, templates PR/issue, process solo) ; précision tests e2e (9/9 vérifs manuelles 04/06 → 12 tests versionnés + CI 05/06) ; note « API v3 uniquement — backfill S3 restant (A6) » sur l'ingestion ; dettes D1 et D5 marquées corrigées (05/06) ; missions A3 et A7 cochées avec liens vers les logs de livraison.
  - `README.md` : « Une fois le walking skeleton livré » → « est livré » ; lien Swagger remplacé par « à venir (backlog A2 — non implémentée à ce jour) » ; mention Moka annotée « prévu Jalon 3 — B7, pas encore branché ».
  - `roadmap.md` : encart « ce document est le plan, l'avancement réel fait foi dans docs/backlog.md » ajouté sous l'en-tête ; Jalon 1 : 8 items Personas/Postgres + 7 items ClickHouse cochés (wireframes laissés non faits) ; Jalon 2 : 7 items cochés, ingestion annotée « API v3 ✅ livré ; backfill S3 restant → A6 », doc API annotée « différée → A2 », test clonage laissé tel quel ; paramètre d'endpoint aligné sur le code (`location_id=…`) ; « Pas de duplication » → « Dénormalisation contrôlée (cf. docs/data-model.md §D.2) ».
  - `docs/pitch.md` : « Pas de duplication » (§3.1) reformulé en « Dénormalisation contrôlée, aucune FK cross-DB », reste de la phrase conservé.
- **Fichiers touchés** :
  - `docs/backlog.md` (modifié)
  - `README.md` (modifié)
  - `roadmap.md` (modifié)
  - `docs/pitch.md` (modifié)
  - `logs/2026-06-06__coordinateur__resync-avancement.md` (créé — ce log)
- **Résultat** : OK — les 4 documents reflètent l'état réel constaté par la revue du 2026-06-06.
- **Vérifs** : grep `^- \[ \]` sur roadmap.md → seuls restent non cochés (Jalons 1-2) : wireframes, ingestion (backfill S3), doc API, test clonage — conforme ; grep `Pas de duplication|location=…` sur `*.md` → 0 occurrence ; les logs référencés (`2026-06-05__agent-back__cors-headers.md`, `2026-06-05__agent-back__cross-tenant-tests-ci.md`) existent bien dans `logs/`. Aucune commande cargo nécessaire (docs uniquement).
- **Prochaine étape** : reprendre le durcissement court terme du backlog — A1 (vérif visuelle front + captures, inclut le suivi CSP nginx) et A2 (doc OpenAPI utoipa).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Branche cible : docs/resync-avancement     (jamais master/dev en direct)
  > 1) git add docs/backlog.md README.md roadmap.md docs/pitch.md logs/2026-06-06__coordinateur__resync-avancement.md
  > 2) git commit -m "docs(doc): resynchronisation avancement (backlog, roadmap, README, pitch)"
  > 3) git push -u origin docs/resync-avancement
  > Puis : ouvrir une PR vers `dev`, attendre la CI verte, squash merge.
  > ─────────────────────────────────────────────
