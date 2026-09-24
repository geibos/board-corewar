//! Generates the fast engine's dispatch (`Run::step` in src/fast.rs): a
//! match on the cell's opcode-and-modifier byte with an arm per pair, each
//! passing that pair to `Run::exec` as a constant.
//!
//! Tried and measured (a round robin of 8 warriors, 250 rounds): arms per
//! opcode and both addressing modes (1216) were 7% faster on an M-series
//! Mac but 5% slower on Zen 3, and took 7–23 minutes to compile; arms per
//! opcode and A-mode (152) gave 2% on Zen 3. Per opcode and modifier: 12%
//! faster on Zen 3, 5% slower on the Mac.
use std::fmt::Write;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let mut out = String::from("match ir.opm {\n");
    for op in 0..19u8 {
        for m in 0..8u8 {
            let opm = op << 3 | m;
            writeln!(out, "    {opm} => self.exec::<W>(cs, pc, ir, {opm}),").unwrap();
        }
    }
    out.push_str("    _ => unsafe { std::hint::unreachable_unchecked() },\n}\n");
    let path = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("dispatch.rs");
    std::fs::write(path, out).unwrap();
}
