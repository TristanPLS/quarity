# Log — agent-back — 2026-06-10 — b7-matching

## 02:45 CEST — B7 : boucle de matching de seuils (Moka L1) → alert_events + force-check API

- **Agent / rôle** : agent-back (implémentation directe + 3 relecteurs adversariaux)
- **Jalon / tâche** : Jalon 3 — axe back, **B7** (backlog.md), dans la foulée du merge de B6.
- **Contexte** : le schéma était prêt (alert_rules, alert_events + T4/T6/T7, MV ClickHouse) mais AUCUNE logique applicative n'écrivait les `alert_events`. Dette explicite depuis #30 : « T7 garde la BASE, pas la boucle — ajouter un test cross-tenant sur la boucle de matching elle-même ». Contrainte d'architecture (roadmap) : règles compilées en mémoire via **Moka**, hot path < 5 ms par mesure.
- **Actions** :
  - **Moteur** (`back/src/matching.rs`, nouveau) :
    - périmètre des règles évaluables : `status='active'` × lieu `is_active` × org `deleted_at IS NULL` — compilées en index `HashMap<(station OpenAQ, polluant), Vec<StationRule>>` (une règle × N stations = N entrées, la station déclencheuse exacte est snapshotée) ;
    - **cache Moka L1** (`RuleCache`, TTL `MATCHING_RULES_TTL_SECS` déf. 60 s) — rechargement dédupliqué entre appelants (`try_get_with`), erreur SQL jamais mise en cache ;
    - évaluation PURE (zéro E/S) : lookup O(1) + comparaison `>`/`>=` — l'exigence < 5 ms est tenue par construction (benchmark formel → C2) ; valeurs non finies (NaN/Inf) tracées et ignorées ; horodatage illisible idem (fail-safe) ;
    - **curseur d'ARRIVÉE** : relecture ClickHouse par `ingested_at > watermark` (`ch.rs::query_new_measurements`, alias `last_ingested_at` — CH substitue les alias jusque dans le WHERE, leçon ILLEGAL_AGGREGATION) ; lot tronqué (== `MATCHING_BATCH_LIMIT`) ⇒ curseur reculé d'1 ms pour relire le groupe d'égalité (les lots d'ingestion partagent un même `ingested_at`) — résiduel documenté : groupe > limit ne progresse que par recouvrements ;
    - **idempotence EN BASE** (migration `0006_alert_events_dedup.sql`) : index unique partiel `(alert_rule_id, openaq_sensor_id, measured_at) WHERE alert_rule_id IS NOT NULL` + insertion UNNEST `ON CONFLICT DO NOTHING` — redémarrage (curseur perdu), fenêtres recouvrantes et force-check rejoué ne créent RIEN ; T6 (AFTER) ne joue que sur les lignes réellement insérées (compteur exact), T7 (BEFORE) joue sur chaque ligne du lot (un mismatch d'org fait échouer TOUT le lot — voulu).
  - **Boucle** (`main.rs` + `config.rs`) : tâche tokio spawnée si `MATCHING_INTERVAL_SECS > 0` (déf. 300 ; 0 = off SANS danger — pas de `restart` en boucle serrée, contrairement au scheduler A6a) ; tick en échec ⇒ warn + curseur INCHANGÉ + retenté ; index vide ⇒ curseur sauté à maintenant (pas d'alertes rétroactives sur un backlog antérieur à la première règle — choix documenté). Spawn dans `main.rs` (PAS `build_router`) : les `spawn_app` des tests n'embarquent aucune boucle.
  - **Force-check** (`POST /api/alert-rules/{id}/run`, prévu roadmap « Transverses ») : CanWrite ; 404 anti-énumération (règle scopée org) puis 422 si non évaluable (inactive/lieu en pause/org supprimée) ; `?lookback_hours=1..=168` (déf. 24) ; même moteur, même idempotence ; OpenAPI complet (`RunOutcome`).
  - **Config/infra** : `MATCHING_INTERVAL_SECS` / `MATCHING_RULES_TTL_SECS` / `MATCHING_LOOKBACK_SECS` / `MATCHING_BATCH_LIMIT` — forwarding EXPLICITE dans `docker-compose.yml` (service back, leçon #19) + doc `.env.example` (⚠ boucle ACTIVE par défaut, et consigne `docker compose stop back` avant un `cargo test` local si l'image du conteneur est ≥ B7 — sa boucle partagerait les bases des tests).
  - **Tests** (`tests/b7_matching.rs`, 8 e2e + 4 unitaires dans le module) : snapshot complet + compteur T6 (org jetable : 0 → 1), idempotence (2e run ⇒ 0 inséré, compteur stable), bornes `>` vs `>=` (valeur == seuil), règle inactive / lieu en pause = hors périmètre, **cross-tenant au niveau boucle** (station étrangère, valeur 999 ⇒ rien), **backstop T7 à travers `insert_events`** (Breach forgé org B sur règle org A ⇒ 23514 `QRT_T7`, zéro ligne, compteurs intacts — LA dette de #30 soldée), force-check de bout en bout (création puis rejouage ⇒ 0) et gardes (403 lecteur, 404 cross-org, 400 lookback, 422 inactive, 401). Stations synthétiques uniques par process (plage 999e9+, compteur atomique), orgs jetables — seed jamais mutée.
  - **Revue adversariale** (3 lentilles : sémantique/watermark, isolation/cycle de vie, contrats/tests) : pas de faille d'isolation ; correctifs appliqués — recul d'1 ms sur lot tronqué, garde NaN/Inf, clarification T6/T7 dans `insert_events`, notes `.env.example` (boucle active par défaut) et en-tête des tests (conteneur back ≥ B7 vs tests locaux). Constats rejetés après vérification (notés pour mémoire) : « premier tick retardé » (faux — `interval` tique immédiatement), « ON CONFLICT intra-lot » (impossible : le GROUP BY ClickHouse unicise la clé par lot), « TTL 60 < interval 300 inefficace » (1 requête indexée/5 min, assumé).
- **Fichiers touchés** :
  - `back/src/matching.rs`, `back/migrations/0006_alert_events_dedup.sql`, `back/tests/b7_matching.rs` (créés)
  - `back/src/{ch,config,main,lib}.rs`, `back/src/routes/{alert_rules.rs,mod.rs}`, `back/src/openapi.rs`, `back/Cargo.toml` (+ moka), `back/Cargo.lock` (modifiés)
  - `docker-compose.yml`, `.env.example` (variables MATCHING_*)
  - `docs/backlog.md` (B7 ✓ + en-tête), `logs/2026-06-10__agent-back__b7-matching.md` (ce log)
- **Résultat** : OK — **161 tests verts en local contre les vraies bases** (93 e2e [30 + 55 B6 + 8 B7] + 24 db + 13 ch + 31 unitaires) ; `fmt`/`clippy -D warnings`/`docker compose config -q` verts ; migration 0006 appliquée et vérifiée en local.
- **Vérifs** :
  - Suite complète verte APRÈS les correctifs de revue (deux passes complètes au total sur B7).
  - ⚠ Conteneur back local : image toujours ANTÉRIEURE à B6/B7 (healthcheck `unhealthy`, migrations 0005/0006 appliquées à la main en local) — après merge, prévoir `docker compose build back && docker compose up -d back` pour voir la boucle tourner en réel.
- **Prochaine étape** : exécuter l'action git ci-dessous ; PR vers `dev`, CI verte, squash merge. Suite logique du Jalon 3 back : **B8** (Redis pub/sub par org + WebSocket — `notification_deliveries` attend), B9 (profils d'exposition + API publique/quotas) ; front B10/B11.
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Branche cible : feature/back-b7-matching-loop   (depuis origin/dev — jamais master/dev en direct)
  > Script prêt : `..\commit-back-b7-matching.cmd`
  > 1) git fetch origin
  > 2) git switch -c feature/back-b7-matching-loop --no-track origin/dev
  > 3) git add back/Cargo.toml back/Cargo.lock back/migrations back/src back/tests docker-compose.yml .env.example docs/backlog.md logs/2026-06-10__agent-back__b7-matching.md
  > 4) git commit -m "feat(back): B7 - boucle de matching de seuils (Moka L1, idempotence 0006, force-check API) + 12 tests"
  > 5) git push -u origin feature/back-b7-matching-loop
  > Puis : ouvrir une PR vers `dev`, attendre la CI verte, squash merge.
  > ─────────────────────────────────────────────
