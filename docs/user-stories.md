# User stories & priorisation MoSCoW — Quarity (Jalon 1)

> 16 user stories rattachées aux [personas](personas.md) et servies par le [modèle de données](data-model.md).
> Format : *En tant que [rôle], je veux [action] afin de [bénéfice]* + critères d'acceptation testables.

## Épics

`Auth & comptes` · `Organisation, utilisateurs & rôles` · `Lieux suivis & stations` · `Règles d'alerte & temps réel` · `Dashboard & AQI` · `Séries temporelles & moyennes glissantes` · `Profils d'exposition` · `API publique & quotas`

---

## US-01 — Définir un lieu suivi *(Sophie · Lieux suivis · **Must**)*
*En tant que responsable environnement, je veux définir un lieu suivi (nom, polluants, rattachement à une ou plusieurs stations OpenAQ) afin de surveiller une zone sensible de ma commune.*
- Création avec un nom + ≥ 1 polluant parmi `{PM2.5, NO2, O3}` via `parameters`.
- Rattachement de **plusieurs** stations OpenAQ (n-n via `tracked_location_stations`) ; le lieu reste valide tant qu'au moins une station est liée.
- La recherche de stations renvoie des `ref_locations` (clé `openaq_location_id`) avec pays + coordonnées.
- Création sans station liée refusée avec message explicite (**422**).

## US-02 — Créer une règle d'alerte *(Sophie · Règles & temps réel · **Must**)*
*…je veux définir une règle d'alerte avec un seuil par polluant sur un lieu suivi afin d'être prévenue des dépassements.*
- `alert_rule` liée à un `tracked_location`, un `parameter`, une `threshold_value` + comparateur (`>`, `>=`).
- Seuil invalide (non numérique / unité incohérente) ⇒ rejet (**422**).
- Activation/désactivation via `status` sans suppression.
- Seules les règles **actives** sont évaluées (index partiel → Moka).

## US-03 — Alerte temps réel *(Sophie · Règles & temps réel · **Must**)*
*…je veux recevoir une alerte en temps réel dès qu'une mesure dépasse un de mes seuils afin de déclencher une mesure.*
- À chaque batch, toute mesure dépassant une règle active génère un `alert_event` **< 1 s** après le match (push WebSocket via Redis pub/sub).
- L'`alert_event` **fige un snapshot immuable** : `measured_value`, `unit`, `measured_at`, `location_id`, `parameter`, `threshold_value`.
- Le snapshot **survit à la purge TTL 90 j** de ClickHouse.
- Latence de matching **< 5 ms / mesure** (lookup Moka), vérifiable en métrique.

## US-04 — Profil d'exposition + plage horaire *(Karim · Profils d'exposition · **Should**)*
*…je veux associer un profil d'exposition « enfants » à un lieu suivi avec une plage horaire afin que les seuils soient adaptés à une population sensible.*
- Liaison `exposure_profile` ↔ `tracked_location` via `tracked_location_profiles` avec `start_time`, `end_time`, jours actifs.
- Seuils issus des `exposure_thresholds` du profil (par `parameter`), distincts des seuils réglementaires.
- Le matching de ce profil ne déclenche que si `measured_at` tombe dans la plage horaire/jours.
- Plage invalide (`end ≤ start`, jours vides) refusée (**422**).

## US-05 — Jauge AQI actionnable *(Karim · Dashboard & AQI · **Should**)*
*…je veux voir une jauge AQI claire et un statut actionnable afin de décider de sortir ou non les enfants.*
- Niveau AQI courant (1-6) avec **libellé FR** et **couleur EPA exacte** d'`identity.md`.
- La couleur est **toujours** doublée d'un libellé + valeur numérique (WCAG).
- AQI calculé par **interpolation linéaire par palier**, par polluant, via `aqi_breakpoints`.
- Polluant dominant (AQI le plus élevé) mis en avant comme statut du lieu.

## US-06 — Séries temporelles *(Hélène · Séries & moyennes · **Must**)*
*…je veux consulter les séries temporelles multi-polluants d'un lieu sur une période afin de corréler les pics avec les admissions.*
- Requête par `parameter` + plage `from/to`, réponse JSON **paginée**.
- Valeurs issues de ClickHouse (`measurements` / rollups) avec `measured_at`.
- Timeseries 30 j horaire **< 200 ms p95**.
- `interval`/`agg`/`parameter` validés par **allowlist** (anti-injection ClickHouse).

## US-07 — Moyennes glissantes réglementaires *(Hélène · Séries & moyennes · **Should**)*
*…je veux obtenir les moyennes glissantes réglementaires afin d'évaluer la conformité sans recalcul.*
- L'API renvoie la moyenne glissante par polluant selon sa fenêtre réglementaire.
- Calcul via window functions ClickHouse (rollups) ; cohérent avec un recalcul manuel sur échantillon.
- Fenêtres minimales : PM2.5/24 h et O3/8 h, documentées.
- Fenêtre non supportée ⇒ erreur explicite (**400**).

## US-08 — Dose d'exposition cumulée *(Hélène · Profils d'exposition · **Could**)*
*…je veux calculer une dose d'exposition cumulée pour un profil sensible sur une plage horaire afin de quantifier l'impact sanitaire.*
- Pour (`tracked_location` + `exposure_profile` + période) : nombre d'heures au-dessus du seuil sur la plage/jours du profil.
- Ne compte que les mesures dans la fenêtre horaire définie.
- Résultat précisant polluant, seuil appliqué, période (et **fenêtre figée** → reproductible/exportable).
- Mode de production explicite (calcul ClickHouse + cache `exposure_results`) documenté.

## US-09 — Gérer utilisateurs & rôles *(Thomas · Org & rôles · **Must**)*
*…je veux gérer les utilisateurs de mon organisation et leurs rôles afin de contrôler qui peut créer des règles et qui ne fait que consulter.*
- Rattachement user ↔ org via `memberships` avec rôle (admin / gestionnaire / lecteur).
- Triplet (user, org, rôle) **unique** (pas de doublon de membership).
- Un « lecteur » se voit refuser la création/modif (**403**).
- Un user ne voit que les données de **ses** organisations (**isolation multi-tenant** vérifiée).

## US-10 — Classement des sites *(Thomas · Dashboard & AQI · **Could**)*
*…je veux un classement de mes lieux par volume d'alertes critiques sur un trimestre afin de prioriser le reporting.*
- Ranking des `tracked_locations` de mon org triés par nb d'`alert_events` critiques sur la période.
- Calcul basé sur les `alert_events` **figés** (défendables en audit), respectant l'isolation par org.
- Filtrable par période, exportable.
- Les lieux **sans alerte** apparaissent à **0** (jamais omis).

## US-11 — Générer une clé API lecture seule *(Léa · API & quotas · **Must**)*
*…je veux générer une clé API en lecture seule afin d'interroger les mesures et l'AQI d'un lieu.*
- `api_token` rattaché à mon org, portée lecture seule.
- Clé **hashée** en base (jamais en clair), affichée **une seule fois** à la création.
- Révocable immédiatement ; clé révoquée ⇒ **401**.
- Mutation avec une clé lecture seule ⇒ **403**.

## US-12 — Quota par plan *(Léa · API & quotas · **Should**)*
*…je veux connaître et respecter mon quota selon mon plan afin que mon appli ne tombe pas.*
- Quota défini par le `subscription_plan` de l'org (`organization_subscriptions`).
- Au dépassement : **429** + en-tête (limite + reset) via token bucket Redis.
- Consommation restante consultable (en-tête ou endpoint).
- Une org a au plus **1 abonnement actif** (index partiel UNIQUE).

## US-13 — Doc OpenAPI & listings cohérents *(Léa · API & quotas · **Should**)*
*…je veux une doc OpenAPI à jour et des réponses paginées/filtrables/triées afin d'intégrer vite et sans surprise.*
- Doc OpenAPI à `/api/docs`, reflétant les endpoints réels.
- Tous les listings : pagination + filtrage + tri, codes HTTP cohérents (200/400/401/403/404/429).
- Erreurs de validation = corps structuré (champ + message).
- Exemple AQI/timeseries documenté et fonctionnel tel quel.

## US-14 — Connexion sécurisée *(Sophie · Auth · **Must**)*
*…je veux me connecter de manière sécurisée afin d'accéder au dashboard de ma collectivité.*
- Login email/mot de passe via Postgres, hash **Argon2id** (jamais bcrypt/clair).
- Login réussi ⇒ JWT court (~15 min) + refresh token en Redis.
- Mauvais mot de passe / compte inconnu ⇒ **401** sans divulguer lequel.
- Logout révoque le refresh token (clé Redis supprimée).

## US-15 — Destinataires de notification *(Thomas · Règles & temps réel · **Should**)*
*…je veux définir des destinataires par règle afin que les bonnes personnes de chaque site soient prévenues.*
- ≥ 1 destinataire par `alert_rule` via `alert_rule_recipients` (user et/ou email/canal).
- Chaque envoi tracé dans `notification_deliveries` (statut + horodatage).
- Un échec d'envoi **n'empêche pas** la création de l'`alert_event` (découplage).
- Les destinataires appartiennent à l'org propriétaire de la règle (sinon **422**).

## US-16 — Force-check d'une règle *(Thomas · Règles & temps réel · **Could**)*
*…je veux forcer la vérification immédiate d'une règle afin de tester ma configuration sans attendre le batch.*
- `POST /api/alert-rules/{id}/run` évalue la règle contre les dernières mesures.
- Emprunte le même hot path Moka (**< 5 ms / mesure**).
- Un dépassement détecté crée un `alert_event` et notifie comme en batch.
- Appel sur une règle d'une **autre org** ⇒ **403**.

---

## Priorisation MoSCoW

| Catégorie | User stories |
|---|---|
| **Must** (walking skeleton Jalon 2) | US-01, US-02, US-03, US-06, US-09, US-11, US-14 |
| **Should** (profondeur Jalon 3) | US-04, US-05, US-07, US-12, US-13, US-15 |
| **Could** (valeur additionnelle, reportable) | US-08, US-10, US-16 |
| **Won't** (hors scope, pitch §5) | paiement Stripe réel · appli mobile native · prévision/IA & détection d'anomalies · capteurs physiques |

**Justification du Must.** Ce noyau matérialise le tuyau bout-à-bout du pitch — *définir un lieu → matcher → alerter → consulter* — avec auth et multi-tenant. Il couvre les 4 segments (B2G via US-01/02/03, B2B via US-06/09, API via US-11) et reste tenable pour le Jalon 2.

**Plan de repli (si le temps manque au Jalon 2).** Si la chaîne temps réel complète (US-03, push WebSocket) déborde, livrer d'abord le **tuyau lecture seule** qui prouve l'archi : **US-14 (auth) + US-01 (lieu + stations) + US-06 (timeseries ClickHouse affichée)** — soit exactement le livrable minimal du Jalon 2 du roadmap (« un user loggé voit des mesures OpenAQ réelles »). L'alerting (US-02/03) et la clé API (US-11) se branchent ensuite sans rejouer la fondation.
