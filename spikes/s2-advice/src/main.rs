//! S2 driver: one payload size per process, so a failure at the top of the
//! ladder does not take the rest of the ladder with it and `/usr/bin/time -l`
//! can attribute peak RSS to a single size.
//!
//! Usage: `s2-advice <payload_bytes>` or `s2-advice --probe-overflow`

use std::time::Instant;

use serde::Serialize;
use spike_util::{Env, Marker};

/// Declared in the guest's `#[jolt::provable]` attributes.
const DECLARED_MAX_ADVICE: u64 = 67_108_864;

#[derive(Serialize)]
struct OverflowProbe {
    spike: &'static str,
    env: Env,
    declared_max_advice_bytes: u64,
    payload_bytes: usize,
    rejected: bool,
    message: String,
}

#[derive(Serialize)]
struct Report {
    spike: &'static str,
    env: Env,
    payload_bytes: usize,
    declared_max_advice_bytes: u64,
    trace_len: u64,
    trace_seconds: f64,
    markers: Vec<Marker>,
    dense_cycles_per_byte: f64,
}

fn main() {
    let cap = spike_util::init();
    let arg = std::env::args()
        .nth(1)
        .expect("usage: s2-advice <payload_bytes|--probe-overflow>");
    if arg == "--probe-overflow" {
        probe_overflow();
        return;
    }
    let payload_bytes: usize = arg.parse().expect("payload_bytes must be an integer");

    // Deterministic, and cheap to build on the host: the point of the spike is
    // the transport, not the content.
    let payload: Vec<u8> = (0..payload_bytes).map(|i| (i * 37 + 11) as u8).collect();

    let started = Instant::now();
    let summary = guest::analyze_advice_sink(
        payload_bytes as u32,
        jolt_sdk::UntrustedAdvice::new(payload.as_slice()),
    );
    let trace_seconds = started.elapsed().as_secs_f64();
    let trace_len = summary.trace_len() as u64;
    let markers = spike_util::drain(&cap);

    let dense = markers
        .iter()
        .find(|m| m.label == "advice_sum_dense")
        .expect("no advice_sum_dense marker");
    let window = payload_bytes.min(1 << 20).max(1);
    let dense_cycles_per_byte = dense.total as f64 / window as f64;

    println!(
        "{:>10} B payload  trace {:>12} cycles  {:>7.1} s  dense {:.2} cyc/B",
        payload_bytes, trace_len, trace_seconds, dense_cycles_per_byte
    );

    spike_util::write_json(
        spike_util::result_path(format!("s2-advice-{payload_bytes}.json")),
        &Report {
            spike: "S2 advice tape + heap ceiling",
            env: Env::capture(),
            payload_bytes,
            declared_max_advice_bytes: DECLARED_MAX_ADVICE,
            trace_len,
            trace_seconds,
            markers,
            dense_cycles_per_byte,
        },
    );
}

/// Records what happens when the payload exceeds the declared advice region.
///
/// The gate memo quotes this failure; quoting is not evidence, so the probe runs
/// it and writes the outcome down like every other measurement.
fn probe_overflow() {
    let payload_bytes = (DECLARED_MAX_ADVICE as usize) + (32 << 20);
    let payload: Vec<u8> = vec![0xAB; payload_bytes];

    // The rejection is a host-side panic inside the tracer, which is the whole
    // point: it happens before anything is proven.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(|| {
        guest::analyze_advice_sink(
            payload_bytes as u32,
            jolt_sdk::UntrustedAdvice::new(payload.as_slice()),
        );
    });
    std::panic::set_hook(previous);

    let message = match &outcome {
        Ok(_) => "accepted (no rejection)".to_string(),
        Err(payload) => payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
            .unwrap_or_else(|| "panicked without a message".to_string()),
    };
    println!("overflow probe: rejected={} :: {message}", outcome.is_err());

    spike_util::write_json(
        spike_util::result_path("s2-advice-overflow.json"),
        &OverflowProbe {
            spike: "S2 advice overflow probe",
            env: Env::capture(),
            declared_max_advice_bytes: DECLARED_MAX_ADVICE,
            payload_bytes,
            rejected: outcome.is_err(),
            message,
        },
    );
}
