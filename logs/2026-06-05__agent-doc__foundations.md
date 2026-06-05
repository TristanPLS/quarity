# Log — agent-doc — 2026-06-05 — foundations

## 14:05 CEST — docs/foundations.md : risques de fondation tranchés (OpenAQ, RGPD, responsabilité, données)

- **Agent / rôle** : agent-doc (axe doc ; transverse juridique/données)
- **Jalon / tâche** : Durcissement post-Jalon 2 — recommandation prioritaire #3. Couvre les angles morts D1–D3 (section D du backlog) + le volet RGPD (B12/B13).
- **Contexte** : produit revendiqué B2B/B2G **payant** sur données OpenAQ + alertes sanitaires, sans aucun cadrage juridique/données dans le dépôt. Ce sont des **risques bloquants**, pas des features — à trancher avant le Jalon 3.
- **Actions** :
  - **Recherche sourcée** de la licence/ToS OpenAQ (pas de mémoire) : OpenAQ est un **agrégateur**, licences **par source** exposées via l'API `/v3/licenses` (`commercialUseAllowed`, `attributionRequired`, `shareAlikeRequired`, `modificationAllowed`, `redistributionAllowed`). Conformité par-source à la charge de l'utilisateur ; attribution OpenAQ + fournisseur obligatoire.
  - Rédaction de `docs/foundations.md` (4 sections, décisions numérotées D1.x–D4.x + « à confirmer ») :
    1. **Droit d'usage OpenAQ** : capture de licence à l'ingestion (colonnes sur `ref_locations`), filtre `commercial_use_allowed`/`share_alike`, écran « Sources & licences ».
    2. **RGPD** : inventaire des données perso (sur le schéma réel), DPA B2G, rétention ≠ TTL 90j des mesures, droits des personnes, registre, RBAC/audit.
    3. **Responsabilité** : disclaimer « données indicatives, non certifiées », pas de SLA d'exactitude, CGU.
    4. **Qualité des données** : normalisation d'unités (ppb/µg/m³) avant AQI, levée de l'incohérence `aqi_breakpoints.unit`, fraîcheur.
  - Matrice d'action finale (décision → où ça atterrit → priorité).
  - Pointeur ajouté dans `docs/backlog.md` (section D) vers `foundations.md`.
- **Fichiers touchés** :
  - `docs/foundations.md` (créé)
  - `docs/backlog.md` (modifié — pointeur vers foundations)
- **Résultat** : OK — document de décision livré. **Ce n'est pas un avis juridique** : revue par un·e juriste requise avant facturation réelle (explicitement marqué dans le doc).
- **Vérifs** : sources OpenAQ citées (docs.openaq.org/about/terms, /resources/licenses) ; inventaire RGPD croisé avec `db/sql/01_schema.sql` (users, api_tokens, alert_rule_recipients, notification_deliveries, audit_log).
- **Prochaine étape** : recommandation #4 — rejet du `JWT_SECRET` placeholder au boot (`back/src/config.rs`).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Branche cible : docs/foundations (depuis `dev`)
  > 1) git switch dev && git pull --ff-only && git switch -c docs/foundations
  > 2) git add docs/foundations.md docs/backlog.md
  > 3) git commit -m "docs(doc): foundations.md — droit d'usage OpenAQ, RGPD, responsabilité, qualité données"
  > 4) git push -u origin docs/foundations
  > Puis : PR vers `dev`, 1 reviewer, squash merge.
  > ─────────────────────────────────────────────
