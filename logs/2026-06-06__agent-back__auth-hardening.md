# Log — agent-back — 2026-06-06 — auth-hardening

## 00:16 CEST (nuit du 06 au 07) — Lot sécurité auth pré-Jalon 3 (S4, A4 & co)

- **Agent / rôle** : agent-back
- **Jalon / tâche** : pré-Jalon 3 — durcissement auth (constats revue sécurité 2026-06-06)
- **Contexte** : la revue a relevé que le refresh token était le `jti` du JWT (fuite d'access 15 min ⇒ session 7 j), l'absence de revérification `is_active` au refresh, l'absence de rate-limit sur login/refresh, et un logout sans contrôle de propriété.
- **Actions** :
  - **S4 — refresh découplé du jti** : le refresh token est désormais un `Uuid::new_v4()` indépendant, au login ET à la rotation (clé Redis `refresh:{token}` → `user_id:org_id`, inchangée par ailleurs).
  - **Logout durci** : lecture de la valeur Redis et suppression UNIQUEMENT si le `user_id` correspond à l'appelant authentifié. Choix « **refus silencieux** » (réponse 200 identique dans tous les cas, tentative sur le token d'autrui tracée en `warn`) plutôt que 403 : un 403 ferait du logout un oracle de validité des refresh tokens d'autrui.
  - **`read_refresh` corrigé** : un payload Redis illisible est tracé (`warn`, sans logger le token) et traité comme token invalide — plus jamais de repli `(0, 0)`.
  - **`is_active` au refresh** : `db::fetch_role` remplacé par `db::fetch_refresh_context` (JOIN `users`, remonte `role_code`, `can_write`, `is_active`). Compte inactif ou membership disparu ⇒ refresh token supprimé de Redis + 401.
  - **A4 — rate-limit Redis** fenêtre fixe (INCR + EXPIRE à la 1re frappe, 60 s, zéro dépendance ajoutée) : login par email (déf. 5/min) ET par IP (déf. 20/min), refresh par IP (déf. 30/min). Nouveau variant `AppError::TooManyRequests` → 429 JSON `{error:"rate_limited", message:…}`. Limites configurables : `RATE_LIMIT_LOGIN_EMAIL_PER_MIN`, `RATE_LIMIT_LOGIN_IP_PER_MIN`, `RATE_LIMIT_REFRESH_IP_PER_MIN`.
  - **Extraction IP** (`security::client_ip`) : `main.rs` étant hors périmètre (pas de `into_make_service_with_connect_info`, donc pas de `ConnectInfo`), l'IP vient des en-têtes : priorité à `X-Real-IP` (écrasé inconditionnellement par notre nginx ⇒ non falsifiable à travers le proxy), repli sur le 1er élément de `X-Forwarded-For` (falsifiable car nginx APPEND), sinon « unknown ». **Limite documentée** dans le code : en accès direct au port du back, ces en-têtes sont forgeables → rate-limit IP best-effort ; le rate-limit par email n'en dépend pas.
  - **`REFRESH_TTL_SECS`** configurable par env (déf. 604800 = 7 j) via `config.rs`, passé en paramètre à `redis_store::store_refresh` (const codée en dur supprimée).
  - **Anti-énumération temporelle** : `security::dummy_verify_password` (vérif Argon2 factice sur un hash constant `LazyLock`, mêmes paramètres que les vrais hashes) exécutée quand l'email est inconnu/inactif au login.
  - **Tests e2e** : `spawn_app_with(tweak)` ajouté (chaque test lance sa propre instance ; les tests existants tournent avec des limites relevées car Redis est partagé entre tests parallèles — sinon 429 parasites). Nouveaux tests : (a) `refresh_token_is_independent_from_access_jti` (décodage base64url local du payload JWT, sans crate `base64`) ; (b) `deactivated_user_cannot_refresh` (compte `lea@cityair.app`, inutilisé ailleurs ; `is_active` restauré AVANT toute assertion, zone sans panic) ; (c) `sixth_failed_login_on_same_email_is_rate_limited` (email aléatoire unique par exécution → insensible au compteur 60 s en cas de relance rapide ; 5×401 puis 429 `rate_limited`).
  - **Tests unitaires** ajoutés dans `security.rs` (`client_ip` ×4, `dummy_verify_password`).
  - `front/nginx.conf` : **aucune modification nécessaire** — `proxy_set_header X-Real-IP $remote_addr;` et `proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;` déjà présents dans le bloc `/api/`.
  - `back/src/routes/mod.rs` et `back/src/routes/measurements.rs` : non modifiés (routing et extracteurs inchangés).
- **Fichiers touchés** :
  - `back/src/routes/auth.rs` (modifié)
  - `back/src/security.rs` (modifié)
  - `back/src/redis_store.rs` (modifié)
  - `back/src/db.rs` (modifié)
  - `back/src/error.rs` (modifié)
  - `back/src/config.rs` (modifié)
  - `back/tests/e2e.rs` (modifié)
  - `logs/2026-06-06__agent-back__auth-hardening.md` (créé)
- **Résultat** : OK — tout compile, clippy `-D warnings` vert, 10/10 tests unitaires verts. Tests e2e non exécutés localement (stack seedée requise — la CI s'en charge), mais ils compilent.
- **Vérifs** (PowerShell, depuis `back/`) :
  - `cargo fmt` → OK
  - `cargo check` → `Finished dev profile` (0 erreur, 0 warning)
  - `cargo clippy --all-targets -- -D warnings` → `Finished` (0 warning)
  - `cargo check --tests` → OK (les e2e compilent)
  - `cargo test --lib` → `10 passed; 0 failed`
- **Prochaine étape** : ajouter les nouvelles variables (`REFRESH_TTL_SECS`, `RATE_LIMIT_*`) à `.env.example` / docs déploiement (fichier hors périmètre de ce lot) ; Jalon 3.
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Branche cible : feature/back-auth-hardening   (jamais master/dev en direct)
  > 1) git add back/src/routes/auth.rs back/src/security.rs back/src/redis_store.rs back/src/db.rs back/src/error.rs back/src/config.rs back/tests/e2e.rs logs/2026-06-06__agent-back__auth-hardening.md
  > 2) git commit -m "feat(back): durcissement auth — refresh découplé du jti, is_active au refresh, rate-limit Redis"
  > 3) git push -u origin feature/back-auth-hardening
  > Puis : ouvrir une PR vers `dev`, attendre la CI verte, squash merge.
  > ─────────────────────────────────────────────
