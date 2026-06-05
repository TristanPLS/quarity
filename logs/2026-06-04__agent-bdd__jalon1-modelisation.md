# Log — agent-bdd — 2026-06-04 — jalon1-modelisation

## 15:10 CEST — Modélisation Jalon 1 : personas/US/MoSCoW + schéma Postgres + ClickHouse

- **Agent / rôle** : agent-bdd
- **Jalon / tâche** : Jalon 1 — modélisation (couche produit + MCD/MLD Postgres + schéma ClickHouse + frontière inter-bases)
- **Contexte** : poser le socle du Jalon 1 dans l'ordre roadmap (personas → user stories → MoSCoW → modèle de données) avant le walking skeleton, en intégrant les correctifs issus d'une revue adversariale du modèle.
- **Actions** :
  - Couche produit : 5 personas (B2G/B2B/B2B2C), 16 user stories format standard + critères d'acceptation testables, priorisation MoSCoW (Must = walking skeleton, plan de repli).
  - Postgres 16 : 23 tables en 3NF (tenant/RBAC, billing/API, référentiel air & OpenAQ, lieux suivis, alerting, exposition, audit), FK + ON DELETE cohérents, index partiels (1 abo actif/org, règles actives Moka, alertes non-lues, clés API vives), associations n-aires (memberships, tracked_location_stations, alert_rule_recipients, tracked_location_profiles).
  - ClickHouse 24.x : `measurements` ReplacingMergeTree(ingested_at) (idempotence), 2 rollups AggregatingMergeTree (horaire/journalier) + 2 MV, codecs DoubleDelta/Gorilla/ZSTD, TTL 90j (ttl_only_drop_parts), `UInt64` sur location_id/sensor_id.
  - Frontière inter-bases documentée (source de vérité, dénormalisation contrôlée, contrat snapshot `alert_events`, types de clés, agrégation multi-stations).
  - Seeds : Postgres (3 orgs pour tester l'isolation, breakpoints EPA conformes à identity.md, alert_events figés, 1 lieu à 0 alerte) ; ClickHouse (mesures au-dessus/en-dessous des seuils + doublon prouvant la déduplication).
  - Correctifs de revue intégrés : `org_id` dénormalisé sur `alert_rules` ; `alert_events.org_id` en RESTRICT (pas CASCADE) ; table `aqi_categories` (3NF couleur/libellé) ; `aqi_breakpoints.unit` ; `UInt64` anti-troncature ; `SimpleAggregateFunction` + correction `AggregateFunction(count)` ; `country` retiré des rollups ; index uniques partiels explicités ; snapshot de fenêtre dans `exposure_results` ; `openaq_sensor_id` figé.
  - Deux bugs détectés UNIQUEMENT à l'exécution réelle et corrigés : (1) TTL ClickHouse exige Date/DateTime → cast `toDateTime(measured_at)` requis (la « simplification » sans cast cassait) ; (2) le format VALUES de ClickHouse refuse les commentaires `--` entre tuples → seed réécrit (1 INSERT par groupe).
- **Fichiers touchés** :
  - `app/docs/personas.md` (créé)
  - `app/docs/user-stories.md` (créé)
  - `app/docs/data-model.md` (créé)
  - `app/db/sql/01_schema.sql` (créé)
  - `app/db/sql/02_seed.sql` (créé)
  - `app/db/clickhouse/01_schema.sql` (créé)
  - `app/db/clickhouse/02_seed.sql` (créé)
  - `logs/2026-06-04__agent-bdd__jalon1-modelisation.md` (créé — ⚠ hors du dépôt git, voir Prochaine étape)
- **Résultat** : OK — schémas + seeds exécutés et vérifiés sur Postgres 16 et ClickHouse 24.8 réels (conteneurs Docker jetables).
- **Vérifs** :
  - Postgres : `psql -f 01_schema.sql` puis `02_seed.sql` avec `ON_ERROR_STOP=1` → exit 0. Sanity : 23 tables, 3 orgs, 6 alert_events, 24 aqi_breakpoints. Ranking : Agglo Riviera=3, GroupeIndus=3, CityAir=0 (lieu sans alerte affiché à 0 ✓).
  - ClickHouse : `01_schema.sql` + `02_seed.sql` via clickhouse-client → exit 0. Déduplication ReplacingMergeTree : 12 lignes brutes → 11 après `FINAL` ; valeur retenue = 22.9 (version au ingested_at le plus récent ✓). Rollup horaire opérationnel (avgMerge/sum sample_count), caveat live-vs-FINAL conforme à la doc (bucket 09:00 : n=2 live, vérité FINAL=22.9).
- **Prochaine étape** : Jalon 3 BDD — implémenter les triggers/procédures listés (`01_schema.sql §8` / `data-model.md §E`), les 2 vues métier, la requête complexe `db/sql/queries/complex_business_query.sql` et `db/clickhouse/queries/rolling_regulatory.sql`. En parallèle (hors BDD) : corriger la structure du dépôt (méta-fichiers + `logs/` hors du repo `app/`) et créer la branche `dev`.
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Branche cible : feature/bdd-jalon1-modelisation     (jamais master/dev en direct)
  >
  > # NB : le dépôt git est dans app/. Les fichiers de modélisation ci-dessous y sont ;
  > #      le log ci-dessus (logs/) est À LA RACINE quarity/, DONC HORS du dépôt app/ →
  > #      il ne sera pas versionné tant que la structure n'est pas corrigée (chantier séparé).
  >
  > 1) git add app/docs/personas.md app/docs/user-stories.md app/docs/data-model.md \
  >            app/db/sql/01_schema.sql app/db/sql/02_seed.sql \
  >            app/db/clickhouse/01_schema.sql app/db/clickhouse/02_seed.sql
  > 2) git commit -m "feat(bdd): modélisation Jalon 1 — personas/US/MoSCoW + schéma Postgres 3NF + ClickHouse + seeds"
  > 3) git push -u origin feature/bdd-jalon1-modelisation
  > Puis : ouvrir une PR vers `dev` (à créer au préalable), 1 reviewer, squash merge.
  > Référencer ce log dans le champ « Préparé par » de la PR.
  > ─────────────────────────────────────────────
