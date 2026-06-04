# Quarity — Pitch produit & stratégie technique

*Document support — vision produit et justifications d'architecture.*

---

## 1. Le problème

La pollution de l'air est le **premier facteur de risque environnemental pour la santé** (des millions de décès prématurés par an selon l'OMS). Pourtant, pour une collectivité, une école, un hôpital ou un grand compte, **agir sur la base des données est difficile** :

- Les obligations et seuils réglementaires (OMS, directives UE, indices nationaux) sont définis comme des **moyennes glissantes** (PM2.5 sur 24 h, O₃ sur 8 h, NO₂ annuel) — pénibles à calculer à la main.
- [OpenAQ](https://openaq.org/) agrège ouvertement des **centaines de réseaux de capteurs dans le monde**, mais son API brute est **inexploitable sans compétences techniques** et **sans interface temps réel**.
- Les outils existants sont soit des portails institutionnels statiques, soit des applis grand public sans logique d'alerte métier (seuils par site, par population sensible, par organisation).

## 2. Quarity

Plateforme SaaS qui transforme OpenAQ en **système d'alerte temps réel filtré sur des seuils déclarés**.

**User flow type** :

1. Une organisation définit un **lieu suivi** — ex : *École Jules-Ferry*, rattachée à une ou plusieurs stations OpenAQ, polluants = `{PM2.5, NO2, O3}`, seuils = recommandations OMS.
2. À chaque batch d'ingestion, Quarity matche les nouvelles mesures contre les **règles d'alerte** actives.
3. Si dépassement → **push WebSocket** vers le client web + entrée dans le journal d'alertes (`alert_events` Postgres).
4. L'utilisateur consulte le dashboard : carte des stations, jauge AQI, séries temporelles multi-polluants, moyennes glissantes réglementaires, historique.

**Différenciateur — les profils d'exposition** : Quarity ne se contente pas d'afficher une concentration. Il calcule une **exposition personnalisée** pour une **population sensible** (enfants, asthmatiques, personnes âgées, sportifs) selon des seuils adaptés — ex : *« sur la plage 8 h-17 h, les enfants de l'école X ont subi 3 h au-dessus du seuil OMS PM2.5 »*. C'est l'utilité sociale concrète du produit.

**Cibles** :

- **Collectivité / autorité sanitaire** (B2G) : surveillance de la commune, déclenchement de mesures (circulation différenciée, information du public).
- **Établissement** (école, hôpital) : décisions opérationnelles (récréation extérieure, admissions respiratoires).
- **Grand compte ESG/QSE** (B2B) : reporting extra-financier, exposition des sites.
- **Dev d'appli citoyenne** : consomme l'API Quarity (clé API + quota par plan).

## 3. Stratégie technique — points clés

### 3.1 Deux bases, deux mondes

- **Postgres** = source de vérité **OLTP** : `users`, `organizations`, `memberships`, `tracked_locations`, `alert_rules`, `exposure_profiles`, `alert_events`, `subscription_plans`. Schéma 3NF, contraintes FK strictes, transactions ACID. C'est là que vit le métier B2B/B2G.
- **ClickHouse** = source de vérité **analytics** : `measurements` partitionnée mensuellement (`PARTITION BY toYYYYMM(measured_at)`), `ORDER BY (location_id, parameter, measured_at)`, `LowCardinality` sur `parameter`/`country`/`location_id`, codecs `DoubleDelta + ZSTD` sur les timestamps, `Gorilla + ZSTD` sur les valeurs de capteurs. Compression typique 10-50× ; rollups horaires/journaliers en `AggregatingMergeTree`.
- **Pas de duplication** : une mesure vit dans ClickHouse. Une alerte (« telle mesure a franchi tel seuil à telle date ») vit dans Postgres. La frontière est nette, documentée, et défendable.

### 3.2 Pourquoi ClickHouse plutôt que Cassandra (famille colonnes)

1. **Pattern de requêtes** = agrégations sur dimensions arbitraires (date × ville × polluant × capteur). Cassandra exige de connaître les partition keys à l'avance ; ClickHouse agrège nativement sur n'importe quelle colonne grâce au stockage column-wise.
2. **Compression** : 10-50× mieux que Cassandra sur les colonnes basse-cardinalité grâce à `LowCardinality` et aux codecs spécialisés (`DoubleDelta`, `Gorilla`, `ZSTD`). `Gorilla` est conçu exactement pour les séries de valeurs flottantes de capteurs.
3. **Modèle d'écriture** : OpenAQ pousse des batches périodiques, pas des millions d'updates/sec sur clé connue. Cassandra est sur-dimensionnée ; ClickHouse correspond au profil batch + analytics ad-hoc.

ClickHouse compte comme **NoSQL** : stockage physique column-wise (MergeTree append-only, un fichier par colonne — inverse exact d'un B-tree row-oriented Postgres), garanties **BASE** (pas de FK appliquées, mutations asynchrones, `ReplacingMergeTree` en eventual consistency), positionnement **CAP-AP**. SQL n'est qu'une interface.

**Pourquoi pas un autre paradigme** : document = inadapté aux séries temporelles denses ; clé-valeur = pas d'agrégation temporelle ; graphe = aucun sens pour des mesures de capteurs. Le couple **relationnel (métier) + columnar (mesures)** est le bon découpage.

### 3.3 Pourquoi Moka **et** Redis (pas l'un ou l'autre)

- **Moka (L1, in-process)** : les règles d'alerte compilées sont rechargées en mémoire à chaque batch d'ingestion. La boucle de matching évalue chaque mesure contre les règles concernées ; chaque mesure doit rester **sous 5 ms**. Lookup mémoire pur, pas de réseau.
- **Redis (L2, distribué)** : sessions JWT, refresh tokens, rate-limit counters (token bucket par IP + user, quota par clé API), **pub/sub pour push WebSocket** (un dépassement → publication sur channel → broadcast aux clients connectés). Ce que Moka ne peut pas faire.

Endpoint qui bénéficie **spécifiquement** de Moka : la boucle de matching de seuils (et `POST /api/alert-rules/{id}/run`), hot path serveur.

### 3.4 Sécurité (résumé)

- Mots de passe : **Argon2id** (pas bcrypt — état de l'art).
- JWT court (~15 min) + refresh tokens en Redis (révocation immédiate possible).
- Validation systématique des inputs (`serde` + crate `validator`).
- Paramètres préparés partout (anti-injection SQL ET ClickHouse ; allowlist stricte des `parameter`/`interval`/`agg`).
- CORS strict (allowlist des origins front), CSP, X-Frame-Options, HSTS via `tower-http`.
- Clés API citoyennes **hashées** en base, scope lecture seule, quota par plan.
- Aucun secret dans le repo, tout en variables d'environnement.

## 4. Indicateurs de succès du produit

| Indicateur | Cible |
|---|---|
| Latence matching mesure → alerte | < 5 ms / mesure (lookup Moka) |
| Ingestion OpenAQ bout-à-bout (1 batch) | < 60 s |
| Latence requête timeseries (30 j, horaire) | < 200 ms p95 (ClickHouse) |
| Latence push WebSocket (alerte → client) | < 1 s après match |
| Fraîcheur des données (âge de la dernière mesure) | < 1 h en médiane |
| Couverture endpoints REST | 16+ exposés, OpenAPI à jour |
| Disponibilité API | ≥ 99,5 % mensuel |

## 5. Ce qui n'est **pas** dans le scope

- Pas de capteurs physiques propres : on consomme OpenAQ, on ne déploie pas de matériel.
- Pas d'application mobile native (le responsive web suffit).
- Pas de paiement réel : les plans d'abonnement sont modélisés, le flow Stripe n'est pas implémenté — c'est une perspective d'évolution.
- Pas de prévision/IA produit au départ : la prévision de pollution et la détection d'anomalies sont des perspectives (et seraient documentées séparément dans `docs/`).
