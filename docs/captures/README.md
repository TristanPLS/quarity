# Captures — preuves de fonctionnement (Livrable L4)

Index des captures du dossier, mappées à la **liste minimale exigée** (sujet, Partie 4 / L4).
Toutes prises sur la stack réelle (`docker compose up -d` + seed + ingestion OpenAQ station 4085).

## Couverture L4

| Exigence du sujet | Capture(s) | État |
|---|---|---|
| **Vue du SGBD SQL** (tables peuplées) | `2026-06-18__sgbd-postgres.png` (`\dt` + comptages) | ✅ |
| **Vue du SGBD NoSQL** (données) | `2026-06-18__sgbd-clickhouse.png` (moteurs + lignes) · `2026-06-18__sgbd-redis.png` (TTL + compteurs) | ✅ |
| **≥ 3 requêtes SQL complexes** exécutées + résultat | `2026-06-18__requetes-sql.png` (CTE+4 JOINs+HAVING+FILTER · vue métier · fonctions fenêtre) | ✅ |
| **≥ 3 requêtes NoSQL** exécutées + résultat | `2026-06-18__requetes-nosql.png` (argMax dedup · AggregatingMergeTree/avgMerge · fenêtre glissante 24 h) | ✅ |
| **Historique Git (graph)** branches + merges | `2026-06-18__git-graph.png` (`git log --graph --all`, 94 commits / 75 branches / 78 PR) | ✅ |
| **Tests d'API** (≥ 6, succès **+ erreurs 4xx**) | `2026-06-18__api-4xx.png` (401·403·404·409·422·429, corps réels) + ci-dessous (200/404 cross-tenant) | ✅ |
| **Vues de l'app** (≥ 4) | login, dashboard, carte, lieux, règles, profils, expositions, mobile, alerte (cf. liste) | ✅ |
| **Doc API Swagger** | `2026-06-07__swagger.png`, `2026-06-10__overview.png` | ✅ |
| **Board Kanban/Scrum** | `2026-06-07__board-github.png` | ✅ |

## Inventaire chronologique

### Bases de données & requêtes (L4 — 2026-06-18)
- `2026-06-18__sgbd-postgres.png` — PostgreSQL 16 : liste des 25 tables (`\dt`) + comptages (peuplement).
- `2026-06-18__sgbd-clickhouse.png` — ClickHouse 24.8 : tables + moteurs (`MergeTree`/`AggregatingMergeTree`/`MaterializedView`) + lignes + mesures par station.
- `2026-06-18__sgbd-redis.png` — Redis 7 : keyspace, familles de clés (refresh-tokens + compteurs anti-brute-force), exemple `GET`/`TTL` (expiration automatique).
- `2026-06-18__requetes-sql.png` — 3 requêtes SQL complexes (CTE + 4 jointures + GROUP BY + HAVING + agrégat `FILTER` ; lecture d'une vue métier ; fonctions fenêtre `RANK`/`SUM OVER PARTITION`).
- `2026-06-18__requetes-nosql.png` — 3 requêtes ClickHouse (`argMax` dédup ; `avgMerge` sur vue matérialisée `AggregatingMergeTree` ; moyenne glissante 24 h réglementaire par fonction fenêtre).
- `2026-06-18__git-graph.png` — graphe Git branches + merges (workflow 1 branche/PR, squash → `dev`).
- `2026-06-18__api-4xx.png` — éventail des codes d'erreur 4xx (401/403/404/409/422/429), réponses réelles du back live.

### Tests d'API & isolation multi-tenant (2026-06-10)
- `2026-06-10__200-cross-tenant.png`, `2026-06-10__404-cross-tenant.png` — isolation : 200 sur sa propre org, 404 (anti-énumération) sur une org étrangère.
- `2026-06-10__get-tracked-locations.png`, `2026-06-10__listing-pagination.png`, `2026-06-10__post-alert-rules.png` — CRUD + pagination.

### Interface & API (2026-06-07 → 06-12)
- `2026-06-07__app-login.png`, `2026-06-07__app-dashboard.png`, `2026-06-07__app-dashboard-nice-4085.png` — login + série temporelle.
- `2026-06-11__map-qualite-de-l-air.png` — carte AQI (Leaflet + OSM).
- `2026-06-12__lieux-suivis.png`, `2026-06-12__regles-alerte.png`, `2026-06-12__profils-exposition.png`, `2026-06-12__expositions.png` — CRUD complet (B11b/B11c).
- `2026-06-12__alerte.png` — alerte temps réel (WebSocket).
- `2026-06-12__mobile.png` — responsive < 768 px.
- `2026-06-07__swagger.png`, `2026-06-10__overview.png` — doc OpenAPI/Swagger.
- `2026-06-07__board-github.png` — board Kanban GitHub.
