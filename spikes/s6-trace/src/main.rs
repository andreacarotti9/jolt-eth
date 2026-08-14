//! S6 driver: prove one trace-length target per process.
//!
//! `max_trace_length` is a `#[jolt::provable]` attribute, i.e. a compile-time
//! literal in the generated `preprocess_shared_*`. To sweep it without a guest
//! rebuild per rung we rebuild `JoltSharedPreprocessing` by hand - the same three
//! calls the macro makes, with the bound as a runtime value.
//!
//! Usage: `s6-trace <log2_max_trace_length>`

use std::time::Instant;

use jolt_sdk::{
    host::JoltProgramSource, JoltSharedPreprocessing, MemoryConfig, MemoryLayout,
    ProgramPreprocessing,
};
use serde::Serialize;
use spike_util::Env;

/// Must match the `#[jolt::provable]` attributes on `spin`.
const STACK_SIZE: u64 = 1 << 20;
const HEAP_SIZE: u64 = 8 << 20;

#[derive(Serialize)]
struct Report {
    spike: &'static str,
    env: Env,
    prover: &'static str,
    log2_max_trace_length: u32,
    max_trace_length: u64,
    iters: u32,
    cycles_per_iter: f64,
    actual_trace_len: u64,
    fill_ratio: f64,
    preprocess_seconds: f64,
    prove_seconds: f64,
    verify_seconds: f64,
    verified: bool,
}

fn main() {
    let cap = spike_util::init();
    let log2t: u32 = std::env::args()
        .nth(1)
        .expect("usage: s6-trace <log2_max_trace_length>")
        .parse()
        .expect("log2_max_trace_length must be an integer");
    let max_trace_length = 1u64 << log2t;

    let target_dir = "/tmp/jolt-guest-targets";
    let mut program = guest::compile_spin(target_dir);

    // Two cheap points give slope (cycles per iteration) and intercept (boot and
    // teardown), so we can aim at the target instead of guessing.
    let (a, b) = (10_000u32, 20_000u32);
    let (ta, tb) = (
        guest::analyze_spin(a).trace_len() as f64,
        guest::analyze_spin(b).trace_len() as f64,
    );
    let cycles_per_iter = (tb - ta) / f64::from(b - a);
    let overhead = ta - cycles_per_iter * f64::from(a);
    // Aim at 90% of the bound: the prover rejects a trace longer than the bound,
    // and we want the rung to be about the bound, not about overshoot.
    let iters = (((max_trace_length as f64) * 0.9 - overhead) / cycles_per_iter).max(1.0) as u32;
    let _ = spike_util::drain(&cap);

    eprintln!(
        "log2T={log2t} cycles/iter={cycles_per_iter:.2} overhead={overhead:.0} iters={iters}"
    );

    let started = Instant::now();
    let (bytecode, memory_init, program_size, e_entry) = program.decode();
    let memory_config = MemoryConfig {
        max_input_size: 4096,
        max_output_size: 4096,
        max_untrusted_advice_size: 4096,
        max_trusted_advice_size: 4096,
        stack_size: STACK_SIZE,
        heap_size: HEAP_SIZE,
        program_size: Some(program_size),
    };
    let memory_layout = MemoryLayout::new(&memory_config);
    let program_data =
        ProgramPreprocessing::preprocess(bytecode, memory_init, e_entry).expect("preprocess");
    let shared = JoltSharedPreprocessing::new(program_data, memory_layout, max_trace_length as usize);

    let prover_pp = guest::preprocess_prover_spin(shared.clone());
    let verifier_setup = prover_pp.generators.to_verifier_setup();
    let verifier_pp = guest::preprocess_verifier_spin(shared, verifier_setup, None);
    let preprocess_seconds = started.elapsed().as_secs_f64();

    let prove = guest::build_prover_spin(program, prover_pp);
    let verify = guest::build_verifier_spin(verifier_pp);

    let started = Instant::now();
    let (output, proof, io_device) = prove(iters);
    let prove_seconds = started.elapsed().as_secs_f64();

    let started = Instant::now();
    let verified = verify(iters, output, io_device.panic, proof);
    let verify_seconds = started.elapsed().as_secs_f64();

    let actual_trace_len = spike_util::drain(&cap)
        .iter()
        .find(|m| m.label == "spin_loop")
        .map_or(0, |m| m.total)
        + overhead as u64;

    println!(
        "2^{log2t:<2} bound  iters {iters:>10}  preprocess {preprocess_seconds:>7.1} s  \
         prove {prove_seconds:>8.1} s  verify {verify_seconds:>5.2} s  ok={verified}"
    );

    spike_util::write_json(
        spike_util::result_path(format!("s6-trace-2p{log2t}.json")),
        &Report {
            spike: "S6 trace-length / RAM ceiling",
            env: Env::capture(),
            // jolt-sdk's `host` feature pulls jolt-prover-legacy; the refactored
            // jolt-prover is not wired into the provable macro at 915faf4.
            prover: "jolt-prover-legacy",
            log2_max_trace_length: log2t,
            max_trace_length,
            iters,
            cycles_per_iter,
            actual_trace_len,
            fill_ratio: actual_trace_len as f64 / max_trace_length as f64,
            preprocess_seconds,
            prove_seconds,
            verify_seconds,
            verified,
        },
    );
}
