# Log - agent-doc - 2026-06-18 - prep release v1.0 (backlog C4 + tirets README)

## CEST - Finalisation avant promotion dev->master + tag v1.0.0

- **Agent / role** : agent-doc (doc pure, aucun code touche)
- **Jalon / tache** : Jalon 4 / C4 - PR `chore/release-prep-v1` (prep de la release 7d). Dernier commit sur `dev` AVANT la promotion fast-forward vers `master`.
- **Contexte** : tout est vert pour la release - `dev` @ `e1e9aaa`, CI verte, `master` FF-able (95 commits d'avance, 0 retard), C1/C2/C3 coches, `/health` 200 (5 services healthy). Decision Tristan : demo **locale Docker** (pas de host public), tirets courts ASCII.
- **Actions** :
  - `docs/backlog.md` : **C4 -> [x]** (deploiement local Docker + recette `/health` 200 + promotion FF + tag `v1.0.0` ; deploiement public = bonus non vise).
  - `docs/captures/README.md` : normalisation des tirets longs en tirets courts ASCII (changement de Tristan, repris tel quel - cf. [[tristan-tirets-courts-ascii]]).
- **Fichiers touches** : `docs/backlog.md` (modifie) · `docs/captures/README.md` (modifie) · ce log (cree).
- **Verifs** : `docs/captures/README.md` = 0 em-dash restant (normalisation complete) ; backlog C1-C4 tous coches ; aucun code touche (PR docs-only, CI verte attendue).
- **Suite immediate** : une fois cette PR mergee + `maj-dev.cmd`, lancer `quarity/commit-release-v1.0.0.cmd` = promotion `dev->master` en `git merge --ff-only` + tag `v1.0.0` (suite de `v1.0.0-rc.1`) + push.
- **Action Git suggeree a l'humain** :
  > -----------------------------------------------
  > Script : quarity/commit-chore-release-prep.cmd
  > Branche cible : chore/release-prep-v1 (depuis origin/dev)
  > fetch + switch -c, git add (backlog + captures README + ce log), commit ASCII, push, gh pr create vers dev.
  > Puis : CI verte, squash merge, maj-dev.cmd, PUIS commit-release-v1.0.0.cmd.
  > -----------------------------------------------
