# Personas — Quarity (Jalon 1)

> 5 personas couvrant les segments cibles B2G / B2B / B2B2C. Ils servent de fil rouge
> aux [user stories](user-stories.md) et au [modèle de données](data-model.md).

---

## 1. Sophie Marchand — Responsable environnement-santé *(B2G)*

**Rôle.** Responsable du service environnement et santé d'une collectivité (agglomération ~150 000 hab.).

**Contexte.** Pilote la qualité de l'air pour le compte des élus. Jongle entre obligations réglementaires (directives UE, seuils OMS), pression citoyenne et plans d'action (circulation différenciée, information du public). Pas d'équipe data dédiée ; s'appuie sur des portails AASQA statiques et des exports CSV pénibles à recouper.

**Objectifs.**
- Surveiller en continu plusieurs zones sensibles (centre-ville, axes routiers, écoles) sur PM2.5, NO₂, O₃.
- Être prévenue immédiatement d'un dépassement pour déclencher une mesure.
- Disposer des moyennes glissantes réglementaires sans les recalculer à la main.
- Produire un historique défendable devant les élus et les citoyens.

**Frustrations.** API OpenAQ brute inexploitable sans compétence technique · moyennes glissantes pénibles à calculer · aucun système d'alerte temps réel filtré sur *ses* seuils et *ses* sites · difficulté à archiver un dépassement passé une fois la donnée brute purgée.

**Critère de succès.** Recevoir une alerte fiable **< 1 min** après un dépassement réel, et pouvoir ressortir l'événement **figé** (valeur, seuil, horodatage) des mois plus tard pour le justifier.

---

## 2. Karim Benali — Directeur d'école / coordinateur périscolaire *(B2G)*

**Rôle.** Directeur d'un groupe scolaire et coordinateur des activités périscolaires.

**Contexte.** Responsable du bien-être d'enfants (population sensible) sur les temps scolaires et périscolaires. Doit décider vite : sortir ou non les enfants en récréation, maintenir ou annuler une activité sportive extérieure. Aucune compétence data, agit depuis un téléphone entre deux tâches.

**Objectifs.**
- Savoir simplement si l'air est sain pour les enfants sur la plage 8 h-17 h les jours d'école.
- Recevoir une alerte claire (libellé + couleur AQI + valeur) quand un seuil **adapté aux enfants** est franchi.
- Consulter en un coup d'œil une jauge AQI et un statut « ok / prudence / à éviter ».
- Justifier auprès des parents et de la hiérarchie la décision d'avoir gardé les enfants à l'intérieur.

**Frustrations.** Indices grand public non adaptés à une population sensible ni à une plage horaire · pas le temps d'interpréter des µg/m³ bruts · besoin d'un signal binaire actionnable · une couleur seule est illisible/inaccessible sans libellé et valeur.

**Critère de succès.** Recevoir, **avant la récréation**, une notification lisible « PM2.5 au-dessus du seuil enfants sur 8 h-12 h » qui lui évite de sortir les enfants un jour à risque.

---

## 3. Dr. Hélène Faure — Gestionnaire hospitalière / référente épidémio *(B2B)*

**Rôle.** Médecin de santé publique, veille épidémiologique et gestion des flux respiratoires d'un CHU.

**Contexte.** Croise les pics de pollution avec les admissions respiratoires (asthme, BPCO) pour anticiper l'activité des urgences et adapter le staffing. Sait lire des séries temporelles, mais veut des moyennes glissantes **calculées et exportables**, pas une API à intégrer elle-même.

**Objectifs.**
- Suivre l'exposition des populations sensibles autour des sites hospitaliers.
- Corréler dépassements et moyennes glissantes avec les arrivées aux urgences.
- Quantifier une **dose d'exposition cumulée** (heures au-dessus du seuil) sur une période et une plage horaire.
- Exporter séries temporelles et statistiques pour ses analyses.

**Frustrations.** Recalculer des moyennes glissantes sur de longs historiques est coûteux et source d'erreurs · pas d'outil reliant un profil de population à un seuil adapté et à une plage horaire · pas d'indicateur de dose prêt à l'emploi · données brutes purgées à 90 j alors qu'elle a besoin d'événements figés pour le rétrospectif.

**Critère de succès.** Obtenir en quelques clics « X heures au-dessus du seuil OMS PM2.5 pour le profil asthmatiques sur 8 h-20 h le mois dernier » et l'exporter.

---

## 4. Thomas Nguyen — Analyste ESG / QSE multi-sites *(B2B)*

**Rôle.** Analyste ESG / responsable QSE d'un grand groupe industriel multi-sites.

**Contexte.** Pilote le reporting extra-financier et la conformité QSE de dizaines de sites dans plusieurs pays. Doit consolider l'exposition de chaque site, comparer, classer, et produire des rapports auditables. Gère une organisation Quarity avec plusieurs utilisateurs et des **rôles différenciés** (lecture pour les auditeurs, gestion pour les référents site).

**Objectifs.**
- Suivre la qualité de l'air autour de chaque site et comparer les sites.
- Disposer d'un **classement** des sites les plus exposés sur un trimestre.
- Gérer les utilisateurs de son organisation et leurs rôles.
- Garder une trace **auditable** des dépassements pour le reporting ESG annuel.

**Frustrations.** Données éparpillées site par site, pas de vue consolidée · pas de gouvernance fine des accès · reporting chronophage et peu auditable · besoin d'événements de dépassement **immuables** pour l'audit.

**Critère de succès.** Sortir un rapport trimestriel consolidé classant ses sites par volume d'alertes critiques, avec des événements figés et un contrôle clair des accès par rôle.

---

## 5. Léa Dubois — Développeuse d'appli citoyenne *(B2B2C)*

**Rôle.** Développeuse fondatrice d'une petite appli citoyenne de qualité de l'air.

**Contexte.** Construit une appli grand public de quartier et veut **consommer l'API** Quarity (AQI, séries, moyennes glissantes) plutôt que de ré-ingérer OpenAQ et réimplémenter les indices. Travaille seule, sensible aux quotas et au coût, intègre en **lecture seule**.

**Objectifs.**
- Obtenir une **clé API** pour interroger mesures, AQI et séries d'un lieu.
- Connaître et maîtriser son **quota** selon son plan.
- Réponses paginées, filtrables, stables (codes HTTP corrects, doc OpenAPI à jour).
- Bénéficier du calcul AQI/moyennes glissantes déjà fait (pas de réimplémentation des breakpoints EPA).

**Frustrations.** Ré-ingérer/nettoyer OpenAQ soi-même est coûteux en solo · réimplémenter l'interpolation AQI est source d'erreurs · crainte de dépasser son quota sans visibilité · besoin d'une clé lecture seule, révocable, et d'une doc fiable.

**Critère de succès.** Intégrer en une après-midi un endpoint AQI/timeseries via une clé API lecture seule, avec un quota lisible et des **429** propres au dépassement.
