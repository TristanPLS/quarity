<!-- Titre de la PR = Conventional Commit valide : <type>(<scope>): <description> -->
<!-- types : feat | fix | chore | docs | refactor | test | perf | style | ci -->
<!-- scopes : bdd | back | front | doc | infra | repo -->

## Description

<!-- Quoi et pourquoi, en 2-3 phrases. Référencer l'item du backlog si applicable (ex : A3, B7). -->

## Préparé par

<!-- Charte AGENTS.md §6 : si le travail a été préparé par un agent IA, lien vers son log. -->

- Log agent : `logs/<YYYY-MM-DD>__<role>__<slug>.md` *(ou « travail humain direct »)*

## Vérifications

<!-- Commandes lancées + résultat clé : cargo test, npm run build, docker compose up, psql... -->

- [ ] CI verte

## Checklist (règles du projet)

- [ ] Cible `dev` (jamais `master`)
- [ ] 1 reviewer minimum assigné
- [ ] Squash merge prévu
- [ ] Aucun secret / `.env` dans le diff
- [ ] Capture dans `docs/captures/` si changement visible (règle de discipline #1)
