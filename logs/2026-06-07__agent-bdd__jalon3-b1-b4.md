# Log — agent-bdd — 2026-06-07 — jalon3-b1-b4

## 21:15 CEST — Jalon 3 BDD : vues métier, triggers T1–T6, procédures P1–P3, requête complexe

- **Agent / rôle** : agent-bdd
- **Jalon / tâche** : Jalon 3 — backlog **B1, B2, B3, B4** (B5 ClickHouse volontairement hors lot, prochaine session).
- **Contexte** : ouverture du Jalon 3 côté Postgres, décidée avec Tristan après la clôture de la section A (A2/A5 mergées). Les triggers ferment des trous d'intégrité réels relevés par les revues : sans T2/T4, l'isolation multi-tenant et l'immuabilité d'audit ne tenaient que par le code applicatif.
- **Actions** :
  - **B1** — migration `0002_business_views.sql` : `org_active_zones_view` (lieux actifs + compteurs stations/règles/profils + dernière alerte ; sous-requêtes scalaires anti fan-out) et `org_alert_stats_view` (volumes par sévérité, non-lus, 30 j, ratio lu — orgs sans alerte à 0 via LEFT JOIN).
  - **B2** — migration `0003_triggers.sql` : T1 (≥ 1 station par lieu actif, verrou `FOR UPDATE` anti-course, cascade parent autorisée), T2 (cohérence `org_id` — REJET explicite, pas de correction silencieuse), T3 (destinataire interne ∈ org, cibles externes libres), T4 (immuabilité jsonb fail-closed ; **nuance** : `alert_rule_id`/`tracked_location_id` mutables UNIQUEMENT vers NULL car leurs FK `ON DELETE SET NULL` passent par le trigger d'UPDATE — flagrant délit attrapé pendant l'écriture des tests), T5 (audit auto avec diff `{col: {from,to}}`, acteur via GUC `quarity.actor_user_id`, updates no-op silencieux), T6 (compteur `organizations.unread_alert_count` — colonne ajoutée ici, 0001 étant gelée).
  - **B3** — migration `0004_procedures.sql` : `create_tracked_location_with_rules` (≥ 1 station garanti à la création, 1ʳᵉ station = primaire, règles JSONB, tout-ou-rien), `archive_old_alert_events` (+ table `alert_events_archive` sans FK, id d'origine préservé ; n'archive JAMAIS un non-lu), `compute_exposure_dose` (moitié transactionnelle : fige seuil + fenêtre, upsert `exposure_results` ; l'agrégat ClickHouse arrive avec B9).
  - **B4** — `db/sql/queries/complex_business_query.sql` : top 10 orgs par alertes critiques trimestre (CTE + 4 jointures + GROUP BY + HAVING), exécutée telle quelle par les tests.
  - **Tests** — `back/tests/db.rs` : **20 tests** d'intégration (triggers, procédures, vues, requête), chacun isolé par transaction rollback ; assertions relatives tolérant les données locales (ingestion réelle). Inclut LE test de **ROLLBACK explicite** exigé par B3 (P1 : effets visibles dans la tx, rollback, zéro trace).
  - **Seed** — `02_seed.sql` : purge conditionnelle de l'archive (`to_regclass`), O3 désactivée par UPDATE (T5 journalise le vrai chemin create→deactivate), suppression des inserts manuels d'audit (le trigger fait foi).
  - **CI** — `ci.yml` : la step Postgres boucle sur `back/migrations/*.sql` (tri lexicographique bash) au lieu du seul 0001.
  - **Revue adversariale** (4 lentilles × contre-vérification, 17 agents) : 8 findings confirmés → **5 corrigés** (doublons P1 → QRT_P1 explicite au lieu d'une unique_violation→500 ; garde `is_active` P3 ; verrous `FOR SHARE` anti-TOCTOU sur T2/T3 ; CHECK en contrainte nommée conditionnelle — vraie idempotence ; fonction `recompute_unread_alert_counts()` pour les dérives TRUNCATE/restore) et **3 documentés** (hot-row T6 = limite MVP connue avec seuil ~10 events/s/org et piste de mitigation ; asymétrie d'ordre T1 = sémantique voulue ; seed sur schéma 0001 nu = comportement documenté). 5 faux positifs réfutés, 49 points RAS.
  - **Docs** — backlog B1–B4 cochés, roadmap Jalon 3 §Postgres cochée (index partiel : livré dès 0001), `data-model.md §E` annoté livré avec les nuances T4/P3.
- **Fichiers touchés** :
  - `back/migrations/0002_business_views.sql`, `0003_triggers.sql`, `0004_procedures.sql` (créés)
  - `back/tests/db.rs` (créé — 20 tests)
  - `db/sql/queries/complex_business_query.sql` (créé)
  - `db/sql/02_seed.sql`, `.github/workflows/ci.yml` (modifiés)
  - `docs/backlog.md`, `docs/data-model.md`, `roadmap.md` (modifiés — resync)
  - `logs/2026-06-07__agent-bdd__jalon3-b1-b4.md` (créé — ce log)
  - `../commit-bdd-jalon3.cmd`, `../pr-bdd-body.md` (créés — hors dépôt, jetables)
- **Résultat** : OK — livré, vérifié sous trois angles.
- **Vérifs** :
  - `cargo fmt --check` OK ; `clippy -D warnings` 0 warning ; `cargo test --all -- --test-threads=1` → **56/56** (14 unitaires + 20 db + 22 e2e).
  - **From scratch** (base jetable `quarity_seedtest`) : 0001→0004 + seed → compteurs T6 justes (agglo 2, groupeindus 2, cityair 0), audit T5 = 5 `create` + 1 `deactivate`, contrainte `chk_org_unread_nonneg` présente, requête B4 = résultat attendu (groupeindus 2 critiques/Enterprise/ratio 0.50 puis agglo 1/Pro/1.00, cityair exclue par HAVING). Base jetable supprimée.
  - **Sur base existante** : ré-application 0002–0004 idempotente (NOTICEs skip propres) — le boot sqlx du back les enregistrera dans `_sqlx_migrations` sans heurt au prochain rebuild d'image.
- **Prochaine étape** : **B5** (ClickHouse : `rolling_regulatory.sql`, calcul AQI via breakpoints) — ou **A6a** (scheduler d'ingestion) pour faire vivre la donnée. Le backlog propose aussi de scinder A6 (scheduler / backfill S3) — à acter.
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Exécuter le script `..\commit-bdd-jalon3.cmd` (depuis la racine `quarity/`), qui fait :
  > Branche cible : feature/bdd-jalon3-profondeur     (créée depuis origin/dev après fetch — jamais master/dev en direct)
  > 1) git add back/migrations/0002_business_views.sql back/migrations/0003_triggers.sql back/migrations/0004_procedures.sql back/tests/db.rs db/sql/queries/complex_business_query.sql db/sql/02_seed.sql .github/workflows/ci.yml docs/backlog.md docs/data-model.md roadmap.md logs/2026-06-07__agent-bdd__jalon3-b1-b4.md
  > 2) git commit -m "feat(bdd): jalon 3 - vues metier, triggers T1-T6, procedures P1-P3, requete complexe (+20 tests db)"
  > 3) git push -u origin feature/bdd-jalon3-profondeur
  > 4) gh pr create --base dev (corps : ../pr-bdd-body.md)
  > Puis : attendre la CI verte (3 checks), squash merge, supprimer les 2 fichiers jetables.
  > ─────────────────────────────────────────────
