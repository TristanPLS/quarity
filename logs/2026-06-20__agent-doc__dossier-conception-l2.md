# Log - agent-doc - 2026-06-20 - dossier-conception-l2

## CEST - Production du dossier de conception (Livrable L2, source Markdown)

- **Agent / role** : agent-doc (orchestration multi-agents, lecture seule sur le code applicatif)
- **Jalon / tache** : conformite sujet - Livrable L2 (dossier de conception). C'etait le **seul risque eliminatoire** restant d'apres l'audit conformite ([[quarity-conformite-sujet-tp]]) : projet au vert cote code, mais dossier PDF absent. Decision Tristan en ouverture de session du 06-20 : attaquer le dossier L2.
- **Contexte** : projet livre en v1.0.0 (origin/dev == origin/master == tag v1.0.0, CI verte, 0 PR ouverte, working tree propre). Le sujet impose un dossier au **sommaire strict** (11 sections + page de garde/ToC/ToF), 30-60 pages, francais, ou une section absente = zero, et qui sert de support de soutenance. Note de cadrage : **projet solo** (derogation groupe actee) + usage d'IA a declarer.
- **Methode** :
  - **Cadrage** : extraction du sommaire impose et de la liste L4 depuis `sujet.txt` ; inventaire de la matiere premiere reelle (schema PG `db/sql/01_schema.sql` + 9 migrations vues/triggers/procedures/guards/index, schema+seed ClickHouse, 18 modules `back/src/routes/`, front React, 9 docs `docs/*.md`).
  - **Redaction** : workflow `dossier-conception-quarity` = **12 redacteurs en parallele** (un par bloc du sommaire), chacun **contraint de lire les fichiers reels** avant d'ecrire (zero invention), + **3 verificateurs adversariaux** (cardinalites MCD/MLD vs schema, liste des endpoints vs `routes/`, completude du sommaire + typographie). Agents en **lecture seule** sur le code, rendu Markdown renvoye (pas d'ecriture de fichier par les agents = zero parasite). 15 agents, ~1,18 M tokens.
  - **Assemblage + correctifs** (post-traitement node, hors depot) : strip des preambules de meta-commentaire d'agents (12 lignes), conversion des relations Mermaid invalides (`only-one to zero-or-many`) en crow's-foot valide (**29 relations**, 0 token invalide residuel), correction du comptage FILTER (`org_alert_stats_view` : cinq -> quatre), reordonnancement de 7.6 Architecture apres 7.5, rehaussement des titres 6.3-6.7 en `###`, unification de la numerotation des user stories (alignee sur 6.2 : US-01..US-22) + renvois (cle API US-11 -> US-16).
  - **Re-accentuation sure** : la 1ere generation etait en francais quasi sans accents (risque orthographe, -2 pts). Passe de re-accentuation par morceaux (11 agents) avec **invariant de surete** verifie par programme : `sans_accents(resultat) == sans_accents(origine)` -> toute modification hors accents est detectee et l'original non corrompu est conserve. L'invariant a **capte 2 corrections abusives** d'agents ("hashe"->"hache", accord "deduplique"->"dedupliquee") ; les 2 morceaux ont ete refaits en consigne stricte puis valides. Bilan : **486 -> 4487 accents**, 11/11 morceaux prouves non corrompus.
- **Fichiers touches** :
  - `docs/dossier-conception.md` (cree puis condense) - source Markdown du dossier, **~30 100 mots** (apres condensation, voir entree suivante), 13 sections (1-11 imposees + 12 Charte L5 + 13 Usage de l'IA), 32 diagrammes Mermaid.
  - `logs/2026-06-20__agent-doc__dossier-conception-l2.md` (ce log).
- **Resultat** : **OK** - source complete et coherente, fidele au code (verifiee), prete pour finalisation bureautique. Pas de PDF produit (hors `app/`, etape bureautique).
- **Verifs** : sommaire impose complet et ordonne (sections 1-13) ; >= 15 user stories (22) ; >= 2 personas (5) ; recette exhaustive en tableau ; **0 em-dash / 0 en-dash** ; fences ``` equilibrees (86) ; **0 relation Mermaid invalide** ; verificateurs adversariaux : fond MCD/MLD/triggers/procedures/endpoints confirmes exacts vs schema reel ; invariant de re-accentuation verifie (11/11).
- **Reste (finalisation bureautique, hors `app/`)** : conversion PDF (ex. pandoc ou traitement de texte), generation auto ToC + table des figures, rendu des 32 blocs Mermaid en images + insertion des PNG de `docs/captures/` aux emplacements references (8.3), saisie des champs `[A COMPLETER]` de la page de garde (classe, annee, enseignant, date), relecture orthographique finale.
- **Action Git suggeree a l'humain** :
  > ------------------------------------------------------------
  > Script : quarity/commit-docs-dossier-conception-l2.cmd
  > Branche cible : docs/dossier-conception-l2 (depuis origin/dev)
  > fetch + switch -c, git add (docs/dossier-conception.md + ce log), commit ASCII, push, gh pr create vers dev.
  > Puis : CI verte (docs-only), squash merge, maj-dev.cmd, purger le script jetable.
  > ------------------------------------------------------------

## CEST - Condensation de la prose (demande Tristan : 74 pages -> viser 30-60 indicatif)

- **Contexte** : la 1ere version etait estimee ~74 pages. Le sujet indique 30-60 pages (indicatif).
- **Methode** : passe de condensation par unites (17 condenseurs en parallele) avec **protection des diagrammes et tableaux** (extraits en jetons `@@B@@`, reinjectes verbatim apres) -> aucun diagramme ni ligne de tableau ne peut etre altere. Verification au remontage : tous les jetons resolus, **0 titre/(sous-)section perdu** (garde-fou par comparaison des titres ; l'unite 13 a ete rejetee et son original conserve car un agent avait de-accentue ses titres), fences = 86, 0 placeholder residuel, 0 Mermaid invalide, 0 em-dash.
- **Resultat** : **33 099 -> 30 084 mots (-9,1%)**. Reduction limitee car les condenseurs ont (a raison) priorise la conservation de tous les faits, et les tableaux (361 lignes) + 32 diagrammes = preuves exigees, incompressibles. Integrite : 361 lignes de tableau avant ET apres, 0 tableau ajoute par un agent.
- **Note** : le nombre de pages rendu depend surtout du gabarit bureautique (police, interligne, taille des diagrammes). Levier supplementaire si besoin : reduire le nombre de cas d'utilisation detailles en 7.4/7.5 (8 UC x 3 diagrammes). **Decision Tristan : on garde ce niveau (complet).**

## CEST - Conversion des diagrammes en PlantUML (demande Tristan)

- **Contexte** : Tristan voulait un rendu UML authentique (le rendu Mermaid n'egale pas un outil UML pour le cas d'usage, les diagrammes d'objets et le MCD Merise). Decision : convertir tout en PlantUML, SAUF le MCD que Tristan refait lui-meme en losanges Merise.
- **Methode** : extraction des 32 blocs Mermaid avec contexte (type/titre/legende), 6 convertisseurs en parallele groupes par type (8 sequences, 16 objets, classes+cas d'usage, stack+archi+gantt, MLD). Reinjection dans le dossier (Mermaid -> blocs ```plantuml) + export de chaque diagramme en `.puml` nomme dans `docs/diagrammes/` + README de rendu.
- **Resultat** : **30 diagrammes convertis** (idx 0-2, 5-31 sauf MCD), **2 blocs Mermaid conserves** (les 2 MCD, avec une note d'aide pour la version Merise). Dossier : 30 blocs ```plantuml. UML authentique verifie sur echantillon : cas d'usage (acteurs + frontiere + generalisations de roles), sequences (autonumber, activations, alt/else, notes, fideles au code auth), MLD (23 tables, PK/UK/FK, crow's-foot), Gantt (@startgantt, jalons B0-B11, dependances). Tous bien formes (@start/@end), 0 em-dash.
- **Pas de moteur PlantUML local** (Java 1.8 present mais pas de plantuml.jar) : pas de rendu PNG produit ; Tristan rend les .puml via l'extension VS Code PlantUML, plantuml.jar ou le serveur public (cf. `docs/diagrammes/README.md`).
- **Fichiers touches** : `docs/dossier-conception.md` (Mermaid -> PlantUML, sauf MCD), `docs/diagrammes/*.puml` (30) + `docs/diagrammes/README.md` (crees).
- **Reste pour Tristan** : produire le **MCD Merise en losanges** (Looping/JMerise/draw.io), rendre les .puml en images (SVG conseille), inserer dans le PDF, + finalisation (PDF, ToC/ToF, page de garde).
