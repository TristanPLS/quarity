# Log — coordinateur — 2026-06-06 — process-solo

## 22:47 CEST — Adaptation du process au mode solo : la CI remplace la review humaine

- **Agent / rôle** : coordinateur
- **Jalon / tâche** : transverse — mise en cohérence du process et des documents de gouvernance avec le mode solo
- **Contexte** : Tristan a confirmé travailler seul (choix personnel — camarades sans expérience Rust/git). La règle « PR + 1 reviewer » était inapplicable : GitHub interdit d'approuver sa propre PR, ce qui forçait le bouton « bypass rules » à chaque merge — dangereux car il court-circuite aussi les checks CI.
- **Actions** :
  - Nouvelle config de protection de branches préparée (`../protection.json`, hors dépôt) : `required_approving_review_count: 0`, **3 checks CI requis** (`Back — fmt · clippy · tests d'intégration (vraies bases)`, `Front — typecheck · build`, `Docker — build images back & front`), `enforce_admins: false` (soupape d'urgence), PR obligatoire / pas de force-push inchangés. Appliquée par Tristan via `gh api` sur `master` et `dev`.
  - `CONTRIBUTING.md` (hors dépôt) : encart « Mode solo », règle absolue reformulée (CI verte + self-review), section PR mise à jour (template `.github/`, self-review du diff), checklist review adaptée.
  - `AGENTS.md` (hors dépôt) : en-tête corrigé (la charte n'est PAS versionnée — contradiction relevée par la revue du 2026-06-06 — évolutions validées par Tristan et tracées dans `logs/`), encart « Mode solo », blocs §4 et conventions §7 : « 1 reviewer » → « CI verte ».
  - `README.md` : tableau Équipe (leads TBD) remplacé par un paragraphe actant le solo + agents IA ; règles de contribution : « 1 reviewer minimum » → « CI verte (3 checks requis) ».
  - `roadmap.md` : 2 occurrences « 1 reviewer » mises à jour (item Jalon 0 l.51, règle de discipline #2) avec mention du process solo.
  - `logs/README.md` : exemple de bloc Git du gabarit aligné (« CI verte » au lieu de « 1 reviewer »).
  - Si un contributeur humain rejoint le projet : rétablir `required_approving_review_count: 1` (noté dans CONTRIBUTING.md et AGENTS.md §7).
- **Fichiers touchés** :
  - `../CONTRIBUTING.md` (modifié — hors dépôt, pas de commit)
  - `../AGENTS.md` (modifié — hors dépôt, pas de commit)
  - `../protection.json` (modifié — hors dépôt, supprimable après application)
  - `README.md` (modifié)
  - `roadmap.md` (modifié)
  - `logs/README.md` (modifié)
  - `logs/2026-06-06__coordinateur__process-solo.md` (créé — ce log)
- **Résultat** : OK — protections réappliquées par Tristan (réponse JSON confirmée pour master et dev), documents cohérents entre eux.
- **Vérifs** : grep « 1 reviewer » ne doit plus remonter d'occurrence prescriptive dans les fichiers du dépôt (seuls les logs historiques 2026-06-04/05 conservent l'ancienne formule, normal : trace d'audit).
- **Prochaine étape** : PR de clôture Jalon 0 + process solo (fichiers de la branche `chore/repo-jalon0-cloture`), puis mission « resynchronisation backlog/roadmap » (recommandation n°1 de la revue du 2026-06-06).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Branche cible : chore/repo-jalon0-cloture     (jamais master/dev en direct)
  > 1) git add .github/PULL_REQUEST_TEMPLATE.md .github/ISSUE_TEMPLATE/tache.md README.md roadmap.md logs/README.md logs/2026-06-06__coordinateur__jalon0-cloture.md logs/2026-06-06__coordinateur__process-solo.md
  > 2) git commit -m "chore(repo): clôture Jalon 0 + process solo (CI = reviewer) — templates, board, protections"
  > 3) git push -u origin chore/repo-jalon0-cloture
  > Puis : ouvrir une PR vers `dev`, attendre la CI verte, squash merge.
  > ─────────────────────────────────────────────
