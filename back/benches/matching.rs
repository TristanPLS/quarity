//! Micro-benchmarks du hot path de matching (C2). Index réaliste (1k/5k/10k règles
//! réparties sur 1000 stations × 6 polluants), puis :
//!   - `lookup()`  : résolution `(station, polluant) → règles` (O(1), zéro E/S) ;
//!   - `evaluate()`: évaluation d'un lot de 500 mesures contre l'index.
//!
//! NB : micro-bench process-local, lancé à la demande (`cargo bench --bench matching`).
//! PAS un gate CI — `clippy --all-targets` le compile (donc clippy-clean obligatoire),
//! mais aucune valeur de perf n'est imposée ici (cf. docs/benchmarks.md pour le protocole
//! et les seuils US-03 < 5 ms/mesure et US-06 timeseries p95 < 200 ms).

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use quarity_back::ch::NewMeasurementRow;
use quarity_back::matching::{evaluate, Comparator, RuleIndex, StationRule};

const PARAMS: [&str; 6] = ["pm25", "pm10", "no2", "o3", "so2", "co"];
const STATIONS: i64 = 1000;

/// `n` règles synthétiques réparties sur `STATIONS` stations × 6 polluants.
fn make_index(n: usize) -> RuleIndex {
    let rules = (0..n)
        .map(|i| StationRule {
            rule_id: i as i64,
            org_id: 1,
            tracked_location_id: i as i64,
            ref_location_id: i as i64,
            openaq_location_id: (i as i64) % STATIONS,
            parameter: PARAMS[i % PARAMS.len()].to_string(),
            comparator: Comparator::Gt,
            threshold: 50.0,
            severity: "warning".to_string(),
        })
        .collect();
    RuleIndex::from_station_rules(rules)
}

/// Lot de `n` mesures, chacune au-dessus du seuil (déclenche un match si une règle existe).
fn make_batch(n: usize) -> Vec<NewMeasurementRow> {
    (0..n)
        .map(|i| NewMeasurementRow {
            location_id: ((i as i64) % STATIONS) as u64,
            sensor_id: i as u64,
            parameter: PARAMS[i % PARAMS.len()].to_string(),
            unit: "ug/m3".to_string(),
            measured_at: "2026-06-01 12:00:00".to_string(),
            value: 60.0,
            last_ingested_at: "2026-06-01 12:00:01".to_string(),
        })
        .collect()
}

fn bench_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("lookup");
    for &n in &[1_000usize, 5_000, 10_000] {
        let index = make_index(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| black_box(index.lookup(black_box(42), black_box("pm25"))));
        });
    }
    group.finish();
}

fn bench_evaluate(c: &mut Criterion) {
    let batch = make_batch(500);
    let mut group = c.benchmark_group("evaluate_500_rows");
    for &n in &[1_000usize, 5_000, 10_000] {
        let index = make_index(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| black_box(evaluate(black_box(&index), black_box(&batch))));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_lookup, bench_evaluate);
criterion_main!(benches);
