//! Engine throughput, in executed instructions per second.
//!
//!   cargo bench --bench mars
//!
//! Three workloads, because they stress different things:
//! * dwarf vs imp — long battles that mostly run to the cycle limit (ties):
//!   the steady-state interpreter loop;
//! * edge vs dwarf — battles that end quickly: loading and setup per round;
//! * random — generated warriors with every opcode, modifier and mode, fixed
//!   seed: the dispatch as a whole.

use corewar::asm::{assemble, Config};
use corewar::mars::Mars;
use corewar::redcode::{Mode, Modifier, Opcode, Warrior};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

fn load(path: &str, cfg: &Config) -> Warrior {
    assemble(&std::fs::read_to_string(path).unwrap(), cfg).unwrap()
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
        let op = Opcode::ALL[(next() % 16) as usize];
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

fn bench(c: &mut Criterion) {
    let cfg = Config::default();
    let dwarf = load("testdata/dwarf.red", &cfg);
    let imp = load("testdata/imp.red", &cfg);
    let edge = load("testdata/edge.red", &cfg);
    let mut seed = 0x9e3779b97f4a7c15u64;
    let randoms: Vec<(Warrior, Warrior)> = (0..20)
        .map(|_| {
            (
                random_warrior(&mut seed, &cfg),
                random_warrior(&mut seed, &cfg),
            )
        })
        .collect();

    let mut g = c.benchmark_group("mars");
    let rounds = 20;
    for (name, a, b) in [
        ("dwarf_vs_imp", &dwarf, &imp),
        ("edge_vs_dwarf", &edge, &dwarf),
    ] {
        let mut m = Mars::new(&cfg, 2);
        m.play(&cfg, a, b, rounds, 1);
        g.throughput(Throughput::Elements(m.steps));
        g.bench_function(BenchmarkId::new(name, rounds), |bch| {
            let mut m = Mars::new(&cfg, 2);
            bch.iter(|| m.play(&cfg, a, b, rounds, 1))
        });
    }
    let mut m = Mars::new(&cfg, 2);
    for (a, b) in &randoms {
        m.play(&cfg, a, b, rounds, 1);
    }
    g.throughput(Throughput::Elements(m.steps));
    g.bench_function(BenchmarkId::new("random_pairs", randoms.len()), |bch| {
        let mut m = Mars::new(&cfg, 2);
        bch.iter(|| {
            for (a, b) in &randoms {
                m.play(&cfg, a, b, rounds, 1);
            }
        })
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
