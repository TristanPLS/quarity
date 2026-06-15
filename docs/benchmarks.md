# Benchmarks de performance — Quarity (Jalon 4 · C2)

> Deux volets : (1) **micro-benchmark du hot path de matching** (criterion, reproductible
> par `cargo bench`) qui prouve l'objectif **US-03 < 5 ms/mesure** ; (2) **protocole de charge
> p95 des séries temporelles** (US-06 < 200 ms p95) avec hypothèses, à exécuter sur l'env démo.

## 1. Matching — micro-benchmark (criterion)

- **Quoi** : `back/benches/matching.rs` mesure le cœur du moteur (`back/src/matching.rs`) :
  - `lookup()` — résolution `(station OpenAQ, polluant) → règles` dans l'index mémoire (`HashMap`) ;
  - `evaluate()` — évaluation d'un lot de **500 mesures** contre l'index (lookup + comparaison + snapshot des dépassements).
- **Index** : 1 000 / 5 000 / 10 000 règles synthétiques réparties sur 1 000 stations × 6 polluants.
- **Reproduire** : `cd back && cargo bench --bench matching` (rapport HTML sous `target/criterion/`).
- **Environnement de mesure** : poste de dev (Windows 11, build `--release`). Valeurs **indicatives** (la machine de prod diffère) — ce qui compte est l'**ordre de grandeur** et la **forme** (O(1)).

### Résultats (médiane criterion, 100 échantillons)

| Bench | 1 000 règles | 5 000 règles | 10 000 règles |
|---|---|---|---|
| `lookup` (1 résolution) | **37.6 ns** | 40.6 ns | **37.5 ns** |
| `evaluate` (500 mesures) | 157.8 µs | 202.5 µs | 294.6 µs |
| **→ par mesure** (`evaluate`/500) | **0.32 µs** | 0.40 µs | **0.59 µs** |

### Interprétation

- **`lookup` est O(1)** : ~37–41 ns quel que soit le nombre de règles (table de hachage). La taille du parc de règles n'impacte pas la résolution.
- **Coût par mesure ≈ 0.3–0.6 µs**, même à **10 000 règles**. Budget cible **US-03 : < 5 ms/mesure (5 000 µs)** → **marge ≈ 8 000–15 000×**. La légère croissance avec le nombre de règles vient de la densité (plus de règles par `(station, polluant)` → plus de comparaisons et de dépassements à snapshotter), pas du lookup.
- ✅ **Objectif US-03 (« < 5 ms / mesure », matching temps réel) largement tenu** — cohérent avec le choix d'architecture « index compilé en mémoire + cache Moka L1 » (cf. `back/src/matching.rs`).

> Le bench n'est **pas un gate CI** (aucune valeur de perf imposée), mais `clippy --all-targets`
> le compile à chaque PR → il reste clippy-clean. Lancement à la demande.

## 2. Séries temporelles — protocole de charge p95 (US-06)

> Cible : **`GET /api/measurements` p95 < 200 ms** sur une fenêtre 30 j horaire. Exécution réelle
> sur l'**env démo** (7d, stack déployée + ClickHouse seedé) — ce protocole en fige les hypothèses.

### Hypothèses de charge

- **Outil** : `oha` (ou `k6`), exécuté depuis une machine proche réseau de l'API.
- **Données** : station réelle ingérée (`location_id=4085`, NICE PROMENADE) + seed démo ; au moins 30 j de mesures horaires en base ClickHouse.
- **Endpoint** : `GET /api/measurements?location_id=4085&parameter=pm25&from=<J-30>&to=<J>&page_size=1000` avec un **JWT valide** (`Authorization: Bearer …`).
- **Charge** : paliers **10 / 50 / 100 VUs** (utilisateurs concurrents), **60 s** par palier, montée progressive.
- **Mesures** : latences **p50 / p95 / p99**, débit (req/s), taux d'erreur (doit rester 0 hors 4xx attendus).

### Commande de référence (`oha`)

```sh
# JWT recupere via POST /api/auth/login (sophie@agglo-riviera.fr / Quarity2026!)
TOKEN=...
oha -z 60s -c 50 \
  -H "Authorization: Bearer $TOKEN" \
  "http://<host>:8080/api/measurements?location_id=4085&parameter=pm25&from=2026-05-01&to=2026-05-31&page_size=1000"
```

### Tableau à remplir (exécution sur l'env démo)

| VUs | p50 | p95 | p99 | req/s | erreurs | Verdict (< 200 ms p95) |
|---|---|---|---|---|---|---|
| 10  | _à mesurer_ | _à mesurer_ | _à mesurer_ | _ | _ | _ |
| 50  | _à mesurer_ | _à mesurer_ | _à mesurer_ | _ | _ | _ |
| 100 | _à mesurer_ | _à mesurer_ | _à mesurer_ | _ | _ | _ |

### Atouts attendus côté lecture

- ClickHouse `measurements` : `ORDER BY (location_id, parameter, measured_at)` + index data-skipping `minmax(measured_at)` → la requête par station/polluant/plage élague fortement.
- Allowlist stricte des paramètres (`parameter`/`from`/`to` validés au boundary, anti-injection) → pas de scan non borné.
- Pagination (`page_size` ≤ 1000, borné par validation) → réponse bornée.

## 3. Synthèse

| Objectif | Cible | Résultat |
|---|---|---|
| US-03 — matching temps réel | < 5 ms / mesure | ✅ **~0.3–0.6 µs/mesure** (micro-bench criterion, marge ~10 000×) |
| US-06 — séries temporelles | p95 < 200 ms | ⏳ protocole figé (ci-dessus) — mesure sur l'env démo (7d) |
