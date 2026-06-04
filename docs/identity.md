# Identité visuelle — Quarity

## Nom & étymologie

**Quarity** — mot-valise de **Qual**ity + **Air** (+ écho sonore à *clarity* / *purity*). Le nom fond « qualité de l'air » en un seul mot et porte la promesse produit : rendre l'air **lisible** (clarity) et donner à voir l'objectif d'un air **pur** (purity).

**Pourquoi ce nom** :

- Court (3 syllabes), prononçable à l'identique en français et en anglais (*KWA-ri-ty*).
- Image mentale immédiate : qualité + air, sans jargon.
- Racine « Q » distinctive, pratique pour le logo et le favicon.
- Pas de collision majeure connue dans le secteur (à vérifier avant tout dépôt nominal réel).

## Palette de marque

Champ lexical : air, ciel, atmosphère, respiration, fraîcheur. Palette **plate** (pas de gradient sauf cas explicite), sobre, orientée « data ».

| Rôle | Nom | Hex | Usage |
|---|---|---|---|
| Primaire | Bleu atmosphère | `#0B3D5C` | Backgrounds sombres, navigation, texte fort |
| Secondaire | Ciel clair | `#CFE8F5` | Cards, séparateurs subtils, états hover, fonds de section |
| Accent / interaction | Cyan respiration | `#1FA8B8` | CTA, liens actifs, indicateurs interactifs |
| Succès | Vert menthe | `#3FB984` | Confirmations, statuts « données fraîches », connecté |
| Texte clair | Brume | `#F4F8FA` | Texte sur fond sombre, contraste principal |
| Texte sombre | Encre nuit | `#13242E` | Texte sur fond clair |

Le couple Bleu atmosphère + Ciel clair évoque le ciel vu d'en haut ; le Cyan respiration = signal d'interaction, frais et net.

> **Important** : l'accent de marque (cyan) **ne sert jamais** à signifier un niveau de pollution. Le codage d'un niveau d'air se fait **uniquement** avec l'échelle AQI ci-dessous.

## Échelle AQI sémantique (séparée)

Set de couleurs **dédié** aux jauges, points de carte et badges de niveau. Standard **US EPA** (le plus universellement reconnu ; variante EU CAQI possible en option). Ces couleurs n'encodent **qu'un niveau de qualité d'air**, jamais des éléments d'UI génériques.

| Niveau | Libellé | Plage AQI | Hex |
|---|---|---|---|
| 1 | Bon | 0–50 | `#00E400` |
| 2 | Modéré | 51–100 | `#FFFF00` |
| 3 | Mauvais pour groupes sensibles | 101–150 | `#FF7E00` |
| 4 | Mauvais | 151–200 | `#FF0000` |
| 5 | Très mauvais | 201–300 | `#8F3F97` |
| 6 | Dangereux | 301+ | `#7E0023` |

On ne « personnalise » pas une échelle réglementaire : ces hex sont conservés tels quels pour une reconnaissance instantanée. Pour l'accessibilité, **toujours doubler la couleur d'un libellé + d'une valeur numérique** (le jaune `#FFFF00` sur blanc échoue WCAG : on l'accompagne d'un texte et d'un fond sombre dans les badges).

## Typographie

- **Headings** : *Inter* (700) — sans-serif lisible, optimisée écran, open source.
- **Body** : *Inter* (400/500) — cohérent avec headings, bundle léger.
- **Monospace / data / chiffres** : *JetBrains Mono* (400) — chiffres alignés (`tabular-nums`) pour les tableaux de mesures, valeurs µg/m³, timestamps.

Une seule famille de texte (Inter) + une mono pour la donnée : bundle léger, cohérence visuelle, charge cognitive faible.

## Logo (concept)

Logo simple à produire en haute-fi. Concept :

```
   ╭───────────╮
   │   · · ·   │    Trois points ascendants = particules / mesure dans l'air
   │  ·  Q  ·  │    Un « Q » formé d'un anneau-capteur (la lentille atmosphérique),
   │   ╰───╯   │    ouvert en bas-droite (la queue du Q = la prise d'air)
   ╰─────┬─────╯
         │          Mât de la station de mesure
       QUARITY
```

Variante favicon : le **Q** seul (anneau + point unique), avec une **pulsation CSS** sur la version web (= la « respiration » / le battement d'une mesure live). Version monochrome : anneau + queue, sans les points.

## Règles d'usage

- L'accent **Cyan respiration** ne dépasse jamais ~10 % d'une vue (c'est un accent d'interaction, pas un fond).
- **Ne jamais mélanger** palette de marque et échelle AQI : le cyan/vert de marque ne signifient pas « qualité d'air » ; les couleurs AQI ne servent pas de couleurs d'UI génériques.
- Toujours respecter un contraste **WCAG AA** minimum (texte 4.5:1, gros texte 3:1). Les couleurs AQI claires sont posées sur fond sombre dans les badges et doublées d'un libellé.
- Palette plate, pas de gradient — sauf jauges/fonds de carte où un dégradé de l'échelle AQI est sémantiquement justifié.
- Le mot **Quarity** est toujours capitalisé en début de phrase. Pas de stylisation custom (pas de tout-majuscules, pas de leetspeak).

## Livrables identité (V1)

- [ ] Logo SVG final + déclinaisons (favicon, OG image, version monochrome)
- [ ] Page design system (palette de marque, échelle AQI, typo, composants UI, exemples)
- [ ] Composants React de base (`Button`, `Card`, `Badge`, `Alert`, `AqiGauge`) appliquant la charte
