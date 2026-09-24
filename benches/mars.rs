//! Engine throughput, in executed instructions per second.
//!
//!   cargo bench --bench mars
//!
//! Workloads:
//! * `corpus` — a round robin of real warriors (testdata/ plus pMARS's own
//!   warriors/ when third_party/pmars is present), the hill's actual work;
//! * `dwarf_vs_imp` — long battles that mostly run to the cycle limit: the
//!   steady-state interpreter loop;
//! * `edge_vs_dwarf` — battles that end at once: per-round setup;
//! * `random` — generated warriors with every opcode, modifier and mode.
//!
//! Each on the fast engine and on the plain reference (`Mars`), so a change
//! to one shows against the other.

use corewar::asm::{assemble, Config};
use corewar::fast::{Compiled, Engine};
use corewar::mars::Mars;
use corewar::redcode::{Mode, Modifier, Opcode, Warrior};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

fn load(path: &str, cfg: &Config) -> Option<Warrior> {
    assemble(&std::fs::read_to_string(path).ok()?, cfg).ok()
}

fn random_warrior(seed: &mut u64, cfg: &Config) -> Warrior {
    let mut next = || {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    };
    let len = 1 + next() % 10;
    let mut src = String::from(";name r\n");
    for i in 0..len {
        let op = Opcode::ALL[(next() % Opcode::ALL.len() as u64) as usize];
        let m = Modifier::ALL[(next() % 7) as usize];
        let am = Mode::ALL[(next() % 8) as usize];
        let bm = Mode::ALL[(next() % 8) as usize];
        let f = |x: u64| (x % 21) as i64 - 10;
        src.push_str(&format!(
            "{} {}.{} {}{}, {}{}\n",
            if i == 0 { "top" } else { "" },
            op.name(),
            m.name(),
            am.symbol(),
            f(next()),
            bm.symbol(),
            f(next())
        ));
    }
    assemble(&src, cfg).unwrap()
}

type Pairs = Vec<(Warrior, Warrior)>;

fn workloads(cfg: &Config) -> Vec<(&'static str, Pairs, u32)> {
    let corpus: Vec<Warrior> = [
        "testdata/dwarf.red",
        "testdata/imp.red",
        "third_party/pmars/warriors/aeka.red",
        "third_party/pmars/warriors/flashpaper.red",
        "third_party/pmars/warriors/pspace.red",
        "third_party/pmars/warriors/rave.red",
        "third_party/pmars/warriors/validate.red",
    ]
    .iter()
    .filter_map(|p| load(p, cfg))
    .collect();
    let mut rr = Vec::new();
    for i in 0..corpus.len() {
        for j in i + 1..corpus.len() {
            rr.push((corpus[i].clone(), corpus[j].clone()));
        }
    }
    let dwarf = load("testdata/dwarf.red", cfg).unwrap();
    let imp = load("testdata/imp.red", cfg).unwrap();
    let edge = load("testdata/edge.red", cfg).unwrap();
    let mut seed = 0x9e3779b97f4a7c15u64;
    let randoms = (0..20)
        .map(|_| {
            (
                random_warrior(&mut seed, cfg),
                random_warrior(&mut seed, cfg),
            )
        })
        .collect();
    vec![
        ("corpus", rr, 4),
        ("dwarf_vs_imp", vec![(dwarf.clone(), imp)], 20),
        ("edge_vs_dwarf", vec![(edge, dwarf)], 200),
        ("random", randoms, 20),
    ]
}

fn bench(c: &mut Criterion) {
    let cfg = Config {
        rounds: 20,
        ..Config::default()
    };
    let mut g = c.benchmark_group("mars");
    g.sample_size(20);
    for (name, pairs, rounds) in workloads(&cfg) {
        let compiled: Vec<(Compiled, Compiled)> = pairs
            .iter()
            .map(|(a, b)| (Compiled::new(a), Compiled::new(b)))
            .collect();
        let mut probe = Engine::new(&cfg);
        for (k, (a, b)) in compiled.iter().enumerate() {
            probe.play(&cfg, a, b, rounds, k as i32 + 1);
        }
        g.throughput(Throughput::Elements(probe.steps));
        g.bench_function(BenchmarkId::new("fast", name), |bch| {
            let mut e = Engine::new(&cfg);
            bch.iter(|| {
                for (k, (a, b)) in compiled.iter().enumerate() {
                    e.play(&cfg, a, b, rounds, k as i32 + 1);
                }
            })
        });
        g.bench_function(BenchmarkId::new("reference", name), |bch| {
            let mut m = Mars::new(&cfg, 2);
            bch.iter(|| {
                for (k, (a, b)) in pairs.iter().enumerate() {
                    m.play(&cfg, a, b, rounds, k as i32 + 1);
                }
            })
        });
    }
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
