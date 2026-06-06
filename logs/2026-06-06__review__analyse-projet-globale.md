# Log — review — 2026-06-06 — analyse-projet-globale

## 13:57 CEST — Analyse complète du projet en lecture seule (8 agents parallèles)

- **Agent / rôle** : review
- **Jalon / tâche** : transverse — état des lieux complet post-walking-skeleton, avant missions A/B du backlog
- **Contexte** : demande d'analyse globale du projet ; cartographie de chaque sous-système puis évaluation croisée (cohérence docs↔code, sécurité, risques/dette).
- **Actions** :
  - Cartographie par 5 lecteurs indépendants : produit/docs (README, roadmap, docs/, 10 logs), back Rust/Axum, front React/Vite, couche données (Postgres + ClickHouse), infra/devops (compose, Dockerfiles, CI).
  - Évaluation croisée par 3 agents : cohérence documentation↔code (vérifiée fichier par fichier), revue de sécurité (multi-tenant, JWT, secrets), risques techniques et dette bloquante pour le Jalon 3.
  - Vérifications positives notables : `docs/data-model.md` fidèle ligne à ligne aux DDL réels ; les 5 appels du front correspondent champ par champ aux routes du back ; les 3 durcissements annoncés (jwt-secret-hardening, cors-headers, cross-tenant-tests-ci) sont réellement livrés dans le code.
  - Constats majeurs (détail dans la synthèse remise à l'humain) :
    - **Critique 1** : aucune stratégie de migration de schéma (`01_schema.sql` = DROP CASCADE, sqlx sans feature `migrate`) — bloquant pour B1–B5.
    - **Critique 2** : contradiction d'unités AQI non corrigée (COMMENT `aqi_breakpoints.unit` vs seed ppb vs data-model.md ; défaut silencieux µg/m³ dans `ingest.rs:201`) — décision D4.2 de foundations.md non appliquée.
    - **Importants (sécurité)** : refresh token == claim `jti` du JWT d'accès (S4 ouvert) ; `users.is_active` jamais revérifié au refresh (constat nouveau — un compte désactivé garde une session renouvelable 7 j) ; pas de rate-limit login/refresh (A4) ; refresh en localStorage sans CSP côté nginx ; triggers T2/T3/T4 d'intégrité multi-tenant absents.
    - **Importants (robustesse)** : ingestion OpenAQ sans pagination (limit=1000 tronqué silencieusement) ni retry/429 ; clé de dédup ClickHouse sans `sensor_id` alors que la lecture groupe par `sensor_id` ; base `quarity.` codée en dur (ch.rs:91, ingest.rs:223) ; allowlist polluants dupliquée ×4 ; multi-org tronqué (`ORDER BY m.org_id LIMIT 1`) ; dates from/to → 500 au lieu de 400 ; CI `curl https://clickhouse.com/ | sh` non épinglé.
    - **Documentaire** : roadmap.md jamais re-cochée (0 case) et backlog.md périmé (A3/A7/D1/D5 livrés mais affichés « à faire ») — risque que de futurs agents refassent du travail déjà fait ; README annonce `/api/docs` (utoipa) et Moka qui n'existent pas dans le code.
- **Fichiers touchés** :
  - `logs/2026-06-06__review__analyse-projet-globale.md` (créé — ce log uniquement ; analyse 100 % lecture seule, aucun fichier de code/doc modifié)
- **Résultat** : OK — analyse complète remise à l'humain (synthèse + priorisation).
- **Vérifs** : n/a (lecture seule ; chaque finding cite fichier:ligne, vérifié sur pièces par les évaluateurs).
- **Prochaine étape** : recommandée — lot « pré-Jalon 3 » dans cet ordre : (1) resynchroniser backlog.md/roadmap.md, (2) migrations sqlx, (3) correctif COMMENT unités AQI + suppression du défaut silencieux d'unité, (4) lot sécurité auth (S4 + is_active au refresh + A4), (5) durcissement ingestion (pagination/retry) avant tout scheduler A6.
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Branche cible : docs/review-analyse-globale     (jamais master/dev en direct)
  > 1) git add logs/2026-06-06__review__analyse-projet-globale.md
  > 2) git commit -m "docs(repo): log de revue — analyse globale du projet post-walking-skeleton"
  > 3) git push -u origin docs/review-analyse-globale
  > Puis : ouvrir une PR vers `dev`, 1 reviewer, squash merge.
  > ─────────────────────────────────────────────
