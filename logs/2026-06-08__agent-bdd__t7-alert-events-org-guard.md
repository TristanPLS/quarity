# Log — agent-bdd — 2026-06-08 — t7-alert-events-org-guard

## 22:52 CEST — P2 : trigger T7 de cohérence org_id sur alert_events (migration 0005)

- **Agent / rôle** : agent-bdd
- **Jalon / tâche** : durcissement post-analyse (rapport multi-agents du 2026-06-08) — item **P2** : fermer l'asymétrie d'isolation multi-tenant au niveau base sur la table des faits d'alerte.
- **Contexte** : la revue adversariale a confirmé que `T2` (alert_rules) et `T3` (alert_rule_recipients) verrouillent déjà la cohérence d'org côté base, mais que `alert_events` — la table des FAITS d'alerte, la plus sensible (audit) — n'avait AUCUN garde-fou. Un INSERT applicatif (future boucle de matching **B7**) avec un `org_id` incohérent fuiterait dans le dashboard d'un autre tenant et fausserait le compteur T6 / `org_alert_stats_view`. À fermer AVANT de brancher B7.
- **Actions** :
  - **Migration `0005_alert_events_org_guard.sql`** (0001 reste gelée ; toute évolution = nouvelle migration) : fonction `trg_alert_events_org_coherence` + trigger **T7** `BEFORE INSERT ON alert_events`. Miroir strict de T2 : `FOR SHARE` anti-TOCTOU sur les référents, `RAISE 'QRT_T7:'` / `ERRCODE 'check_violation'`.
  - **Logique** : quand `tracked_location_id` est non-NULL, `org_id` doit égaler `tracked_locations.org_id` ; quand `alert_rule_id` est non-NULL, `org_id` doit égaler `alert_rules.org_id`. Référent inexistant → on laisse la FK trancher. Les deux référents NULL (event ayant survécu à un SET NULL en cascade) → rien à imposer, seul `org_id` (FK garantie) porte le tenant.
  - **`BEFORE INSERT` seulement** : `org_id` est immuable (T4 rejette son évolution sur UPDATE) et `alert_rule_id`/`tracked_location_id` ne peuvent évoluer que vers NULL (T4) — aucune incohérence ne peut naître d'un UPDATE.
  - **4 tests d'intégration** ajoutés dans `back/tests/db.rs` (transaction + rollback, style T1/T2) : rejet org ≠ lieu (`QRT_T7`), rejet org ≠ règle (`QRT_T7`), acceptation d'un event cohérent, acceptation d'un event snapshot pur (référents NULL).
- **Fichiers touchés** :
  - `back/migrations/0005_alert_events_org_guard.sql` (créé — T7)
  - `back/tests/db.rs` (modifié — section T7, +4 tests)
  - `logs/2026-06-08__agent-bdd__t7-alert-events-org-guard.md` (créé — ce log)
- **Résultat** : OK — migration en place, compile, portes statiques vertes. **Compatibilité seed vérifiée** : `db/sql/02_seed.sql:274-291` insère ses `alert_events` avec `org_id = tl.org_id` et une règle appariée au lieu (T2 garantit déjà règle.org = lieu.org) → tous les events seedés passent T7, le chargement seed (CI) n'est pas cassé.
- **Vérifs** :
  - `cargo check --tests` → **OK** ; `cargo clippy --all-targets -- -D warnings` → **OK** ; `cargo fmt --check` → **OK**.
  - Tests d'intégration (Postgres réel, migrations 0001→0005 + seed) → **délégués à la CI** (job back). La CI boucle déjà sur l'ordre lexicographique des migrations : 0005 sera appliquée.
- **Prochaine étape** : exécuter l'action git ci-dessous ; PR vers `dev`, CI verte, squash merge. **À compléter quand B7 (boucle de matching) écrira réellement les `alert_events`** : ajouter un test e2e cross-tenant négatif sur le chemin de matching lui-même (T7 garde la base ; B7 doit aussi être prouvé côté API).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Branche cible : feature/bdd-t7-alert-events-org-guard   (depuis origin/dev — jamais master/dev en direct)
  > Script prêt : `..\commit-bdd-t7-alert-events-org-guard.cmd`
  > 1) git fetch origin
  > 2) git switch -c feature/bdd-t7-alert-events-org-guard --no-track origin/dev
  > 3) git add back/migrations/0005_alert_events_org_guard.sql back/tests/db.rs logs/2026-06-08__agent-bdd__t7-alert-events-org-guard.md
  > 4) git commit -m "feat(bdd): T7 - coherence org_id sur alert_events (trigger BEFORE INSERT, migration 0005) + 4 tests"
  > 5) git push -u origin feature/bdd-t7-alert-events-org-guard
  > Puis : ouvrir une PR vers `dev`, attendre la CI verte, squash merge.
  > ─────────────────────────────────────────────
