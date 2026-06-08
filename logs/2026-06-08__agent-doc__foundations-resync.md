# Log — agent-doc — 2026-06-08 — foundations-resync

## 23:36 CEST — Dépoussiérage de foundations.md §4 (constats périmés + réfs fichiers)

- **Agent / rôle** : agent-doc
- **Jalon / tâche** : durcissement post-analyse (rapport multi-agents du 2026-06-08) — mineur doc : `foundations.md §4` décrivait au présent des comportements déjà corrigés et pointait un fichier vidé.
- **Contexte** : la revue a relevé deux « Constat » périmés dans §4 (un lecteur pouvait croire OUVERT un mode de défaillance sanitaire déjà fermé) : (1) « l'ingestion stocke un fallback µg/m³ arbitraire si unité absente » — FAUX depuis D4.2 (le code IGNORE désormais les mesures sans unité) ; (2) « incohérence repérée sur le COMMENT `aqi_breakpoints.unit` » — corrigée, et la référence visait `db/sql/01_schema.sql`, devenu un simple pointeur (schéma migré vers sqlx). Idem §1 : la réf. `ref_locations` pointait le fichier vidé.
- **Actions** (uniquement `docs/foundations.md`, aucun code touché) :
  - §4 1er Constat : réécrit au passé/résolu — **✅ Résolu (D4.2, PR #15, 2026-06-07)** : unités strictes, mesure sans unité ignorée+comptée, plus aucun fallback µg/m³ ; unité brute stockée, conversion au calcul AQI.
  - §4 2e Constat : **✅ Incohérence corrigée (2026-06-07)** ; le COMMENT vit désormais dans `back/migrations/0001_init.sql`, en accord avec le seed et `data-model.md §B` ; précise que `db/sql/01_schema.sql` est un pointeur sans DDL.
  - §4 3e bullet : reformulé en principe toujours valable (la valeur fausse silencieuse reste le pire mode de défaillance → justifie la séparation unité source / unité de calcul).
  - §1 « Manque actuel » : réf. `db/sql/01_schema.sql` → `back/migrations/0001_init.sql` (le constat D1 reste vrai sur le fond : `ref_locations` n'a toujours aucune colonne de licence).
  - En-tête : « Dernière mise à jour » 2026-06-05 → **2026-06-08** avec note de resync.
- **Fichiers touchés** :
  - `docs/foundations.md` (modifié — §1 réf. fichier, §4 constats datés/résolus, en-tête)
  - `logs/2026-06-08__agent-doc__foundations-resync.md` (créé — ce log)
- **Résultat** : OK — doc en cohérence avec le code et le backlog ; aucune décision juridique modifiée (D1–D4 inchangées sur le fond), seulement la fraîcheur des constats et les références de fichiers.
- **Vérifs** : relecture ; les **2 références périmées** (§1 et §4) sont corrigées vers `back/migrations/0001_init.sql`. Il subsiste **1 seule** mention de `db/sql/01_schema.sql` (§4), **intentionnelle** : elle décrit explicitement ce fichier comme un *pointeur sans DDL* — c'est de la doc utile, pas une référence périmée.
- **Prochaine étape** : exécuter l'action git ci-dessous ; PR vers `dev`, CI verte, squash merge.
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Branche cible : docs/foundations-resync   (depuis origin/dev — jamais master/dev en direct)
  > Script prêt : `..\commit-doc-foundations-resync.cmd`
  > 1) git fetch origin
  > 2) git switch -c docs/foundations-resync --no-track origin/dev
  > 3) git add docs/foundations.md logs/2026-06-08__agent-doc__foundations-resync.md
  > 4) git commit -m "docs(doc): resync foundations sec.4 - unites strictes D4.2 + COMMENT AQI corrige + refs post-migrations"
  > 5) git push -u origin docs/foundations-resync
  > Puis : ouvrir une PR vers `dev`, attendre la CI verte, squash merge.
  > ─────────────────────────────────────────────
