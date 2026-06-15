# Log — agent-back — 2026-06-15 — fix CI Docker (COPY benches) — correctif PR #79 (7b)

## CEST — Dockerfile : COPY benches/ (parse du manifeste avec [[bench]])

- **Agent / rôle** : agent-back
- **Contexte** : la PR #79 (7b, benchmarks C2) a fait **planter le job CI Docker** : `back/Dockerfile` builder fait `cargo build --release`, mais `Cargo.toml` déclare désormais `[[bench]] name="matching"` → cargo **valide l'existence de `benches/matching.rs` au parse du manifeste** (avant toute compilation), même pour `cargo build --release`. Or le Dockerfile ne `COPY` que `Cargo.*`/`src`/`migrations`, **pas `benches/`** → `error: can't find matching bench at benches/matching.rs`.
- **Cause profonde** : récurrence de la **leçon B10** (contexte de build Docker = sous-ensemble de `back/`). Ma vérif locale (`cargo clippy/build` dans `back/` complet) ne pouvait PAS le voir — `benches/` y existe ; seul le contexte Docker partiel échoue.
- **Action** : `back/Dockerfile` builder — ajout de `COPY benches ./benches` (avant `cargo build --release`) + commentaire explicatif. `cargo build --release` ne **compile pas** le bench (ni `criterion`, dev-dep) — il a juste besoin que le fichier existe pour le parse.
- **Fichiers touchés** : `back/Dockerfile` (modifié).
- **Vérif (fidèle, dans un contexte reproduisant les COPY du Dockerfile)** :
  - SANS `benches/` → `cargo build --release` échoue **immédiatement** avec l'erreur EXACTE de la CI.
  - AVEC `benches/` → passe le parse, `Compiling …` démarre. ✅
  - (Le 1er essai via `cargo metadata` était un FAUX positif : metadata ne valide pas l'existence des targets explicites comme `cargo build`.)
- **Livraison** : nouveau commit sur la branche **existante** `perf/c2-benchmarks-matching` → push → met à jour PR #79 → CI re-déclenchée (Docker doit passer).
- **Leçon persistée** : tout nouveau **target cargo déclaré** (`[[bench]]`/`[[example]]`) doit être `COPY`é dans le Dockerfile, OU vérifié dans le contexte Docker (pas seulement `back/` complet).
