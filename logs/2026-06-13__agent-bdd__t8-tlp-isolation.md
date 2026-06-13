# Log — agent-bdd — 2026-06-13 — t8-tlp-isolation

## 14:55 CEST — Trigger T8 : isolation lieu × profil d'exposition au niveau base (A5)

- **Agent / rôle** : agent-bdd
- **Jalon / tâche** : Jalon 3/4 — constat A5 (audit 06-12), trou d'isolation en base.
- **Contexte** : `tracked_location_profiles` (association lieu × profil d'exposition) n'était protégée que par la **garde applicative** (`routes/tracked_location_profiles.rs` : lieu de l'org + profil système OU de l'org). Un `INSERT`/`UPDATE` **direct en base** acceptait `(lieu_orgA, profil_custom_orgB)` — contrairement à `alert_rules` (T2), `alert_rule_recipients` (T3) et `alert_events` (T7) déjà verrouillés en base. C'était la dernière asymétrie d'isolation multi-tenant.
- **Actions** :
  - Migration `0008_tracked_location_profile_org_guard.sql` : `CREATE OR REPLACE FUNCTION trg_tracked_location_profile_org_coherence()` + trigger `t8_tlp_org_coherence` **BEFORE INSERT OR UPDATE** sur `tracked_location_profiles`. Invariant : le profil doit être **système** (`org_id NULL`, partagé) **OU** appartenir à la **même org que le lieu**. `FOR SHARE` sur les référents (anti-TOCTOU, comme T2/T3/T7), `RAISE 'QRT_T8:'` / `ERRCODE 'check_violation'`. Couvre aussi l'UPDATE (lieu/profil non immuables en base). Le seed lie uniquement à des profils système → non impacté.
  - `back/tests/db.rs` : 3 tests T8 — `t8_foreign_custom_profile_association_is_rejected` (profil custom d'une autre org → QRT_T8), `t8_system_profile_association_is_allowed` (profil système → OK), `t8_same_org_custom_profile_association_is_allowed` (profil custom de la même org → OK).
- **Fichiers touchés** :
  - `back/migrations/0008_tracked_location_profile_org_guard.sql` (créé)
  - `back/tests/db.rs` (modifié)
- **Résultat** : OK.
- **Vérifs** : `cargo fmt` OK · `cargo clippy --all-targets -- -D warnings` OK · **smoke du trigger sur Postgres RÉEL** (transaction rollback, trigger créé inline) → `TEST1 FOREIGN: rejeté` · `TEST2 SYSTEM: accepté` · `TEST3 SAME-ORG: accepté` · `ROLLBACK` (base intacte). Les 3 tests `db.rs` e2e délégués à la CI.
- **Prochaine étape** : #6 borne de période + parallélisation `compute-dose` (A3/P3), puis ESLint (A6) + process (resync backlog/roadmap, README final, tag v1.0).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Script : quarity/commit-fix-t8-tlp-isolation.cmd
  > Branche cible : fix/bdd-t8-tlp-isolation (depuis origin/dev — contient deja 0007)
  > fetch + switch -c, git add migration 0008 + db.rs + ce log, commit ASCII, push, gh pr create vers dev.
  > Puis : attendre la CI verte, squash merge.
  > ─────────────────────────────────────────────
