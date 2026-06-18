# Log — agent-fullstack — 2026-06-18 — 4b backfill alertes (Vague 2)

## CEST — Endpoint GET /api/alert-events + backfill du panneau temps réel

- **Agent / rôle** : agent-fullstack (back + front)
- **Jalon / tâche** : finition v1.0 — Vague 2, PR `feat/back-alert-events-backfill` (4b). Seule tranche de la roadmap restée non livrée avant la release. Tristan a tranché : 4b d'abord, puis captures L4 + README.
- **Contexte** : limite assumée de B11a — au hard-refresh, le panneau d'alertes restait vide jusqu'au prochain push WebSocket (aucun backfill au montage). L'endpoint de lecture `GET /api/alert-events` n'existait pas (seule la *table* `alert_events` existait, écrite par la boucle de matching B7). L'index `idx_alert_events_org_fired (org_id, fired_at DESC)` existe déjà (`0001_init.sql:305`) → **aucune migration**.

### Back
- `back/src/routes/alert_events.rs` (créé) : handler `list` read-only. `AuthUser` (pas de `CanWrite` : lecture seule). Isolation **par construction** `WHERE org_id = $1` (org du JWT) — calqué sur `alert_rules.rs`. DTO `AlertEventDto` = miroir du message WS `matching::InsertedEvent` **moins** `ref_location_id` (surrogate interne non exposé) et **avec** la nullabilité des FK historisées : `alert_rule_id` / `tracked_location_id` / `openaq_sensor_id` en `Option<i64>` (FK `ON DELETE SET NULL`). NUMERIC `measured_value`/`threshold_value` castés `::float8`. Pagination/tri via `ListParams` (`AlertEventsPage { page, page_size, count, total, data }` — homogène B6) ; allowlist de tri `fired_at`/`measured_at`/`severity`, défaut `-fired_at`.
- `back/src/routes/mod.rs` : `pub mod alert_events;` + route `GET /api/alert-events` (dans `crud_routes`, près de alert-rules).
- `back/src/openapi.rs` : import `alert_events`, `alert_events::list` dans `paths(...)`, tag `alert-events`.
- `back/tests/b11_alert_events.rs` (créé) : 2 tests e2e — (1) ordre `fired_at` DESC + isolation org (A voit ses 3 events, B isolée avec 1) + sérialisation des colonnes nullables (sensor `null`, FK `null`) + champs `::float8` numériques ; (2) pagination `page_size=2` borne `data` (total reste 3) + **401 sans Bearer**. Events insérés en direct (pool PG du harnais) avec FK `NULL` (T7 ne contrôle l'org que sur référents non-NULL — exactement le cas « orphelin » du `SET NULL`).

### Front
- `front/src/api/types.ts` : `AlertEvent` — les 3 FK passent en `number | null` (le backfill peut renvoyer `null` ; le push WS les renseigne toujours). Doc mise à jour (deux origines, même forme, dédup par `id`).
- `front/src/features/alerts/mergeAlerts.ts` (créé) : helper **pur** `mergeAlerts(existing, incoming, max)` — dédup par `id` (incoming gagne à id égal), tri `fired_at` DESC (ISO-8601 ⇒ compare lexicographique ; tie-break `id` desc), borné `max`. Déterministe, indépendant de l'ordre d'arrivée.
- `front/src/features/alerts/useAlertsSocket.ts` : backfill au montage (`api.alertEvents.list(MAX_ALERTS)`, `useEffect` séparé, garde `cancelled`, échec **silencieux** `.catch(()=>{})`) fusionné par `mergeAlerts` ; le flux WS passe aussi par `mergeAlerts` (un push arrivé avant la réponse du backfill n'est ni écrasé ni dupliqué).
- `front/src/api/client.ts` : service `api.alertEvents.list(limit=50)` → `GET /api/alert-events?page_size=limit`, renvoie `Page<AlertEvent>`.
- `front/src/features/alerts/mergeAlerts.test.ts` (créé) : 7 tests Vitest (tri, dédup, fraîcheur à id égal, borne max, indépendance d'ordre, tie-break id, incoming vide).

- **Fichiers touchés** :
  - Modifiés : `back/src/openapi.rs` · `back/src/routes/mod.rs` · `front/src/api/client.ts` · `front/src/api/types.ts` · `front/src/features/alerts/useAlertsSocket.ts`
  - Créés : `back/src/routes/alert_events.rs` · `back/tests/b11_alert_events.rs` · `front/src/features/alerts/mergeAlerts.ts` · `front/src/features/alerts/mergeAlerts.test.ts`
- **Vérifs (poste de dev)** :
  - Front : `npm run lint` OK · `npm run typecheck` OK · `npm run test:run` → **27/27** (dont 7 `mergeAlerts`) · `npm run build` OK (bundle d'entrée 654 Ko inchangé, Leaflet toujours code-split).
  - Back : `cargo fmt --check` OK · `cargo clippy --all-targets -- -D warnings` → **0 warning** (compile aussi le test crate ⇒ `b11_alert_events.rs` compile proprement).
  - ⚠️ **Runtime des tests e2e back non exécuté localement** (Docker éteint, les 3 bases ne tournaient pas) → **validé par la CI** (job `back` contre PG/CH/Redis réels + seed). Aucun test existant ne casse : `openapi_json_is_complete` ne vérifie qu'une allowlist de présence (pas un compte de chemins), `/api/alert-events` ne collisionne aucun test.
- **Empreinte working tree** : 5 modifiés + 4 créés (+ ce log). Le parasite non suivi `step` (préexistant, 83 o) reste hors PR — à supprimer au ménage (9b).
- **Prochaine étape** (décision Tristan) : captures L4 (vue SGBD SQL/NoSQL, 3 requêtes SQL + 3 NoSQL, git graph, éventail 4xx) + README (compte démo standard, variables d'env). Puis release 7d (FF `dev→master` + tag `v1.0.0`).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Script : quarity/commit-feat-4b-alert-events-backfill.cmd
  > Branche cible : feat/back-alert-events-backfill (depuis origin/dev)
  > fetch + switch -c, git add (9 fichiers back/front + ce log), commit ASCII, push, gh pr create vers dev.
  > Puis : attendre la CI verte (le job back exécute b11_alert_events.rs contre les 3 bases), squash merge, maj-dev.cmd.
  > ─────────────────────────────────────────────
