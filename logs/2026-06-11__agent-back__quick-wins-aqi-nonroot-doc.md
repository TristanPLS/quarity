# Quick wins / durcissement — 2026-06-11

**Axe** : back + infra + doc
**Branche cible** : `chore/quick-wins-0611` → `dev` (CI verte, squash merge)
**Part de** : `origin/dev` @ `913c451` (B8b, PR #40)

Lot de petites améliorations issues de l'analyse d'ouverture du 2026-06-11.

## 1. `/api/aqi` — parallélisme ClickHouse borné *(ferme le 2ᵉ constat confirmé)*

L'analyse (vérifiée en adversarial) avait confirmé que `GET /api/aqi` lançait `join_all`
sur **toutes** les stations de l'org → un org à 1000 lieux = 1000 requêtes ClickHouse
concurrentes (amplification de coût). Corrigé en `buffer_unordered(16)` :

- `back/src/routes/aqi.rs:146` — `futures_util::future::join_all(...)` →
  `futures_util::stream::iter(...).buffer_unordered(AQI_MAX_CONCURRENT_QUERIES).collect()`.
- Au plus **16** requêtes Q2 en vol ; le reste s'écoule au fil de l'eau. **Sortie identique**
  (l'agrégation §3 indexe par station, l'ordre des résultats n'importe pas) → les e2e B10
  existants (`b10_aqi.rs`) restent valides sans modification.
- Import `future::join_all` → `stream::StreamExt` ; const `AQI_MAX_CONCURRENT_QUERIES = 16`.

## 2. Conteneur back NON-ROOT *(P1 audit)*

- `back/Dockerfile` — ajout d'un uid système dédié sans privilège
  (`useradd --system --no-create-home --uid 10001 quarity` + `USER quarity`) avant `EXPOSE`.
- Sûr : le binaire écoute sur **8080** (port non privilégié), ne sert que des **migrations
  embarquées** (résolues à la compilation), n'écrit sur aucun volume ; idem `quarity-ingest`.
  `curl` (healthcheck) reste disponible. Validé par le job **Docker** de la CI (build de l'image).

## 3. Resync doc *(dérives repérées à l'analyse)*

- `README.md` — (a) le claim « sub-5 ms » de la boucle de matching est désormais qualifié
  « *par construction* — benchmark formel → C2, Jalon 4 » ; (b) le cadrage « Le walking
  skeleton (Jalon 2) est livré » (qui sous-vendait l'état réel) devient « **Jalon 3 largement
  avancé** » avec la liste B6/B7/B8(b)/B10 + renvoi backlog/roadmap.
- `roadmap.md` — même caveat « par construction ; à prouver en C2 » sur les deux mentions
  « sous 5 ms » (§ Pourquoi Moka et Redis + hot path).

## Reliquats NON pris dans ce lot

- **Conteneur FRONT non-root** : nécessite le passage à `nginx-unprivileged` + `listen 8080`
  + remap compose `3000:8080`, **non validable par la CI seule** (pas de healthcheck front ;
  à smoke-tester au runtime). Laissé à un lot dédié (vérification `docker compose up front`).
- Test e2e d'**échec partiel ClickHouse** sur `/api/aqi` (gap de couverture noté à l'analyse) —
  séparé (simulation d'erreur CH fiddly).

## Vérifications

- `cargo fmt --all` ✅ · `cargo clippy --all-targets -- -D warnings` ✅ ·
  `cargo build --release` ✅ (mirror du job Docker).
- Pas de nouveau test (changement de concurrence à sortie identique ; e2e B10 inchangés,
  délégués à la CI).

## Bloc git (exécuté par Tristan — charte)

Script : `quarity/commit-quick-wins-0611.cmd`
- `git fetch origin` ; `git switch -c chore/quick-wins-0611 --no-track origin/dev`
- `git add back/src/routes/aqi.rs back/Dockerfile README.md roadmap.md logs/2026-06-11__agent-back__quick-wins-aqi-nonroot-doc.md`
- commit ASCII ; `git push -u` ; `gh pr create --base dev`
