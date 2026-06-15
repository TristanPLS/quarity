# Recette fonctionnelle — Quarity v1.0 (Jalon 4 · C1)

> Tableau de tests fonctionnels **attendu vs obtenu**, par domaine, rejouant les smoke-tests
> de bout en bout. Chaque ligne renvoie soit à une **capture** (`docs/captures/`), soit à la
> **couverture automatisée** (tests d'intégration back sur vraies bases + Vitest front).
> Rattaché aux [user stories](user-stories.md) et au [backlog](backlog.md). Statut global : **✅ conforme**.

## 1. Environnement de recette & reproduction

- **Stack** : `docker compose up -d --build` (5 services : Postgres 16, ClickHouse `24.8.14.39-alpine`, Redis 7, back Axum, front nginx). Ports DB en loopback ; si un Postgres natif occupe 5432 → `POSTGRES_PORT=55432` (cf. backlog D2).
- **Schéma** : appliqué au boot du back (migrations sqlx `back/migrations/0001→0009`). Seed démo : `docker compose --profile seed run --rm seed`. Données réelles : `docker compose --profile ingest run --rm ingest 4085` (NICE PROMENADE).
- **Compte de recette** : `sophie@agglo-riviera.fr` / `Quarity2026!` (org A, rôle gestionnaire). Front sur `:3000`, API sur `:8080`, doc OpenAPI `/api/docs`.
- **Couverture automatisée** (gate CI) : **~170 tests d'intégration back** sur vraies bases (`back/tests/*.rs`) + **20 tests front** Vitest (`front/src/**/*.test.tsx`). Tous verts sur `dev`.

**Légende** : ✅ conforme · codes HTTP attendus indiqués · « cap. » = capture d'écran de preuve · « auto » = couvert par test automatisé.

---

## 2. Authentification & sécurité (US-14)

| # | Scénario | Attendu | Obtenu | Preuve |
|---|----------|---------|--------|--------|
| A1 | Login email/mot de passe | JWT court (~15 min) + refresh token en Redis ; hash **Argon2id** | ✅ 200 + jetons | cap. `2026-06-07__app-login.png` · auto `e2e.rs` |
| A2 | Mauvais mot de passe / compte inconnu | **401** sans divulguer lequel (anti-énumération) | ✅ 401 générique | auto `e2e.rs` |
| A3 | Logout | Refresh token révoqué (clé Redis supprimée) | ✅ refresh ultérieur refusé | auto `e2e.rs` |
| A4 | Rate-limit login (email + IP) | Au-delà du seuil → **429** (compteur Redis atomique, auto-guérison TTL) | ✅ 429 | auto `e2e.rs` / `b9b_public_api.rs` |
| A5 | Jeton forgé / mauvais secret | **401** `invalid_token` | ✅ 401 | auto `e2e.rs` |

## 3. Organisations, rôles (RBAC) & isolation multi-tenant (US-09)

| # | Scénario | Attendu | Obtenu | Preuve |
|---|----------|---------|--------|--------|
| B1 | Rôle « lecteur » tente une création/modif | **403** `read_only_role` | ✅ 403 | auto `b6_*.rs` |
| B2 | Accès à une ressource d'une **autre org** | **404** anti-énumération (le 403 est réservé aux rôles) | ✅ 404 | cap. `2026-06-10__404-cross-tenant.png` · auto `b6_*`, `b7_matching.rs`, `b8_alerts.rs`, `b9_*` |
| B3 | Accès à **sa** ressource | **200** | ✅ 200 | cap. `2026-06-10__200-cross-tenant.png` |
| B4 | Isolation de la boucle de matching & du push WS | Un tenant ne reçoit jamais l'alerte d'un autre | ✅ isolé | auto `b7_matching.rs`, `b8_alerts.rs` (cross-tenant A/B) |

## 4. Lieux suivis & stations (US-01)

| # | Scénario | Attendu | Obtenu | Preuve |
|---|----------|---------|--------|--------|
| C1 | Créer un lieu (nom + ≥ 1 station OpenAQ) | **201**, lieu listé | ✅ 201 | cap. `2026-06-12__lieux-suivis.png` · `2026-06-10__get-tracked-locations.png` |
| C2 | Créer un lieu **sans station** | Refus **422** message explicite | ✅ 422 | auto `b6_tracked_locations.rs` |
| C3 | Lister (pagination / tri / filtre / recherche) | Page `{page, page_size, count, total, data}`, codes cohérents | ✅ 200 | cap. `2026-06-10__listing-pagination.png` |
| C4 | Éditer / supprimer | **200** / **204** ; lecture seule si rôle lecteur (**403**) | ✅ | auto `b6_tracked_locations.rs` |
| C5 | Nom de lieu dupliqué dans l'org | **409** `conflict` | ✅ 409 | auto `b6_tracked_locations.rs` |

## 5. Règles d'alerte & temps réel (US-02, US-03, US-16)

| # | Scénario | Attendu | Obtenu | Preuve |
|---|----------|---------|--------|--------|
| D1 | Créer une règle (lieu, polluant, seuil, comparateur) | **201** ; seuil/polluant immuables en édition | ✅ 201 | cap. `2026-06-12__regles-alerte.png` · `2026-06-10__post-alert-rules.png` |
| D2 | Activer / désactiver (`status`) | Bascule sans suppression ; seules les **actives** sont évaluées | ✅ | auto `b6_alert_rules.rs`, `b7_matching.rs` |
| D3 | **Force-check** `POST /api/alert-rules/{id}/run` | Évalue immédiatement (hot path Moka) ; bilan `{evaluated, breaches, events_created}` ; idempotent au re-run | ✅ 200 | auto `b7_*.rs` |
| D4 | **Alerte temps réel** : mesure > seuil | `alert_event` créé **< 1 s** après le match, **poussé en WebSocket** sans reload | ✅ alerte en direct | cap. `2026-06-12__alerte.png` |
| D5 | Snapshot d'alerte | Immuable (valeur/unité/seuil/heure figés) ; **survit au TTL 90 j** ClickHouse | ✅ figé en base (trigger T4) | auto `b7_matching.rs`, `db.rs` |
| D6 | Force-check sur règle d'une **autre org** | **403** | ✅ 403 | auto `b7_matching.rs` |

## 6. Dashboard, AQI & carte (US-05, US-06)

| # | Scénario | Attendu | Obtenu | Preuve |
|---|----------|---------|--------|--------|
| E1 | Vue d'ensemble AQI | Niveau 1-6 + **libellé FR** + **couleur EPA exacte** ; couleur **toujours** doublée d'un libellé+valeur (WCAG) ; polluant dominant | ✅ | cap. `2026-06-10__overview.png` |
| E2 | Carte des lieux | Marqueurs Leaflet/OSM colorés par AQI, popup (nom/AQI/dominant), gris = pas de donnée | ✅ | cap. `2026-06-11__map-qualite-de-l-air.png` |
| E3 | Séries temporelles `/api/measurements` | JSON **paginé** par `parameter` + plage `from/to` ; allowlist anti-injection | ✅ 200 | cap. `2026-06-07__app-dashboard-nice-4085.png` · `2026-06-07__app-dashboard.png` |
| E4 | `page_size` / `from`/`to` hors borne | **400** (validation) au lieu d'un clamp/500 silencieux | ✅ 400 | auto `e2e.rs` |
| E5 | AQI déterministe (pm25=20) | Indice EPA = **71**, isolation org A/B | ✅ | auto `b10_aqi.rs`, `ch.rs` |

## 7. Profils d'exposition & seuils (US-04)

| # | Scénario | Attendu | Obtenu | Preuve |
|---|----------|---------|--------|--------|
| F1 | Profils système (lecture seule) + custom (CRUD) | Système `org_id NULL` non modifiable (**404** en mutation) ; custom scopé org | ✅ | cap. `2026-06-12__profils-exposition.png` |
| F2 | Seuils par polluant × période | Unité **dérivée** du référentiel ; **409** doublon (profil, polluant, période) ; **400** polluant hors allowlist | ✅ | auto `b9_exposure_profiles.rs` |
| F3 | Associer un profil à un lieu (plage horaire + jours) | Plage `HH:MM`–`HH:MM` + `days_mask` + fuseau ; **422** si `end ≤ start`, **400** si `days_mask` hors 1..=127 | ✅ | auto `b9_tracked_location_profiles.rs` |

## 8. Expositions & dose cumulée (US-08)

| # | Scénario | Attendu | Obtenu | Preuve |
|---|----------|---------|--------|--------|
| G1 | Calcul de dose `POST …/{id}/compute-dose` | Heures > seuil sur la plage/jours du profil, **par fenêtre** 1h/8h/24h ; ne compte que la fenêtre horaire ; `annual` ignoré | ✅ 200 | cap. `2026-06-12__expositions.png` |
| G2 | Reproductibilité | Fenêtre **figée** au snapshot ; relecture via `GET …/{id}/results` | ✅ | auto `b9_exposure_dose.rs` |
| G3 | Doses multi-fenêtres même polluant | pm25 1h **ET** 24h → **2 lignes distinctes** (pas d'écrasement) | ✅ | auto `b9_exposure_dose.rs` (régression migration 0007) |
| G4 | Période inversée / trop longue | **400** (`period_end < period_start` ; > 366 j) | ✅ 400 | auto `b9_exposure_dose.rs` |

## 9. API publique, clés & quotas (US-11, US-12, US-13)

| # | Scénario | Attendu | Obtenu | Preuve |
|---|----------|---------|--------|--------|
| H1 | Générer une clé API (admin) | Secret `qrt_<…>` **affiché une seule fois**, stocké **hashé SHA-256** (jamais en clair) | ✅ | auto `b9b_api_keys.rs` (hash vérifié EN BASE) |
| H2 | Appel `GET /api/public/{aqi,measurements}` avec `X-API-Key` valide | **200**, scopé à l'org de la clé | ✅ 200 | auto `b9b_public_api.rs` |
| H3 | Clé manquante / invalide / **révoquée** | **401** | ✅ 401 | auto `b9b_api_keys.rs`, `b9b_public_api.rs` |
| H4 | Mutation avec clé lecture seule | **403** | ✅ 403 | auto `b9b_public_api.rs` |
| H5 | Dépassement de quota / rate-limit | **429** (token bucket Redis par plan d'abo) | ✅ 429 (30→429) | auto `b9b_public_api.rs` |
| H6 | Doc OpenAPI | `/api/docs` reflète les endpoints réels ; erreurs = corps structuré `{error, message}` | ✅ | cap. `2026-06-07__swagger.png` |

## 10. Santé, exploitation & responsive

| # | Scénario | Attendu | Obtenu | Preuve |
|---|----------|---------|--------|--------|
| I1 | `GET /health` | **200** | ✅ 200 | auto `e2e.rs` |
| I2 | Healthchecks compose | PG/CH/Redis/back/front `service_healthy` ; front **non-root** (nginx-unprivileged) | ✅ | `docker-compose.yml` |
| I3 | Responsive < 768px | Tables compactées, login/formulaires/nav adaptés | ✅ | cap. `2026-06-12__mobile.png` |
| I4 | Suivi projet | Board GitHub Projects rempli (28 items) | ✅ | cap. `2026-06-07__board-github.png` |

---

## 11. Périmètre & exclusions (fidélité)

- **Livré et recetté** : US-01, US-02, US-03, US-04, US-05, US-06, US-08, US-09, US-11, US-12, US-13, US-14, US-16 (Must + Should + Could clés).
- **Reporté (Could/Should, sans régression)** :
  - **US-10** (classement des sites) : la vue `org_alert_stats_view` existe (BDD B1) mais **aucune UI/endpoint dédié** — reportable.
  - **US-15** (destinataires par règle + `notification_deliveries`) : le push WebSocket temps réel est livré ; la **livraison durable par destinataire** (`alert_rule_recipients`/`notification_deliveries`) reste un éphémère non câblé — reportable.
  - **US-07** (moyennes glissantes réglementaires) : validées au niveau BDD (`rolling_regulatory.sql` Q1-Q4, tests `ch.rs`) ; pas d'endpoint front dédié (l'AQI consomme Q2).
- **Hors v1.0 (v1.1)** : backfill S3 (A6b), rate-limit handshake WS + `/metrics` (B8c), dose annuelle + scheduler, conformité RGPD/licence OpenAQ (D1-D3, B12-B13).

## 12. Conclusion

La chaîne **`OpenAQ → ingest → ClickHouse+Postgres → back (JWT + isolation) → nginx → front`** est
fonctionnelle de bout en bout sur les 10 domaines ci-dessus, avec **isolation multi-tenant**, **RBAC**,
**alerte temps réel < 1 s** et **API publique à quotas**. Tous les scénarios sont **conformes** (✅),
adossés à ~170 tests d'intégration back (vraies bases) + 20 tests front, et illustrés par 18 captures.
