# MCD Merise de Quarity (section 7.3 a du dossier)

Modele conceptuel de donnees (Merise), genere avec **Mocodo 4.3.3**.

## Pourquoi 4 vues par domaine et pas un seul schema

Le MCD complet (18 entites, ~25 associations) forme un graphe **non planaire** : des entites-pivots (Organisation, Parametre, Lieu suivi) relient tous les domaines, si bien qu'aucune disposition sans croisement de boites n'existe (Mocodo renvoie `Err.41 - plongement planaire impossible`). On le presente donc en **4 vues thematiques**, ce qui est une pratique standard et plus lisible pour le correcteur. Les entites partagees reapparaissent dans plusieurs vues : c'est volontaire et normal.

| Fichier | Domaine | Entites principales |
|---|---|---|
| `1-identite-acces` | Identite et acces | Organisation, Utilisateur, Role, Plan, Cle API |
| `2-donnees-reference` | Donnees de reference (air) | Parametre, Station, Capteur, Categorie AQI, Palier AQI |
| `3-lieux-regles-alertes` | Lieux suivis, regles, alertes | Lieu suivi, Regle alerte, Evenement alerte, Notification (+ Organisation, Station, Parametre, Utilisateur) |
| `4-exposition-doses` | Profils d'exposition et doses | Profil exposition, Seuil exposition, Exposition, Resultat dose (+ Organisation, Parametre, Lieu suivi) |

Chaque vue est fournie en **`.svg`** (vectoriel, a inserer dans le PDF), **`.png`** (apercu) et **`.mcd`** (source editable). La source complete unifiee est `quarity-mcd-complet.mocodo`.

## Notation

- Entite = rectangle ; identifiant souligne (1re propriete).
- Association = rectangle arrondi (convention Mocodo pour le losange Merise) ; cardinalites `(min,max)` sur chaque patte.
- `Adherer` est une association **ternaire** (Utilisateur x Organisation x Role) = la table `memberships`.
- `Exposition` est une **entite reifiee** (l'association lieu x profil porte une plage horaire et **produit** des resultats de dose ; comme une association ne peut pas etre reliee a une autre entite, on la reifie) = la table `tracked_location_profiles`.

> Forme des associations : Mocodo dessine des rectangles arrondis (standard academique francais), pas des losanges stricts. Si une notation a losanges est exigee, redessiner dans draw.io a partir de ces sources ; le contenu (entites, associations, cardinalites) est identique.

## Regenerer / editer

```
pip install mocodo cairosvg
python -m mocodo -i 3-lieux-regles-alertes.mcd --transform arrange   # met en page sans chevauchement -> .svg
python -c "import cairosvg; cairosvg.svg2png(url='3-lieux-regles-alertes.svg', write_to='out.png', output_width=1500)"
```

Ou, sans rien installer : coller un domaine de `quarity-mcd-complet.mocodo` sur https://mocodo.net puis bouton *Rearrange*.
