# logs/ — Journal d'actions des agents

Ce dossier contient la **trace d'audit** de tout ce que font les agents IA sur Quarity.
Règle posée par la **charte agents** (interne, hors dépôt) §6 : **avant de clore une tâche, un agent écrit ici ce qu'il a fait.**

## Convention de nommage

```
logs/<YYYY-MM-DD>__<role>__<slug>.md
```

- `<YYYY-MM-DD>` : date du jour (ex : `2026-06-04`).
- `<role>` ∈ `coordinateur | agent-bdd | agent-back | agent-front | agent-infra | agent-doc | review`.
- `<slug>` : sujet en kebab-case (ex : `clickhouse-measurements-schema`).

Un fichier par **session/feature**. On **append** les entrées au fil de la session (la plus récente en bas).

Exemples de noms valides :

```
2026-06-04__coordinateur__repo-bootstrap.md
2026-06-05__agent-bdd__postgres-schema.md
2026-06-07__agent-back__auth-jwt.md
```

## Pourquoi des `.md` (et pas des `.log`)

- Lisibles directement dans une PR (rendu Markdown).
- Triables par date, greppables par rôle (`grep -r agent-bdd logs/`).
- **Versionnés** dans Git (audit), alors que les `*.log` runtime applicatifs sont **ignorés** (cf. `.gitignore`).

## Format d'une entrée (imposé)

```markdown
## <HH:MM TZ> — <action en une ligne>

- **Agent / rôle** : <role>
- **Jalon / tâche** : <ex : Jalon 1 — schéma ClickHouse mesures>
- **Contexte** : <pourquoi, 1 phrase>
- **Actions** :
  - <bullet>
  - <bullet>
- **Fichiers touchés** :
  - `<chemin>` (créé / modifié / supprimé)
- **Résultat** : OK / partiel / bloqué
- **Vérifs** : <commande + sortie clé, ou n/a>
- **Prochaine étape** : <action suivante>
- **Action Git suggérée à l'humain** :
  > <bloc « Prochaine étape » de AGENTS.md §4, ou « aucune »>
```

## Exemple rempli

```markdown
# Log — agent-bdd — 2026-06-05 — postgres-schema

## 14:32 CEST — Création du schéma Postgres métier

- **Agent / rôle** : agent-bdd
- **Jalon / tâche** : Jalon 1 — modélisation OLTP
- **Contexte** : poser les entités métier (orgs, lieux suivis, règles, profils d'exposition) avant l'API.
- **Actions** :
  - Tables `organizations`, `users`, `roles`, `memberships` (association n-aire user × org × rôle)
  - Tables `tracked_locations`, `alert_rules`, `exposure_profiles` + `exposure_thresholds`
  - Contraintes FK, index partiel sur les alertes non-lues
- **Fichiers touchés** :
  - `db/sql/01_schema.sql` (créé)
  - `db/sql/02_seed.sql` (créé)
- **Résultat** : OK — DDL valide, appliqué en local sur Postgres 16.
- **Vérifs** : `psql -f db/sql/01_schema.sql` → 0 erreur ; `\dt` → 19 tables.
- **Prochaine étape** : schéma ClickHouse des mesures (agent-bdd, même jalon).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Branche cible : feature/bdd-postgres-schema   (jamais master/dev en direct)
  > 1) git add db/sql/01_schema.sql db/sql/02_seed.sql
  > 2) git commit -m "feat(bdd): schéma Postgres métier + seed (orgs, lieux, règles, profils d'exposition)"
  > 3) git push -u origin feature/bdd-postgres-schema
  > Puis : ouvrir une PR vers `dev`, attendre la CI verte, squash merge.
  > ─────────────────────────────────────────────
```
