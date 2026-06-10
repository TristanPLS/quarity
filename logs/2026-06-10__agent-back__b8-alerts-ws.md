# Log — agent-back — 2026-06-10 — b8-alerts-ws

## B8 : alertes temps réel — Redis pub/sub par org → WebSocket `/api/ws`

- **Agent / rôle** : agent-back (conception via workflow multi-agents : 3 designs + 4 critiques adversariales, puis implémentation + suite e2e).
- **Jalon / tâche** : Jalon 3 — axe back, **B8** (backlog.md), dans la foulée de B6/B7.
- **Contexte** : B7 écrit les `alert_events` mais NE notifie personne (« l'événement est le fait persistant ; `notification_deliveries` reste vide »). B8 ajoute la notification temps réel : à chaque dépassement réellement constaté, publier sur un canal Redis **par org** et pousser aux clients **WebSocket** connectés. Contrainte d'archi (roadmap « Pourquoi Moka ET Redis ») : Redis = L2 distribué, pub/sub pour le push — ce que Moka (L1 in-process) ne peut pas faire en multi-instance.

- **Trouvaille de conception (critique adversariale)** : la version naïve (« publier les `Breach` si `rows_affected > 0` ») était **un bug** — `insert_events` renvoyait un COMPTEUR, or `ON CONFLICT DO NOTHING` fait que `breaches.len() != lignes insérées` ; on aurait republié un dépassement déjà alerté à CHAQUE rejouage/redémarrage/force-check (la tempête que l'idempotence 0006 de B7 évite). **Correctif** : `RETURNING` sur l'INSERT — Postgres ne renvoie QUE les lignes réellement insérées — et on publie EXACTEMENT ce jeu.

- **Actions** :
  - **Producteur** (`back/src/matching.rs`) : `insert_events` refactoré `Result<u64>` → `Result<Vec<InsertedEvent>>` (ajout `RETURNING …, measured_value::float8, …, fired_at`) ; `MatchOutcome` porte `inserted_events` (+ `inserted = len`). Publication dans les APPELANTS (séparation des responsabilités : la fonction DB reste pure) : `run_loop` (clone Redis avant la boucle) et le force-check API publient `outcome.inserted_events` — jamais au rejouage (vide).
  - **Bus** (`back/src/alerts.rs`, nouveau) : `publish_alert_events` (un PUBLISH par event sur `quarity:alerts:org:{org_id}`, via la `ConnectionManager` partagée, **best-effort** : Redis down ⇒ `warn`, jamais fatal) ; `AlertHub` = registre in-process `org_id → {clients}` (canal mpsc **borné** par client, `try_send` ⇒ drop si lent = backpressure bornée, pas d'OOM) ; `spawn_alert_subscriber` = **UNE** tâche par instance, connexion Redis dédiée (mode souscription, incompatible avec la `ConnectionManager` des commandes) `PSUBSCRIBE quarity:alerts:org:*` → fan-out mémoire vers les clients de l'org. **Pas** 1 connexion Redis par client (anti-pattern écarté). Tâche résiliente (reconnexion sur coupure).
  - **Endpoint** (`back/src/routes/ws.rs`, nouveau) : `GET /api/ws?token=<jwt>` — jeton en **query string** (un navigateur ne pose pas d'`Authorization` sur une WebSocket native) validé 1× à l'upgrade via `security::decode_access_token` (extrait de l'extracteur `AuthUser`, réutilisé) ; **`org_id` des claims SIGNÉS** (infalsifiable, jamais d'un paramètre client) ; boucle `select!` mono-tâche (pousse les messages du hub, détecte la fermeture, ping keep-alive) ; désenregistrement du hub à la sortie (tous chemins).
  - **Isolation** : un client n'est enregistré que sous SON `org_id` → un message d'une autre org ne le trouve pas. Multi-instance : l'idempotence 0006 garantit qu'UNE seule instance insère donc publie (les autres ont `RETURNING` vide) → aucun doublon côté client.
  - **Config/infra** : `WS_PING_INTERVAL_SECS` (déf. 30) + `WS_CLIENT_BUFFER` (déf. 64) dans `config.rs`, forwarding EXPLICITE `docker-compose.yml` (service back, leçon #19) + doc `.env.example`. `Cargo.toml` : feature `axum` `ws` + `futures-util` (flux pub/sub) ; dev-dependency `tokio-tungstenite` (client WS des tests). **Pas** de migration (B8 n'écrit pas `notification_deliveries` — push éphémère, la livraison durable = B9). nginx du front : `Upgrade`/`Connection` DÉJÀ posés (préparés pour le Jalon 3) — rien à toucher côté infra ; CSP `connect-src` à étendre côté front si un navigateur cible l'exige (B11).
  - **Tests** (`back/tests/b8_alerts.rs`, nouveau, +4 e2e ; +1 unit dans `alerts.rs`) : client WebSocket RÉEL (tokio-tungstenite). (1) `breach_is_pushed_to_its_org_only` : org A reçoit le snapshot complet, **org B ne reçoit RIEN sur la MÊME instance** (isolation cross-tenant — le filtrage par `org_id` du hub est le vrai gardien) ; (2) `same_org_two_clients_both_receive` : fan-out vers 2 clients de la même org ; (3) `replay_does_not_push_twice` : 1er force-check pousse, rejouage (0 inséré) ne republie RIEN (idempotence du push) ; (4) `ws_without_valid_token_is_rejected` : jeton bidon ⇒ handshake KO. Mêmes conventions que B7 (org jetable, station synthétique 999e9+, seed jamais mutée). Unit : `org_from_channel`.

- **Revue adversariale (workflow 4 axes, lecture seule)** : **isolation/auth = SAIN (0 finding)**. Durcissements RETENUS : (a) premier `PSUBSCRIBE` rendu **synchrone** dans `AppState::connect` (`start_alert_subscriber`) — `connect` ne rend la main qu'une fois l'abonné prêt, ce qui SUPPRIME la course « publié avant abonnement » (la principale fragilité de timing des e2e) ; helpers `subscribe`/`pump_messages` ⇒ connexion pub/sub fermée (drop) à chaque cycle, réabonnement TOUJOURS temporisé ; (b) test fan-out 2 clients ajouté ; (c) commentaires : `fired_at` au DEFAULT assumé, colonnes nullables toujours posées à l'INSERT. Findings ÉCARTÉS après vérif (faux positifs) : « two-orgs-same-instance non testé » (le test connecte DÉJÀ org A et org B au même `base`), « tight-loop si flux Ok » (le `sleep` était déjà inconditionnel), « fuite de connexion par reconnexion » (le `drop` ferme la connexion). Reliquats ASSUMÉS → **B8b** : arrêt gracieux explicite (CancellationToken), timeout d'envoi par socket lente, limite de connexions WS/org, publish parallèle, métriques.

- **Fichiers touchés** :
  - `back/src/alerts.rs`, `back/src/routes/ws.rs`, `back/tests/b8_alerts.rs` (créés)
  - `back/src/{matching,config,security,state,lib}.rs`, `back/src/routes/{mod,alert_rules}.rs`, `back/Cargo.toml`, `back/Cargo.lock` (modifiés)
  - `docker-compose.yml`, `.env.example`, `docs/backlog.md`, `roadmap.md` (modifiés)
  - `logs/2026-06-10__agent-back__b8-alerts-ws.md` (ce log)

- **Résultat** : OK — **166 tests verts en local contre les vraies bases** (97 e2e [+4 B8] + 24 db + 13 ch + 32 unit [+1 alerts]) ; `cargo fmt --check` OK, `cargo clippy --all-targets -D warnings` OK, `cargo build` OK.

- **Vérifs** :
  - Suite B8 verte isolément (3/3) PUIS suite complète verte (aucune régression B6/B7 malgré le refactor de `insert_events`/`MatchOutcome`).
  - Lancées avec un `JWT_SECRET` de test dédié + URLs hôte (ports mappés : pg 55432, redis 6379, ch 8123) — le vrai secret du `.env` n'a pas été touché.
  - ⚠ Conteneur back local toujours antérieur à B7/B8 (pas de boucle ni d'abonné ⇒ ne parasite pas les e2e). Après merge : `docker compose build back && docker compose up -d back` pour voir le pub/sub tourner en réel.

- **Prochaine étape** : exécuter l'action git ci-dessous ; PR vers `dev`, CI verte, squash merge. Suite logique : **B9** (profils d'exposition + API publique/quotas), **B10/B11** front (dont le **client WebSocket** qui consomme enfin ce push). Reliquat ops noté **B8b** (arrêt gracieux explicite des tâches de fond, limite de connexions WS/org, métriques).

- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Branche cible : feature/back-b8-alerts-ws   (depuis origin/dev — jamais master/dev en direct)
  > Script prêt : `..\commit-back-b8-alerts.cmd`
  > 1) git fetch origin
  > 2) git switch -c feature/back-b8-alerts-ws --no-track origin/dev
  > 3) git add back/Cargo.toml back/Cargo.lock back/src back/tests docker-compose.yml .env.example docs/backlog.md roadmap.md logs/2026-06-10__agent-back__b8-alerts-ws.md
  > 4) git commit -m "feat(back): B8 - alertes temps reel (Redis pub/sub par org -> WebSocket /api/ws) + 5 tests"
  > 5) git push -u origin feature/back-b8-alerts-ws
  > Puis : ouvrir une PR vers `dev`, attendre la CI verte, squash merge.
  > ─────────────────────────────────────────────
