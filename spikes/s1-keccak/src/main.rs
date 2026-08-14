//! S1 driver: trace the keccak guest at a ladder of input sizes and report
//! cycles/byte for the inline against the software implementation.
//!
//! Tracing only - no proving. S1 answers "is the inline reachable and how much
//! does it save", which is a trace-length question; S6 owns proving cost.

use serde::Serialize;
use spike_util::{Env, Marker};

const SIZES: &[u32] = &[1024, 8192, 65_536, 262_144, 1_048_576];

#[derive(Serialize)]
struct Row {
    size_bytes: u32,
    trace_len: u64,
    inline: Marker,
    soft: Marker,
    inline_cycles_per_byte: f64,
    soft_cycles_per_byte: f64,
    speedup: f64,
}

#[derive(Serialize)]
struct Report {
    spike: &'static str,
    env: Env,
    rows: Vec<Row>,
}

fn main() {
    let cap = spike_util::init();
    let mut rows = Vec::new();

    for &size in SIZES {
        let summary = guest::analyze_keccak_bench(size);
        let trace_len = summary.trace_len() as u64;

        let markers = spike_util::drain(&cap);
        let pick = |label: &str| -> Marker {
            markers
                .iter()
                .find(|m| m.label == label)
                .unwrap_or_else(|| panic!("no cycle marker {label} at size {size}"))
                .clone()
        };
        let (inline, soft) = (pick("keccak_inline"), pick("keccak_soft"));

        let per_byte = |m: &Marker| m.total as f64 / size as f64;
        rows.push(Row {
            size_bytes: size,
            trace_len,
            inline_cycles_per_byte: per_byte(&inline),
            soft_cycles_per_byte: per_byte(&soft),
            speedup: soft.total as f64 / inline.total as f64,
            inline,
            soft,
        });

        let r = rows.last().unwrap();
        println!(
            "{:>9} B  inline {:>10} cyc ({:>6.2}/B)  soft {:>10} cyc ({:>6.2}/B)  {:>5.1}x  trace {}",
            r.size_bytes,
            r.inline.total,
            r.inline_cycles_per_byte,
            r.soft.total,
            r.soft_cycles_per_byte,
            r.speedup,
            r.trace_len,
        );
    }

    spike_util::write_json(
        spike_util::result_path("s1-keccak.json"),
        &Report {
            spike: "S1 keccak inline reachability",
            env: Env::capture(),
            rows,
        },
    );
}
