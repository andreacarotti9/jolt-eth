//! Host driver for the Jolt stateless-validator guest.
//!
//! ```text
//! jolt-eth-host analyze <fixture.json> [--baseline] [--case SUBSTRING]
//! jolt-eth-host prove   <fixture.json> [--baseline] [--case SUBSTRING]
//! ```
//!
//! `--case` matters more than it looks. An EEST file holds many cases and the
//! first is often degenerate - `ecpairing/valid.json` opens with the *empty*
//! input, which does no pairing at all, and `p256verify/wycheproof_valid.json`
//! opens with an invalid public key. Benchmarking those and calling the result
//! "the bn254 workload" would be wrong, so the corpus names the case it means.
//!
//! The fixture is an EEST `blockchain_tests` JSON; we take the first block that
//! carries `statelessInputBytes` / `statelessOutputBytes`, which is the same
//! contract `zkevm-benchmark-workload` feeds to the other three backends.
//!
//! The guest `Program` is assembled here rather than through the macro's
//! `compile_validate_block`, because the A/B that matters - Jolt inlines against
//! the software crypto they replace - is a guest *feature*, and only this path
//! can add it per run.

use std::{path::Path, time::Instant};

use jolt_sdk::{
    host::{JoltProgramSource, Program},
    JoltSharedPreprocessing, MemoryConfig, MemoryLayout, ProgramPreprocessing,
};
use serde::Serialize;
use spike_util::{Env, Marker};

/// Must match the `#[jolt::provable]` attributes on `validate_block`.
const MEMORY: MemoryConfig = MemoryConfig {
    max_input_size: 4096,
    max_output_size: 4096,
    max_untrusted_advice_size: 16 << 20,
    max_trusted_advice_size: 4096,
    stack_size: 4 << 20,
    heap_size: 128 << 20,
    program_size: None,
};
/// Jolt sizes its generators from `max_trace_length`, and S6 measured the cost
/// of over-declaring it: peak RSS grows ~1.5x per doubling. So the prove path
/// traces first and rounds the real length up to the next power of two.
const MIN_TRACE_LENGTH: usize = 1 << 16;
const GUEST: &str = "stateless-validator-reth-jolt";
const FUNC: &str = "validate_block";

fn main() {
    let cap = spike_util::init();
    // Without this the linker drops the inline sequence builders and decoding an
    // accelerated guest ELF fails with `Expansion(UnsupportedInstruction)`.
    jolt_zkvm_accel::register_host_inlines();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let baseline = args.iter().any(|a| a == "--baseline");
    let case = args
        .iter()
        .position(|a| a == "--case")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let mut positional = args
        .iter()
        .enumerate()
        .filter(|(i, a)| {
            !a.starts_with("--") && !(*i > 0 && args[i - 1] == "--case")
        })
        .map(|(_, a)| a);
    let action = positional.next().unwrap_or_else(|| usage());
    let fixture_path = positional.next().unwrap_or_else(|| usage());

    let fixture = Fixture::load(fixture_path, case.as_deref());
    let config = if baseline { "baseline" } else { "accel" };
    println!(
        "fixture {} :: {} bytes witness, {} bytes expected output [{config}]",
        fixture.name,
        fixture.input.len(),
        fixture.expected_output.len()
    );

    let program = build_program(baseline);
    let witness = jolt_sdk::postcard::to_stdvec(&fixture.input.as_slice()).expect("encode witness");

    match action.as_str() {
        "analyze" => analyze(&cap, &fixture, config, program, &witness),
        "prove" => prove(&cap, &fixture, config, program, &witness),
        other => panic!("unknown action `{other}`"),
    }
}

fn usage() -> ! {
    eprintln!(
        "usage: jolt-eth-host <analyze|prove> <fixture.json> [--baseline] [--case SUBSTRING]"
    );
    std::process::exit(2)
}

/// Builds the guest ELF, with or without the Jolt accelerator layer.
fn build_program(baseline: bool) -> Program {
    let mut program = Program::new(GUEST);
    program.set_func(FUNC);
    program.set_memory_config(MEMORY);
    if !baseline {
        program.add_guest_feature("accel");
    }
    // Separate target dirs, or the two configurations would keep evicting each
    // other's ELF and every A/B would pay a full reth rebuild.
    let suffix = if baseline { "baseline" } else { "accel" };
    program.build(&format!("/tmp/jolt-guest-targets/{GUEST}-{suffix}"));
    program
}

#[derive(Serialize)]
struct Report {
    fixture: String,
    /// Corpus path, because the EEST test id does not name the workload.
    fixture_path: String,
    config: &'static str,
    witness_bytes: usize,
    trace_len: u64,
    regions: Vec<Marker>,
    output_matches: bool,
    trace_seconds: f64,
    prove_seconds: Option<f64>,
    verify_seconds: Option<f64>,
    verified: Option<bool>,
    env: Env,
}

fn analyze(
    cap: &spike_util::Capture,
    fixture: &Fixture,
    config: &'static str,
    program: Program,
    witness: &[u8],
) {
    let started = Instant::now();
    // `trace_analyze` consumes the program; nothing after it needs one.
    let summary = program.trace_analyze::<jolt_sdk::F>(&[], witness, &[]);
    let trace_seconds = started.elapsed().as_secs_f64();
    let trace_len = summary.trace_len() as u64;
    let regions = spike_util::drain(cap);

    let output = decode_output(&summary.io_device.outputs);
    let output_matches = output == fixture.expected_output;

    report_regions(&regions, trace_len);
    println!("trace {trace_len} cycles in {trace_seconds:.1} s, output matches fixture: {output_matches}");

    spike_util::write_json(
        spike_util::result_path(format!("guest-{}-{config}-analyze.json", fixture.slug)),
        &Report {
            fixture: fixture.name.clone(),
            fixture_path: fixture.path.clone(),
            config,
            witness_bytes: fixture.input.len(),
            trace_len,
            regions,
            output_matches,
            trace_seconds,
            prove_seconds: None,
            verify_seconds: None,
            verified: None,
            env: Env::capture(),
        },
    );
}

fn prove(
    cap: &spike_util::Capture,
    fixture: &Fixture,
    config: &'static str,
    mut program: Program,
    witness: &[u8],
) {
    let trace_len = program.clone().trace_analyze::<jolt_sdk::F>(&[], witness, &[]).trace_len();
    let max_trace_length = trace_len.next_power_of_two().max(MIN_TRACE_LENGTH);
    let _ = spike_util::drain(cap);
    println!("traced {trace_len} cycles, proving with bound {max_trace_length}");

    let started = Instant::now();
    let (bytecode, memory_init, program_size, e_entry) = program.decode();
    let mut memory_config = MEMORY;
    memory_config.program_size = Some(program_size);
    let layout = MemoryLayout::new(&memory_config);
    let program_data =
        ProgramPreprocessing::preprocess(bytecode, memory_init, e_entry).expect("preprocess");
    let shared = JoltSharedPreprocessing::new(program_data, layout, max_trace_length);

    let prover_pp = guest::preprocess_prover_validate_block(shared.clone());
    let verifier_setup = prover_pp.generators.to_verifier_setup();
    let verifier_pp = guest::preprocess_verifier_validate_block(shared, verifier_setup, None);
    println!("preprocess {:.1} s", started.elapsed().as_secs_f64());

    let prove_fn = guest::build_prover_validate_block(program, prover_pp);
    let verify_fn = guest::build_verifier_validate_block(verifier_pp);

    let started = Instant::now();
    let (output, proof, io_device) =
        prove_fn(jolt_sdk::UntrustedAdvice::new(fixture.input.as_slice()));
    let prove_seconds = started.elapsed().as_secs_f64();

    let started = Instant::now();
    let verified = verify_fn(output.clone(), io_device.panic, proof);
    let verify_seconds = started.elapsed().as_secs_f64();

    let regions = spike_util::drain(cap);
    let output_matches = output == fixture.expected_output;
    report_regions(&regions, 0);
    println!(
        "prove {prove_seconds:.1} s, verify {verify_seconds:.2} s, verified {verified}, \
         output matches {output_matches}"
    );

    spike_util::write_json(
        spike_util::result_path(format!("guest-{}-{config}-prove.json", fixture.slug)),
        &Report {
            fixture: fixture.name.clone(),
            fixture_path: fixture.path.clone(),
            config,
            witness_bytes: fixture.input.len(),
            trace_len: trace_len as u64,
            regions,
            output_matches,
            trace_seconds: 0.0,
            prove_seconds: Some(prove_seconds),
            verify_seconds: Some(verify_seconds),
            verified: Some(verified),
            env: Env::capture(),
        },
    );
}

/// The guest writes its `Vec<u8>` return value into the output region as
/// postcard; the tracer hands that region back verbatim, trailing zeroes and all.
fn decode_output(raw: &[u8]) -> Vec<u8> {
    jolt_sdk::postcard::take_from_bytes::<Vec<u8>>(raw)
        .map(|(value, _)| value)
        .unwrap_or_default()
}

fn report_regions(regions: &[Marker], trace_len: u64) {
    for m in regions {
        let share = if trace_len > 0 {
            format!("{:>5.1}%", 100.0 * m.total as f64 / trace_len as f64)
        } else {
            "    -".into()
        };
        println!(
            "  {:<28} {:>12} cycles ({:>10} real + {:>10} virtual) {share}",
            m.label, m.total, m.real, m.virt
        );
    }
}

/// One EEST `blockchain_tests` block, reduced to the two byte strings the
/// stateless guest contract is defined over.
struct Fixture {
    name: String,
    path: String,
    slug: String,
    input: Vec<u8>,
    expected_output: Vec<u8>,
}

impl Fixture {
    /// Picks the first block carrying canonical stateless bytes, restricted to
    /// cases whose name contains `case` when one is given.
    fn load(path: &str, case: Option<&str>) -> Self {
        let text = std::fs::read_to_string(path).expect("read fixture");
        let root: serde_json::Value = serde_json::from_str(&text).expect("parse fixture json");
        let cases = root.as_object().expect("fixture must be a JSON object");

        for (name, entry) in cases {
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
                // The case goes into the slug so two cases from one file cannot
                // overwrite each other's results.
                let tag = case.map(|c| format!("-{c}")).unwrap_or_default();
                return Self {
                    name: format!("{name}#{index}"),
                    path: path.to_string(),
                    slug: sanitize(&format!("{stem}{tag}-{index}")),
                    input: unhex(input),
                    expected_output: unhex(output),
                };
            }
        }
        panic!(
            "no block in {path}{} carries statelessInputBytes/statelessOutputBytes",
            case.map(|c| format!(" matching case `{c}`")).unwrap_or_default()
        );
    }
}

fn unhex(s: &str) -> Vec<u8> {
    hex::decode(s.strip_prefix("0x").unwrap_or(s)).expect("fixture bytes must be hex")
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}
