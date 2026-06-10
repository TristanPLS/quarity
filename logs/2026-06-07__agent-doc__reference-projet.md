# Log — agent-doc — 2026-06-07 — reference-projet

## 23:15 CEST — Création du fichier de référence inter-projets REFERENCE-PROJET.md

- **Agent / rôle** : agent-doc
- **Jalon / tâche** : transverse — distiller la discipline Quarity en modèle réutilisable, à la demande de Tristan
- **Contexte** : Tristan veut un fichier de référence (pour lui + agents IA) corrigeant ses dérives sur d'autres projets : commits directs sur master, messages vides, zéro log, peu de commentaires.
- **Actions** :
  - Lecture des trois documents fondateurs : `../AGENTS.md`, `../CONTRIBUTING.md`, `logs/README.md`.
  - Rédaction d'un brouillon généralisé (sans spécifique Quarity), puis revue adversariale par 4 agents critiques (angles : adoption par un dev peu discipliné, exécutabilité par un agent IA, fidélité à la source, portabilité à d'autres projets) — 33 findings.
  - Révision : ajout d'une section « discipline humaine » + hook pre-commit bloquant main/dev, résolution du deadlock jour 1 (le bloc « Prochaine étape » peut créer `dev`), template PR et squelette CI fournis inline, hygiène quotidienne et rituel d'itération réintégrés depuis CONTRIBUTING.md, niveaux de discipline 0/1/complet avec déclencheurs concrets, [À ADAPTER] pour plateforme git / langue des commits / scopes / tests.
  - Rejets motivés : remplacement de `logs/` par des bodies de commit (contredit le système éprouvé), autorisation de `git stash`/`checkout` aux agents (affaiblit la règle zéro git).
- **Fichiers touchés** :
  - `../../reference/REFERENCE-PROJET.md` (créé — hors dépôt, copie maîtresse v1.0)
  - `../REFERENCE-PROJET.md` (brouillon supprimé après livraison)
  - `logs/2026-06-07__agent-doc__reference-projet.md` (créé — ce log)
  - `../commit-log-analyse-organisation.cmd` (modifié — embarque désormais les 2 logs du soir dans la même PR)
- **Résultat** : OK — fichier livré dans `__AiAgents/reference/`, aucun fichier du dépôt `app/` modifié hors `logs/`.
- **Vérifs** : `git status --short` propre après les workflows multi-agents (aucun fichier parasite) ; relecture finale du fichier livré.
- **Prochaine étape** : appliquer REFERENCE-PROJET.md au prochain nouveau projet ; reporter sur Quarity, si souhaité, les deux clarifications nées de la revue (liste blanche git lecture seule dans AGENTS.md §3, hook pre-commit local).
- **Action Git suggérée à l'humain** :
  > Exécuter le script `..\commit-log-analyse-organisation.cmd` (inchangé dans son rôle : il committe maintenant les deux logs du 2026-06-07 — review + agent-doc — dans une seule PR vers `dev`), merger à la CI verte, supprimer le script.
