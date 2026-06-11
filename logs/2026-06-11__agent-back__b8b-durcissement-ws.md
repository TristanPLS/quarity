# B8b — Durcissement WebSocket (alertes temps réel)

**Date** : 2026-06-11
**Axe** : back
**Branche cible** : `feature/back-b8b-ws-hardening` → `dev` (CI verte, squash merge)
**Part de** : `origin/dev` @ `605c747` (B10)

## Contexte

L'analyse d'ouverture du 2026-06-11 (delta non couvert par l'audit du 06-10, qui s'arrêtait à
B7) a confirmé **en vérification adversariale** un seul constat neuf réel : **`GET /api/ws` n'avait
aucune limite du *nombre* de connexions par org** (`AlertHub::register` sans cap) → un utilisateur
**authentifié** pouvait épuiser la mémoire du back en ouvrant des connexions sans fin (chaque
connexion = un canal mpsc retenu). Le constat était déjà listé comme reliquat **B8b** (backlog:71
« limite de connexions WS/org ») mais non mitigé.

B8b regroupe ce correctif avec les autres reliquats ops de B8.

## Livré (5 items)

1. **Cap connexions WS par org** *(le constat confirmé)* — `AlertHub::register` renvoie désormais
   `Option<(id, rx)>` : refus **ATOMIQUE sous le verrou** (lecture du compteur + insertion dans le
   même tour de mutex → pas de TOCTOU, deux handshakes concurrents ne dépassent pas le cap). Un refus
   ne crée aucune entrée d'org vide. `routes/ws.rs` ferme alors la socket proprement (`Close 1008`
   Policy Violation + motif). Config `WS_MAX_CONNECTIONS_PER_ORG` (déf. **50**, planché à 1).
2. **Arrêt gracieux** — `tokio_util::CancellationToken` partagé via `AppState.shutdown`, câblé à :
   la boucle de matching (`run_loop` : `select!` tick/cancel — seule l'**attente** du tick est
   interrompue, jamais un tick en cours → watermark intègre), l'abonné Redis
   (`start_alert_subscriber` : `select!` pump/cancel + `sleep_or_cancel` sur les attentes de
   réabonnement, renvoie son `JoinHandle`), et chaque session WS (arm `shutdown.cancelled()` →
   `Close 1001` Going Away). `main` annule le token **après** `axum::serve` puis attend la boucle de
   matching sous un budget borné (5 s). Règle aussi proprement le sort des sessions WS à l'extinction.
3. **Timeout d'envoi socket** — `send_or_break` enveloppe `socket.send` (messages ET pings) dans
   `tokio::time::timeout(WS_SEND_TIMEOUT_SECS, …)` (déf. **10 s**, planché à 1) : un client à la
   fenêtre TCP saturée est déconnecté plutôt que de retenir indéfiniment sa tâche de service.
4. **Publish parallèle borné** — `publish_alert_events` passe de la boucle séquentielle à
   `for_each_concurrent(16)` sur `ConnectionManager` cloné (multiplexé, clone bon marché). Signature
   `&mut ConnectionManager` → `&ConnectionManager`. Best-effort et idempotence (0006) inchangés ;
   l'ordre des PUBLISH d'un lot devient non garanti — neutre (events indépendants : id/snapshot/horodatage).
5. **Métriques légères** — compteurs atomiques `active` (connexions actives, maintenu **sous le
   verrou** du registre → exact) et `dropped` (messages déposés sur client lent/fermé), exposés via
   `active_connections()` / `dropped_messages()` + traces `debug`. **Pas** d'endpoint `/metrics`
   (infra Prometheus absente → B8c).

## Fichiers touchés

- `back/src/alerts.rs` — cap atomique dans `register` (→ `Option`), compteurs `active`/`dropped`,
  `publish_alert_events` parallèle, `start_alert_subscriber` annulable (+ `JoinHandle`, `sleep_or_cancel`).
- `back/src/routes/ws.rs` — refus au cap (Close 1008), arm shutdown (Close 1001), `send_or_break` (timeout).
- `back/src/state.rs` — champ `shutdown: CancellationToken`, `AlertHub::new(buffer, max_per_org)`, abonné détaché.
- `back/src/config.rs` — `ws_max_connections_per_org` (déf. 50), `ws_send_timeout_secs` (déf. 10).
- `back/src/matching.rs` — `run_loop` `select!` tick/cancel ; appel publish en `&redis`.
- `back/src/routes/alert_rules.rs` — call-site force-check du publish (`&redis`).
- `back/src/main.rs` — `JoinHandle` de la boucle conservé ; `shutdown.cancel()` + join borné après `serve`.
- `back/Cargo.toml` (+ `Cargo.lock`) — dépendance `tokio-util = "0.7"` (CancellationToken).
- `back/tests/b8_alerts.rs` — +1 e2e cap (`connection_cap_rejects_beyond_limit`) +1 e2e isolation
  par org (`connection_cap_is_per_org`) + helper `is_closed_soon`.
- `.env.example`, `docker-compose.yml` — `WS_MAX_CONNECTIONS_PER_ORG`, `WS_SEND_TIMEOUT_SECS`.
- `docs/backlog.md` — B8b acté.

## Vérifications (implémenteur)

- `cargo fmt --all` ✅
- `cargo clippy --all-targets -- -D warnings` ✅ (lib + bins + tests)
- `cargo build --release` ✅ (2m42s — **mirror exact du job Docker CI** : la leçon B10 sur
  `include_str!`/build release est couverte, B8b n'introduit aucun include path-dépendant)
- `cargo test --lib` ✅ **30/30** unit, dont 3 nouveaux : `register_enforces_per_org_cap`,
  `unregister_frees_a_slot`, `cap_of_zero_is_floored_to_one`.
- e2e adossés aux 3 bases (dont les 2 nouveaux tests cap) : **compilent** (clippy `--all-targets`),
  **délégués à la CI** — un `cargo test` complet en local pollue le Postgres partagé (cf. mémoire).

## Revue adversariale (3 lentilles, lecture seule sur le diff)

- **Cap & isolation** : *sain-avec-réserves*. Race-free confirmé (un seul verrou), refus laisse
  l'état propre, register/unregister strictement appairés, cap infalsifiable (`org_id` des claims
  signés), isolation multi-tenant maintenue, compteur jamais négatif.
- **Arrêt gracieux & régression** : **sain, 0 finding** (aucun tick interrompu en cours, pas de
  deadlock, chemin normal inchangé tant que le token n'est pas annulé, tests e2e B8 préservés).
- **Concurrence** : *sain-avec-réserves*. Publish parallèle sûr (ConnectionManager multiplexé),
  perte d'ordre neutre, `send_or_break` couvre les 3 cas (ok/erreur/timeout) + unregister garanti.

**Constats traités après revue** :
- *(majeur → vérifié « partiel »)* timeout d'envoi 10 s = tradeoff **assumé/documenté/configurable**,
  pas un bug ; faux-positif possible seulement sur fenêtre TCP saturée > 10 s (rare, < 1 KiB). Le vrai
  manque est l'**observabilité** (`/metrics`) — déjà repoussée à B8c. **Aucun changement de code.**
- *(mineur)* compteur `active` : incrément déplacé **sous le verrou** → exact, plus d'off-by-one.
- *(mineur)* ajout du test e2e d'**isolation du cap par org** (`connection_cap_is_per_org`).

## Reliquat → B8c (post-MVP)

- Rate-limit de **handshake** par IP/minute **avant** l'upgrade (le cap borne la mémoire RETENUE ;
  la rafale de handshakes courts reste un sujet L7 générique).
- Endpoint **`/metrics`** (Prometheus) exposant `active`/`dropped` + taux de déconnexion-timeout.

## Bloc git (exécuté par Tristan — charte : l'agent ne touche jamais git)

Script : `quarity/commit-back-b8b-ws-hardening.cmd`
- `git fetch origin` ; `git switch -c feature/back-b8b-ws-hardening --no-track origin/dev`
- `git add` (liste explicite ci-dessus) ; commit ASCII ; `git push -u` ; `gh pr create --base dev`
- Message : `feat(back): B8b - durcissement WebSocket (cap connexions par org + arret gracieux + timeout d envoi + publish parallele) + tests`
