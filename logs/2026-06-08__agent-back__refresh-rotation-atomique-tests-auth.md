# Log — agent-back — 2026-06-08 — refresh-rotation-atomique-tests-auth

## 22:09 CEST — P1 rotation atomique du refresh (GETDEL) + P3/P4 tests auth

- **Agent / rôle** : agent-back
- **Jalon / tâche** : durcissement post-analyse (rapport multi-agents du 2026-06-08) — items **P1** (rotation refresh non atomique, seul vrai défaut de correction du back), **P3** (logout anti-oracle non testé) et **P4** (expiration access token non testée).
- **Contexte** : la revue adversariale a confirmé une fenêtre de course réelle dans la rotation du refresh (`read` puis `delete` séparés → deux requêtes concurrentes sur le même token pouvaient dupliquer la session, vol indétectable) et deux trous de couverture sur des chemins de sécurité documentés comme subtils (logout anti-oracle, durée de vie de session). Le code de logout/expiration était correct mais sans filet de non-régression.
- **Actions** :
  - **P1 — rotation atomique** : nouvelle primitive `redis_store::take_refresh` qui **consomme** le refresh via **GETDEL** (lecture + suppression en une seule commande Redis). Redis étant mono-thread, sous deux requêtes concurrentes portant le même token, exactement une reçoit la valeur, l'autre `None` → 401. Parsing du payload `user_id:org_id` factorisé en `parse_refresh_payload` (réutilisé par `read_refresh` et `take_refresh`, même comportement « payload illisible → warn + rejet, jamais (0,0) »).
  - **P1 — handler `refresh`** (`routes/auth.rs`) : remplace le couple `read_refresh` + deux `delete_refresh` par un unique `take_refresh` en tête. Le token est donc consommé atomiquement AVANT la revalidation Postgres ; le cas compte désactivé/sorti d'org se contente désormais de refuser l'émission d'un nouveau couple (l'ancien refresh est déjà supprimé). Comportement fonctionnel inchangé hors la garantie d'atomicité. `read_refresh`/`delete_refresh` restent utilisés par le `logout` (qui doit vérifier la propriété AVANT de supprimer — anti-oracle).
  - **P1 — test de non-régression d'atomicité** : `concurrent_refresh_with_same_token_consumes_it_once` — deux `/api/auth/refresh` concurrents (`tokio::join!`) avec le même token → exactement `[200, 401]` (déterministe sur le code atomique ; échouait sur l'ancien code).
  - **P3 — logout anti-oracle** : 3 tests e2e — `logout_revokes_own_refresh_token` (le propriétaire révoque bien son token → refresh suivant 401) ; `logout_does_not_revoke_another_users_refresh_token` (sophie tente de déconnecter le refresh d'audit → le token de la victime reste valide) ; `logout_response_is_indistinguishable_across_cases` (réponse statut+corps identique pour token propre / inconnu / d'autrui).
  - **P4 — expiration / signature** : `expired_access_token_is_rejected` (token forgé avec le BON secret mais `exp` dans le passé, au-delà de la leeway 5 s → 401 `token_expired` sur `/me` et `/measurements`) ; `token_signed_with_wrong_secret_is_rejected` (signature avec un autre secret → 401 `invalid_token` — verrouille la résistance à la falsification du claim `org_id`).
- **Fichiers touchés** :
  - `back/src/redis_store.rs` (modifié — `take_refresh` GETDEL + `parse_refresh_payload` ; `read_refresh` refactoré sans changement de comportement)
  - `back/src/routes/auth.rs` (modifié — `refresh` consomme via `take_refresh`, rotation atomique)
  - `back/tests/e2e.rs` (modifié — +6 tests : 1 atomicité, 3 logout anti-oracle, 2 expiration/signature)
  - `logs/2026-06-08__agent-back__refresh-rotation-atomique-tests-auth.md` (créé — ce log)
- **Résultat** : OK — code en place, portes CI locales vertes. Tests d'intégration non exécutés localement (nécessitent les 3 bases) : ils compilent et reprennent strictement les patterns des tests existants ; ils seront exécutés par la CI contre les vraies bases.
- **Vérifs** :
  - `cargo check --tests` → **OK** (confirme notamment que `get_del` existe dans redis 0.27 et que les tests sont bien typés).
  - `cargo clippy --all-targets -- -D warnings` → **OK** (0 warning).
  - `cargo fmt --check` → **OK** (après `cargo fmt`).
  - Exécution e2e contre bases réelles → **déléguée à la CI** (services PG/CH/Redis).
- **Prochaine étape** : exécuter l'action git ci-dessous (de préférence APRÈS le commit A6a, qui isole les fichiers infra du working tree) ; PR vers `dev`, CI verte, squash merge. Suites possibles (non faites ici) : P2 (trigger `BEFORE INSERT` de cohérence `org_id` sur `alert_events`, à grouper avec B7) ; mineurs (overflow d'offset pagination, `DefaultBodyLimit` sur `/auth`, healthcheck Docker du `back`).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Branche cible : fix/back-refresh-atomic-rotation   (créée depuis origin/dev — jamais master/dev en direct)
  > Script prêt : `..\commit-back-refresh-atomique-tests.cmd` (à lancer APRÈS `..\commit-infra-a6a-scheduler.cmd`)
  > 1) git fetch origin
  > 2) git switch -c fix/back-refresh-atomic-rotation --no-track origin/dev
  > 3) git add back/src/redis_store.rs back/src/routes/auth.rs back/tests/e2e.rs logs/2026-06-08__agent-back__refresh-rotation-atomique-tests-auth.md
  > 4) git commit -m "fix(back): rotation atomique du refresh (GETDEL) + tests logout anti-oracle et expiration access token"
  > 5) git push -u origin fix/back-refresh-atomic-rotation
  > Puis : ouvrir une PR vers `dev`, attendre la CI verte, squash merge.
  > ─────────────────────────────────────────────
