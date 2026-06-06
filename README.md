# Quarity

> Surveillance et alerte temps réel sur la qualité de l'air mondiale (données OpenAQ).

Quarity permet à une **collectivité**, une **autorité sanitaire**, un **établissement** (école, hôpital) ou un **grand compte** de définir des **lieux suivis** (sites, polluants, seuils réglementaires) et de recevoir des **alertes temps réel** dès qu'un dépassement est détecté dans le flux de mesures OpenAQ.

## Stack

| Couche | Choix | Pourquoi |
|---|---|---|
| Back | Rust + Axum | Performance + sûreté mémoire pour la boucle de matching de seuils sub-5 ms |
| Front | React + Vite | DX moderne, build rapide, écosystème mature |
| BDD OLTP | PostgreSQL 16 | Users, orgs, lieux suivis, règles d'alerte, profils d'exposition, abonnements (3NF, contraintes fortes) |
| BDD analytics | ClickHouse 24.x | Mesures OpenAQ — stockage columnar, agrégations time-series, downsampling, rollups |
| Cache L1 | Moka | In-process, sub-ms lookup pour les règles de seuils compilées |
| Cache L2 / Pub-Sub | Redis 7 | Sessions, refresh tokens, rate-limit, push WebSocket |
| Orchestration | Docker Compose | Boot en une commande |

## Prérequis

- Docker 25+ / Docker Compose v2
- Ports libres : `3000` (front), `8080` (back), `5432` (Postgres), `8123`/`9000` (ClickHouse), `6379` (Redis)
- 8 Go RAM minimum (4 Go pour ClickHouse seul)
- Une clé API OpenAQ (gratuite — https://openaq.org/), placée dans `.env`

## Lancement

```bash
cp .env.example .env   # .env.example livré au Jalon 2
docker compose up -d
```

Une fois le walking skeleton (Jalon 2) livré :

- Front : http://localhost:3000
- Doc API (Swagger / OpenAPI via utoipa) : http://localhost:8080/api/docs

## Équipe

| Rôle | Owner |
|---|---|
| Lead BDD (Postgres + ClickHouse) | Tristan |
| Lead Back (Rust / Axum) | *TBD* |
| Lead Front (React / Vite) | *TBD* |
| Lead Conception / UX-doc | *TBD* |
| Agents IA (Claude Code) | charte interne (hors dépôt) |

## Liens

- 📋 Board (GitHub Projects) : https://github.com/users/TristanPLS/projects/1
- 🗺️ Roadmap produit : [roadmap.md](roadmap.md)
- 🧭 Pitch produit : [docs/pitch.md](docs/pitch.md)
- 🎨 Identité visuelle : [docs/identity.md](docs/identity.md)
- 🤖 Charte des agents : interne (non publiée)
- 🧾 Journal d'actions des agents : [logs/](logs/)

## Contribuer

Règles non-négociables : **Conventional Commits**, **PR obligatoire** avec 1 reviewer minimum, **pas de commit direct** sur `master` ni `dev`.

> Les actions Git/GitHub sont effectuées **par les humains**. Les agents IA préparent le travail et **signalent** la commande à lancer — ils ne committent ni ne poussent jamais.

## Licence

Propriétaire — produit B2B/B2G. Licence à confirmer avant tout déploiement public.
