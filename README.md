# Quarity

> Surveillance et alerte temps réel sur la qualité de l'air mondiale (données OpenAQ).

Quarity permet à une **collectivité**, une **autorité sanitaire**, un **établissement** (école, hôpital) ou un **grand compte** de définir des **lieux suivis** (sites, polluants, seuils réglementaires) et de recevoir des **alertes temps réel** dès qu'un dépassement est détecté dans le flux de mesures OpenAQ.

## Stack

| Couche | Choix | Pourquoi |
|---|---|---|
| Back | Rust + Axum | Performance + sûreté mémoire pour la boucle de matching de seuils (sub-5 ms *par construction* — lookup mémoire pur ; benchmark formel → C2, Jalon 4) |
| Front | React + Vite | DX moderne, build rapide, écosystème mature |
| BDD OLTP | PostgreSQL 16 | Users, orgs, lieux suivis, règles d'alerte, profils d'exposition, abonnements (3NF, contraintes fortes) |
| BDD analytics | ClickHouse 24.x | Mesures OpenAQ — stockage columnar, agrégations time-series, downsampling, rollups |
| Cache L1 | Moka | In-process, sub-ms lookup pour les règles de seuils compilées *(livré le 2026-06-10 — B7, PR #35 : boucle de matching de seuils branchée)* |
| Cache L2 / Pub-Sub | Redis 7 | Sessions, refresh tokens, rate-limit, push WebSocket |
| Orchestration | Docker Compose | Boot en une commande |

## Prérequis

- Docker 25+ / Docker Compose v2
- Ports libres : `3000` (front), `8080` (back), `5432` (Postgres), `8123`/`9000` (ClickHouse), `6379` (Redis)
- 8 Go RAM minimum (4 Go pour ClickHouse seul)
- Une clé API OpenAQ (gratuite — https://openaq.org/), placée dans `.env`

## Lancement

```bash
cp .env.example .env
# Générer un vrai JWT_SECRET — le back REFUSE de démarrer avec le placeholder du .env.example :
openssl rand -hex 32          # coller la valeur dans JWT_SECRET=… du .env
# Renseigner aussi OPENAQ_API_KEY (clé gratuite https://openaq.org/) pour l'ingestion.
docker compose up -d
docker compose --profile seed run --rm seed     # jeu de démo (orgs, lieux, règles, profils)
```

**Périmètre fonctionnel des 3 jalons livré** : CRUD complet (B6, ~44 endpoints REST), boucle de matching temps réel (B7, cache Moka L1), alertes WebSocket (B8/B8b, `/api/ws`) **consommées par le client front temps réel** (B11a), **carte + jauges AQI** (B10/B10b), **profils d'exposition + calcul de dose** (B9a), **API publique par clé** (B9b), **CRUD front** lieux/règles/profils/expositions + **responsive** (B11b/c). Le **Jalon 4** (recette, benchmarks, déploiement, tag `v1.0`) reste à finaliser. Détail : [`docs/backlog.md`](docs/backlog.md) · plan : [`roadmap.md`](roadmap.md).

- Front : http://localhost:3000
- Doc API (Swagger / OpenAPI via utoipa) : http://localhost:3000/api/docs *(servie par le back, proxy nginx `/api`)*

### Comptes de démo

Après le seed (mot de passe commun de démo `Quarity2026!`) :

| Compte | Rôle | Organisation |
|---|---|---|
| `sophie@agglo-riviera.fr` | admin | Agglo Riviera (org A) |

Station réelle ingérable : `location_id=4085` (NICE PROMENADE) — `docker compose --profile ingest run --rm ingest 4085`.

## Équipe

Projet mené **en solo** par Tristan (tous les axes : BDD, back, front, conception/UX-doc), assisté d'agents IA (Claude Code — charte interne, hors dépôt). La review humaine des PR est remplacée par la **CI obligatoire** (3 checks requis avant merge).

## Liens

- 📋 Board (GitHub Projects) : https://github.com/users/TristanPLS/projects/1
- 🗺️ Roadmap produit : [roadmap.md](roadmap.md)
- 🧭 Pitch produit : [docs/pitch.md](docs/pitch.md)
- 🎨 Identité visuelle : [docs/identity.md](docs/identity.md)
- 🤖 Charte des agents : interne (non publiée)
- 🧾 Journal d'actions des agents : [logs/](logs/)

## Contribuer

Règles non-négociables : **Conventional Commits**, **PR obligatoire** avec **CI verte** (3 checks requis), **pas de commit direct** sur `master` ni `dev`.

> Les actions Git/GitHub sont effectuées **par les humains**. Les agents IA préparent le travail et **signalent** la commande à lancer — ils ne committent ni ne poussent jamais.

## Licence

Propriétaire — produit B2B/B2G. Licence à confirmer avant tout déploiement public.
