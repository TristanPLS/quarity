# Diagrammes PlantUML du dossier de conception

Ce dossier contient les **sources PlantUML** des diagrammes du dossier de conception (`docs/dossier-conception.md`), au format `.puml`. Le rendu PlantUML offre une notation UML authentique (acteurs, ovales de cas d'utilisation, lignes de vie de sequence, diagrammes d'objets, classes, deploiement).

## A produire separement : le MCD Merise

Le **MCD (modele conceptuel, section 7.3 (a))** n'est pas ici : il est a realiser en **notation Merise a losanges** (entites + associations en losanges, cardinalites `(0,n)` / `(1,1)`) avec un outil dedie : **Looping**, **JMerise** ou **draw.io**. Le **MLD** (`7.3-mld.puml`) en donne la traduction relationnelle complete et sert de point de depart.

## Comment rendre les `.puml` en images

Trois options (Java 1.8 est present sur le poste) :

1. **Extension VS Code "PlantUML"** (jebbs.plantuml) : ouvrir un `.puml`, `Alt+D` pour l'apercu, clic droit -> "Export Current Diagram" (PNG/SVG). Le plus simple.
2. **plantuml.jar** (telecharger depuis plantuml.com/download) :
   ```
   java -jar plantuml.jar -tpng docs/diagrammes/*.puml      # tout en PNG
   java -jar plantuml.jar -tsvg docs/diagrammes/*.puml      # ou en SVG (vectoriel, recommande pour le PDF)
   ```
3. **Serveur public** : coller le contenu sur https://www.plantuml.com/plantuml (aucune installation).

Conseil : exporter en **SVG** pour une qualite nette dans le PDF final.

## Inventaire (fichier -> figure du dossier)

| Fichier | Figure | Type |
|---|---|---|
| `6.6-stack-docker.puml` | 6.6.1 | Deploiement (stack docker-compose) |
| `7.1-cas-utilisation.puml` | 7.1 | Cas d'utilisation |
| `7.2-classes.puml` | 7.2 | Classes |
| `7.3-mld.puml` | 7.3 (b) | MLD entite-association (crow's-foot) |
| `7.4-uc1-*.puml` ... `7.4-uc4-*.puml` | 7.4 | UC1 a UC4 : objets pre / sequence / objets post |
| `7.5-uc5-*.puml` ... `7.5-uc8-*.puml` | 7.5 | UC5 a UC8 : objets pre / sequence / objets post |
| `7.6-architecture.puml` | 7.6 | Composants (architecture applicative) |
| `9.1-gantt.puml` | 9.1 | Gantt des jalons |

30 diagrammes au total (les 24 diagrammes objets/sequence des 8 cas d'utilisation, plus cas d'usage, classes, MLD, stack Docker, architecture, Gantt). Le MCD Merise est a ajouter manuellement.
