//! S5: the SP1 column, produced through Ere's own SP1 backend.
//!
//! ```text
//! sp1-baseline <execute|prove> <fixture.json> [--case SUBSTRING]
//! ```
//!
//! This is the neutrality argument in code. The guest is eth-act's
//! `stateless-validator-reth` - the same crate `guests/stateless-validator-jolt`
//! wraps - the fixture is the same EEST block, and both provers are driven
//! through the same `zkVMProver` trait. What differs is the zkVM, which is the
//! only thing that should.

use std::{path::Path, time::Instant};

use ere_compiler_sp1::{Compiler, SP1RustRv64imaCustomized};
use ere_prover_core::{Input, ProverResource, zkVMProver};
use ere_prover_sp1::SP1Prover;
use serde::Serialize;
use sp1_sdk::blocking::{Elf, Prover as _, ProverClient, SP1Stdin};

/// eth-act's SP1 guest binary, unmodified.
const GUEST_DIR: &str = "../upstream/ere-guests/bin/stateless-validator-reth/sp1";

#[derive(Serialize)]
struct Report {
    zkvm: &'static str,
    sdk_version: String,
    fixture: String,
    /// Corpus path, because the EEST test id does not name the workload. The
    /// Jolt driver records the same field so the two columns join on it.
    fixture_path: String,
    witness_bytes: usize,
    total_num_cycles: u64,
    execution_seconds: f64,
    prove_seconds: Option<f64>,
    verified: Option<bool>,
    output_matches: bool,
    host: String,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let case = args
        .iter()
        .position(|a| a == "--case")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let mut positional = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && !(*i > 0 && args[i - 1] == "--case"))
        .map(|(_, a)| a.clone());
    let action = positional.next().unwrap_or_else(|| usage());
    let fixture_path = positional.next().unwrap_or_else(|| usage());
    let (name, slug, witness, expected_output) = load_fixture(&fixture_path, case.as_deref());

    let guest_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join(GUEST_DIR);
    eprintln!("compiling {} ...", guest_directory.display());
    // Succinct's own toolchain via `cargo prove build`, rather than Ere's
    // stock-nightly path: the latter needs `-Zjson-target-spec`, which only very
    // recent nightlies have, and those same nightlies fail to `-Zbuild-std` this
    // guest's dependency tree. `cargo prove build` is what SP1 users actually
    // run, so it is also the fairer thing to measure.
    // `--ignore-rust-version`: the installed Succinct toolchain reports 1.93,
    // and ere-guests' reth pin declares 1.95. The guest builds and runs; the
    // declaration is the only thing in the way.
    let elf = SP1RustRv64imaCustomized
        .compile(&guest_directory, &["--ignore-rust-version".to_string()])
        .expect("compile SP1 guest");
    eprintln!("elf {} bytes", elf.len());

    let prover = SP1Prover::new(elf.clone(), ProverResource::Cpu).expect("SP1 prover");
    let input = Input::new().with_stdin(witness.clone());

    // Cycles come from the SDK executor rather than `ere-prover-sp1::execute`.
    // Ere pools `MinimalExecutorEnum`s and calls `reset()` between runs, and SP1
    // v6.3.1's portable minimal executor - the one every non-x86_64 host gets -
    // leaves `reset()` as `todo!()`. Same SP1 version, same ELF, same input; only
    // the pooling wrapper is bypassed, and that is a finding in its own right.
    let started = Instant::now();
    let client = ProverClient::builder().cpu().build();
    let mut stdin = SP1Stdin::new();
    stdin.write_slice(&witness);
    let (public_values, execution) = client
        .execute(Elf::Dynamic(std::sync::Arc::from(elf.0.clone())), stdin)
        .run()
        .expect("execute");
    let execution_seconds = started.elapsed().as_secs_f64();
    let total_num_cycles = execution.total_instruction_count();
    let output_matches = public_values.as_slice() == expected_output.as_slice();

    println!(
        "{name}\n  SP1 execute: {total_num_cycles} cycles in {execution_seconds:.1} s, output matches {output_matches}"
    );

    let (prove_seconds, verified) = if action == "prove" {
        let started = Instant::now();
        let (_public_values, proof, _report) = prover.prove(&input).expect("prove");
        let prove_seconds = started.elapsed().as_secs_f64();
        let verified = prover.verify(&proof).is_ok();
        println!("  SP1 prove: {prove_seconds:.1} s, verified {verified}");
        (Some(prove_seconds), Some(verified))
    } else {
        (None, None)
    };

    let report = Report {
        zkvm: "sp1",
        sdk_version: prover.sdk_version().to_string(),
        fixture: name,
        fixture_path: fixture_path.clone(),
        witness_bytes: witness.len(),
        total_num_cycles,
        execution_seconds,
        prove_seconds,
        verified,
        output_matches,
        host: std::env::var("SPIKE_HOST").unwrap_or_else(|_| "unknown".into()),
    };
    let path = format!("../bench/results/sp1-{slug}-{action}.json");
    std::fs::create_dir_all("../bench/results").expect("create result dir");
    std::fs::write(&path, serde_json::to_string_pretty(&report).unwrap()).expect("write result");
    eprintln!("wrote {path}");
}

fn usage() -> ! {
    eprintln!("usage: sp1-baseline <execute|prove> <fixture.json> [--case SUBSTRING]");
    std::process::exit(2)
}

/// Same selection rule as `jolt-eth-host`: the first block carrying canonical
/// stateless bytes, restricted to cases matching `case`. Both drivers must pick
/// the same block or the columns are not comparable.
fn load_fixture(path: &str, case: Option<&str>) -> (String, String, Vec<u8>, Vec<u8>) {
    let text = std::fs::read_to_string(path).expect("read fixture");
    let root: serde_json::Value = serde_json::from_str(&text).expect("parse fixture json");
    for (name, entry) in root.as_object().expect("fixture object") {
        if case.is_some_and(|wanted| !name.contains(wanted)) {
            continue;
        }
        let Some(blocks) = entry.get("blocks").and_then(|b| b.as_array()) else {
            continue;
        };
        for (index, block) in blocks.iter().enumerate() {
            let (Some(input), Some(output)) = (
                block.get("statelessInputBytes").and_then(|v| v.as_str()),
                block.get("statelessOutputBytes").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            let stem = Path::new(path)
                .file_stem()
                .map_or("fixture".into(), |s| s.to_string_lossy().into_owned());
            let tag = case.map(|c| format!("-{c}")).unwrap_or_default();
            let slug: String = format!("{stem}{tag}-{index}")
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            return (format!("{name}#{index}"), slug, unhex(input), unhex(output));
        }
    }
    panic!(
        "no block in {path}{} carries statelessInputBytes/statelessOutputBytes",
        case.map(|c| format!(" matching case `{c}`")).unwrap_or_default()
    );
}

fn unhex(s: &str) -> Vec<u8> {
    hex::decode(s.strip_prefix("0x").unwrap_or(s)).expect("fixture bytes must be hex")
}
