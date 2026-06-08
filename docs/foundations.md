# Fondations — risques juridiques & données de Quarity

> Ce document **tranche les angles morts de fondation** avant d'investir dans le Jalon 3.
> Il porte sur 4 sujets qui conditionnent la **viabilité d'un produit payant B2B/B2G** : droit
> d'usage OpenAQ, RGPD, responsabilité des alertes sanitaires, qualité des données.
>
> ⚠️ **Ce n'est pas un avis juridique.** Chaque section propose une **décision par défaut** +
> les points **« à confirmer »** avant tout encaissement réel (revue par un·e juriste).
> Périmètre supposé : clients en **France / UE** (collectivités, établissements).
> Dernière mise à jour : 2026-06-08 (§1 et §4 resynchronisés : références fichiers post-migrations sqlx, statut D4.2/B5 figé).

---

## 1. Droit d'usage des données OpenAQ

### Constat (sourcé — voir §Sources)
- **OpenAQ est un agrégateur**, pas un producteur : chaque mesure provient d'une **source/fournisseur** qui impose **sa propre licence** (Creative Commons, licence ouverte gouvernementale, ou termes spécifiques).
- L'API v3 expose la licence **par source** via la ressource `/v3/licenses`, avec les drapeaux :
  `commercialUseAllowed`, `attributionRequired`, `shareAlikeRequired`, `modificationAllowed`,
  `redistributionAllowed`, `sourceUrl`.
- Les conditions OpenAQ précisent : *« Users of the OpenAQ platform are solely responsible for their
  use of the data and for compliance with any applicable laws and third-party terms. »* → **la conformité
  par-source nous incombe**, pas à OpenAQ.
- **Attribution obligatoire** à OpenAQ **et** aux sources d'origine (selon les termes de chaque fournisseur).
- Clause concurrentielle : interdit d'utiliser l'**API hébergée** pour bâtir un produit qui *« substantially
  duplicate or directly compete with OpenAQ's core offerings »*. Quarity (alerte/supervision pour des orgs)
  **ne concurrence pas** la plateforme de données ouvertes d'OpenAQ → risque faible, mais à garder en tête.

### Implications pour un produit **payant**
- On ne peut **pas** présumer que toute mesure OpenAQ est utilisable commercialement : certaines sources ont
  `commercialUseAllowed = false` ou `shareAlikeRequired = true` (le **share-alike** est **incompatible** avec un
  produit propriétaire payant — il forcerait à repartager les dérivés sous la même licence).
- **Manque actuel** : l'ingestion (`back/src/bin/ingest.rs`) ne récupère **aucune** info de licence, et
  `ref_locations` (`back/migrations/0001_init.sql`) n'a **aucune** colonne de licence/attribution.

### Décisions proposées
- **D1.1 — Capturer la licence à l'ingestion.** Interroger `/v3/licenses` + la licence de chaque station et
  stocker sur `ref_locations` : `license_id`, `commercial_use_allowed (bool)`, `attribution_required (bool)`,
  `share_alike_required (bool)`, `attribution_name (text)`, `license_source_url (text)`.
- **D1.2 — Filtrer à la source.** Par défaut, n'ingérer/servir que les stations où
  `commercial_use_allowed = true` **et** `share_alike_required = false`. Les autres : exclues du produit payant
  (ou cantonnées à un usage interne/démo, jamais facturé), tant qu'un accord explicite n'est pas obtenu.
- **D1.3 — Attribuer.** Écran **« Sources & licences »** + attribution **OpenAQ + fournisseur** visible sur les
  vues de données et les exports (mention par station). Conserver `attribution_name`/`license_source_url` jusqu'au
  rendu front.
- **D1.4 — Tracer la provenance.** Garder le lien station → licence pour pouvoir prouver la conformité a posteriori.

### À confirmer
- Revue juridique **avant facturation**. Pour les sources non-commerciales mais stratégiques (ex. une AASQA),
  envisager une **demande d'autorisation directe** au fournisseur.
- Décider si le filtre D1.2 est **strict** (exclusion) ou **par palier d'offre** (commercial = sources libres ;
  interne = tout).

---

## 2. RGPD / données personnelles

### Inventaire des données personnelles (d'après le schéma réel)
| Donnée | Où | Sensibilité |
|---|---|---|
| Email, nom complet, hash mot de passe | `users` | Identifiant direct |
| Créateur de jeton | `api_tokens.created_by` | Indirect |
| Destinataires d'alerte (email, user) | `alert_rule_recipients` | Direct |
| Cible d'envoi (email) | `notification_deliveries.target` | Direct |
| Acteur d'audit | `audit_log.actor_user_id` | Indirect |
| Contacts B2G (collectivités) | `organizations` + users | Direct, contexte public |

> Les **mesures** (ClickHouse) sont des données **environnementales**, **non personnelles** — leur TTL 90j
> ne relève **pas** du RGPD. À ne pas confondre avec la rétention des données personnelles (ci-dessous).

### Décisions proposées
- **D2.1 — Rôles & base légale.** Quarity = **responsable de traitement** pour les comptes/users ; potentiellement
  **sous-traitant** pour certaines données clients → **DPA** (accord de traitement) à fournir aux clients B2G/B2B.
  Bases légales : **contrat** (compte), **intérêt légitime** (alerte/supervision), **consentement** (marketing).
- **D2.2 — Rétention distincte du TTL mesures.** Politique écrite : données de compte conservées tant que le
  compte est actif + délai légal ; logs d'audit (`audit_log`) durée définie ; **purge** à la suppression de compte.
  (≠ TTL 90j des mesures, qui reste tel quel.)
- **D2.3 — Droits des personnes.** Process (et à terme endpoints) pour **accès, rectification, effacement,
  portabilité** ; registre des demandes.
- **D2.4 — Registre des traitements (Art. 30)** maintenu ; **politique de confidentialité** publiée (page dédiée).
- **D2.5 — Sécurité.** Argon2id (déjà en place), chiffrement en transit (HTTPS), **RBAC appliqué** (cf. backlog
  B13 — aujourd'hui le rôle est dans le JWT mais non vérifié), **journal d'audit applicatif**, procédure de
  **notification de violation** (72 h).

### À confirmer
- **DPO** : non obligatoire a priori, mais recommandé vu la cible B2G + contexte sanitaire — à arbitrer.
- Hébergement des données **dans l'UE** (vérifier la localisation de l'infra de déploiement).

---

## 3. Responsabilité & disclaimer (alertes sanitaires)

### Risque
Émettre des alertes de qualité de l'air à des **écoles / hôpitaux** sans cadrage = **exposition juridique** si une
décision est prise sur une donnée fausse, manquante ou périmée. Les mesures OpenAQ sont **indicatives**, pas un
dispositif réglementaire certifié.

### Décisions proposées
- **D3.1 — Disclaimer visible partout.** Mention type, sur **chaque alerte, le dashboard et les exports** :
  > *« Données indicatives, non certifiées. Ne se substituent pas aux dispositifs de surveillance réglementaire
  > (ex. AASQA / Atmo en France, AEE en Europe). »*
- **D3.2 — Pas de SLA d'exactitude.** Un éventuel SLA porte sur la **disponibilité du service**, jamais sur
  l'**exactitude** ou l'exhaustivité des mesures.
- **D3.3 — CGU / limitation de responsabilité.** Clause de CGU encadrant l'usage et limitant la responsabilité
  (rédaction juridique).
- **D3.4 — Renvoi aux sources officielles** (Atmo France, AEE) pour l'information faisant foi.

### À confirmer
- Rédaction des **CGU** et de la clause de responsabilité par un·e juriste ; opportunité d'une **assurance RC pro**.

---

## 4. Qualité & hétérogénéité des données (pré-requis au calcul AQI)

### Constat *(statut au 2026-06-08 — D4.2 soldée par B5)*
- **Unités hétérogènes** : OpenAQ mélange `µg/m³` et `ppb` selon polluant/source. **✅ Résolu (D4.2, PR #15,
  2026-06-07)** : l'ingestion (`ingest.rs`) applique des **unités strictes** — toute mesure **sans unité** est
  **ignorée et comptée** (plus **aucun** fallback `µg/m³` silencieux). L'unité brute est stockée telle quelle ;
  la conversion `ppb ↔ µg/m³` est faite **au calcul AQI** (jamais à l'ingestion).
- **✅ Incohérence corrigée (2026-06-07)** : le `COMMENT` sur `aqi_breakpoints.unit` — désormais dans
  `back/migrations/0001_init.sql` (le schéma Postgres a migré vers sqlx ; `db/sql/01_schema.sql` n'est plus qu'un
  **pointeur** sans DDL) — est en accord avec le seed et `data-model.md §B` : paliers O₃/NO₂ en `ppb`, distincts
  de l'unité OpenAQ `µg/m³`, conversion explicite au calcul.
- Pour des **alertes sanitaires**, une **valeur fausse silencieuse** (mauvaise conversion d'unité) reste le **pire**
  mode de défaillance — d'où la stricte séparation unité source / unité de calcul ci-dessus.

### Décisions proposées
- **D4.1 — Couche de validation/normalisation à l'ingestion** (avant le calcul AQI du Jalon 3) : allowlist
  d'unités **par polluant**, **conversion explicite** vers l'unité canonique, **rejet/flag** des valeurs hors plage
  physique (négatives, aberrantes), gestion des trous.
- **D4.2 — Lever l'ambiguïté `aqi_breakpoints.unit`** : reformuler le `COMMENT`, stocker l'unité **source** + l'unité
  **canonique**, convertir **au calcul** (jamais comparer deux unités différentes).
- **D4.3 — Fraîcheur & couverture** : exposer `last_seen` / données manquantes pour ne **pas** déclencher d'alerte
  sur des mesures **périmées**.

### À confirmer
- Table de conversion de référence (ppb↔µg/m³ dépend de la masse molaire et des conditions T/P) — figer les
  hypothèses (ex. 25 °C, 1 atm) et les documenter.
  **✅ Figé le 2026-06-07 (B5)** : 25 °C / 1 atm, volume molaire Vm = 24.45 L/mol — `ppb = µg/m³ × 24.45 / M`,
  avec M(O₃) = 48.00 et M(NO₂) = 46.01 g/mol. Documenté et appliqué dans
  `db/clickhouse/queries/rolling_regulatory.sql` (Q2 `aqi_snapshot`).

---

## 5. Synthèse — matrice d'action

| # | Décision | Atterrit dans | Priorité |
|---|---|---|---|
| D1.1–D1.4 | Licence OpenAQ : capture + filtre + attribution | `ref_locations` (schéma), `ingest.rs`, front « Sources » | **Avant facturation** |
| D2.1–D2.5 | RGPD : DPA, rétention, droits, registre, RBAC/audit | back (RBAC/audit), doc, page confidentialité | **Avant données réelles de clients** |
| D3.1–D3.4 | Disclaimer + CGU responsabilité | front (toutes vues d'alerte), CGU | **Avant 1ʳᵉ alerte à un vrai établissement** |
| D4.1–D4.3 | Normalisation unités + cohérence AQI | `ingest.rs`, `db/sql` (AQI), back | **Avant le calcul AQI (Jalon 3 B5)** |

> **Règle d'or** : ces 4 points sont des **pré-requis**, pas des features. Les trancher maintenant coûte peu ;
> les rétro-fitter après avoir bâti le produit dessus coûte cher.

---

## Sources (OpenAQ, consultées le 2026-06-05)
- Terms of use — https://docs.openaq.org/about/terms
- Licenses (ressource & drapeaux par source) — https://docs.openaq.org/resources/licenses
- Aide & FAQ développeurs — https://openaq.org/developers/help/
