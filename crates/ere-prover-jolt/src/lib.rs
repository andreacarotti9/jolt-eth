//! Ere [`zkVMProver`] implementation for the Jolt zkVM.
//!
//! # Guest contract
//!
//! Jolt has no ambient guest IO: `#[jolt::provable]` computes the memory layout
//! and bakes the input and output addresses into the entrypoint it generates. So
//! an Ere Jolt guest is a program whose provable function has this shape:
//!
//! ```ignore
//! #[jolt::provable(/* sizes */)]
//! fn main(input: jolt::UntrustedAdvice<&[u8]>) -> Vec<u8> { .. }
//! ```
//!
//! [`Input::stdin`] is postcard-encoded into the untrusted advice region, and the
//! guest's returned `Vec<u8>` is read back out of the output region as
//! [`PublicValues`]. Untrusted advice rather than committed input is deliberate:
//! it is not part of what the verifier trusts, which is the right position for a
//! stateless-validation witness the guest authenticates for itself.
//!
//! # Trace-length bound
//!
//! Jolt's preprocessing - and therefore the verifying key - is parameterised by
//! `max_trace_length`, so it must be fixed when the prover is constructed rather
//! than derived per input, or the key would depend on the witness. Set it with
//! `ERE_JOLT_MAX_TRACE_LENGTH`. It is not free: measured on an M3 Max, peak RSS
//! runs about 1.5x per doubling of the bound (2^24 -> 14 GB, 2^25 -> 22 GB), so
//! declaring far above the real trace wastes memory rather than time.

mod error;

use std::time::Instant;

use ere_compiler_core::Elf;
use jolt_sdk::{
    JoltProverPreprocessing, JoltSharedPreprocessing, MemoryConfig, MemoryLayout,
    ProgramPreprocessing, RV64IMACProver,
};
use tracing::info;

pub use ere_prover_core::{
    CommonError, Input, ProgramExecutionReport, ProgramProvingReport, ProverResource,
    ProverResourceKind, PublicValues, zkVMProver, zkVMVerifier,
};
pub use ere_verifier_jolt::{JoltProgramVk, JoltProof, JoltVerifier};

pub use crate::error::Error;

/// Forces the linker to keep every inline crate's `inventory` registration.
///
/// The tracer expands a guest's inline instructions by looking up
/// `(opcode, funct3, funct7)` in an `inventory` registry each `jolt-inlines-*`
/// crate populates from its `host` module. Those registrations live in rlib
/// object files the linker drops unless something references them, and a backend
/// that merely depends on the crates references nothing - so decoding a guest
/// that uses, say, the secp256k1 inline fails with `UnsupportedInstruction`.
/// A backend cannot know which inlines a guest picked, so it keeps all of them.
fn register_inlines() {
    type StoreFn = fn() -> Result<(), String>;
    let _: [StoreFn; 8] = [
        jolt_inlines_sha2::store_inlines,
        jolt_inlines_keccak256::store_inlines,
        jolt_inlines_blake2::store_inlines,
        jolt_inlines_blake3::store_inlines,
        jolt_inlines_bigint::store_inlines,
        jolt_inlines_secp256k1::store_inlines,
        jolt_inlines_p256::store_inlines,
        jolt_inlines_grumpkin::store_inlines,
    ];
}

/// Overrides the trace-length bound the prover is built for.
pub const MAX_TRACE_LENGTH_ENV: &str = "ERE_JOLT_MAX_TRACE_LENGTH";

/// Jolt's own default (`common::DEFAULT_MAX_TRACE_LENGTH`).
const DEFAULT_MAX_TRACE_LENGTH: usize = 1 << 24;

/// Prover bound to a specific compiled guest program.
pub struct JoltProver {
    elf: Vec<u8>,
    memory_config: MemoryConfig,
    max_trace_length: usize,
    preprocessing: JoltProverPreprocessing<jolt_sdk::F, jolt_sdk::Curve, jolt_sdk::PCS>,
    verifier: JoltVerifier,
}

impl core::fmt::Debug for JoltProver {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("JoltProver")
            .field("elf_len", &self.elf.len())
            .field("max_trace_length", &self.max_trace_length)
            .finish()
    }
}

impl JoltProver {
    /// Builds a prover for `elf`.
    ///
    /// Jolt has no GPU path at this pin, so anything other than
    /// [`ProverResource::Cpu`] is rejected rather than silently downgraded - a
    /// benchmark that quietly ran on CPU while labelled GPU would be worse than
    /// an error.
    pub fn new(elf: Elf, resource: ProverResource) -> Result<Self, Error> {
        register_inlines();
        if !matches!(resource, ProverResource::Cpu) {
            return Err(Error::Preprocessing(format!(
                "unsupported prover resource {:?}, Jolt supports {:?} only",
                resource.kind(),
                ProverResourceKind::Cpu
            )));
        }

        let max_trace_length = std::env::var(MAX_TRACE_LENGTH_ENV)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_MAX_TRACE_LENGTH);

        let (bytecode, memory_init, program_size, e_entry) =
            jolt_sdk::guest::program::decode(&elf.0);

        // The guest's `#[jolt::provable]` attributes are the source of truth for
        // the layout, but Jolt exposes no way to read them back out of the ELF at
        // this pin, so they have to be restated here. See `guest_memory_config`.
        let memory_config = guest_memory_config(program_size);
        let memory_layout = MemoryLayout::new(&memory_config);

        let program_data = ProgramPreprocessing::preprocess(bytecode, memory_init, e_entry)
            .map_err(|err| Error::Preprocessing(format!("{err:?}")))?;
        let shared = JoltSharedPreprocessing::new(program_data, memory_layout, max_trace_length);

        let preprocessing = JoltProverPreprocessing::new(shared.clone());
        // Through the trait rather than the inherent method: the inherent one is
        // generic in a way a return-type annotation alone cannot pin down.
        let verifier_setup = <jolt_sdk::PCS as jolt_sdk::CommitmentScheme>::setup_verifier(
            &preprocessing.generators,
        );
        let verifier_preprocessing =
            jolt_sdk::jolt_prover_legacy::zkvm::proof::verifier_preprocessing_from_shared::<
                jolt_sdk::F,
                jolt_sdk::Curve,
                jolt_sdk::PCS,
            >(shared, verifier_setup, None);

        Ok(Self {
            elf: elf.0,
            memory_config,
            max_trace_length,
            preprocessing,
            verifier: JoltVerifier::new(verifier_preprocessing)?,
        })
    }

    /// The trace-length bound this prover was built for.
    pub fn max_trace_length(&self) -> usize {
        self.max_trace_length
    }

    /// The guest's output region, padded back out to `max_output_size`.
    ///
    /// The tracer hands back only the bytes the guest actually touched, but the
    /// guest serialized its return value into the full region - so a value whose
    /// encoding ends in zeroes comes back truncated and fails to decode. Jolt's
    /// own generated prover pads for the same reason.
    fn public_values(&self, io_device: &jolt_sdk::JoltDevice) -> PublicValues {
        let mut outputs = io_device.outputs.clone();
        outputs.resize(self.memory_config.max_output_size as usize, 0);
        outputs.into()
    }
}

impl zkVMProver for JoltProver {
    type Verifier = JoltVerifier;
    type Error = Error;

    fn verifier(&self) -> &JoltVerifier {
        &self.verifier
    }

    fn execute(&self, input: &Input) -> Result<(PublicValues, ProgramExecutionReport), Error> {
        reject_proof_composition(input)?;
        let advice = encode_stdin(input);

        let start = Instant::now();
        let (trace, _final_cycles, _memory, io_device, _advice_tape) =
            jolt_sdk::guest::program::trace(
                &self.elf,
                None,
                &[],
                &advice,
                &[],
                &self.memory_config,
                None::<jolt_sdk::AdviceTape>,
            );
        // The iterator is lazy; counting it walks the trace without holding it.
        let total_num_cycles = trace.count() as u64;
        let execution_duration = start.elapsed();

        let mut report = ProgramExecutionReport::new(total_num_cycles);
        report.execution_duration = execution_duration;
        Ok((self.public_values(&io_device), report))
    }

    fn prove(
        &self,
        input: &Input,
    ) -> Result<(PublicValues, JoltProof, ProgramProvingReport), Error> {
        reject_proof_composition(input)?;
        let advice = encode_stdin(input);

        info!("Generating Jolt proof...");
        let start = Instant::now();
        let prover = RV64IMACProver::gen_from_elf(
            &self.preprocessing,
            &self.elf,
            &[],
            &advice,
            &[],
            None,
            None,
            None,
        );
        let io_device = prover.program_io.clone();
        let (proof, _) = prover
            .prove()
            .map_err(|err| Error::Proving(format!("{err:?}")))?;
        let proving_time = start.elapsed();

        let public_values = self.public_values(&io_device);
        Ok((
            public_values,
            JoltProof { proof, io_device },
            ProgramProvingReport::new(proving_time),
        ))
    }
}

/// The guest reads its witness with `postcard::take_from_bytes::<&[u8]>`, so the
/// raw stdin has to arrive length-prefixed.
fn encode_stdin(input: &Input) -> Vec<u8> {
    jolt_sdk::postcard::to_stdvec(&input.stdin()).expect("postcard encoding of a byte slice")
}

fn reject_proof_composition(input: &Input) -> Result<(), Error> {
    if input.proofs.is_some() {
        return Err(Error::ProofCompositionUnsupported);
    }
    Ok(())
}

/// The guest's memory layout, from `ERE_JOLT_*` over Jolt's own defaults.
///
/// This has to be restated rather than read back: `#[jolt::provable]` bakes the
/// sizes into the guest's entrypoint, and Jolt exposes no accessor for them at
/// this pin. `ere-compiler-jolt` reads the same variables, so the linker script
/// and the prover's layout stay in agreement.
///
/// Getting them wrong is loud, not silent - the guest traps on its first input
/// read - which is the only reason restating them is tolerable.
fn guest_memory_config(program_size: u64) -> MemoryConfig {
    let size = |key: &str, default: u64| {
        std::env::var(key)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    MemoryConfig {
        max_input_size: size("ERE_JOLT_MAX_INPUT_SIZE", 4096),
        max_output_size: size("ERE_JOLT_MAX_OUTPUT_SIZE", 4096),
        max_untrusted_advice_size: size("ERE_JOLT_MAX_UNTRUSTED_ADVICE_SIZE", 4096),
        max_trusted_advice_size: size("ERE_JOLT_MAX_TRUSTED_ADVICE_SIZE", 4096),
        // Jolt's own defaults, and the same values `ere-compiler-jolt` passes to
        // `jolt build`. Both crates read the same variables on purpose: the
        // linker script and the prover's memory layout have to agree.
        stack_size: size("ERE_JOLT_STACK_SIZE", 4096),
        heap_size: size("ERE_JOLT_HEAP_SIZE", 32 << 20),
        program_size: Some(program_size),
    }
}

#[cfg(test)]
mod tests {
    use ere_compiler_core::Compiler;
    use ere_compiler_jolt::JoltRustRv64imac;

    use super::*;

    /// Ere's backend contract, end to end, on the repo's stateless-validator
    /// guest: compile, execute, prove, verify - all through the unified API.
    ///
    /// The guest declares its own memory sizes with `#[jolt::provable]`, so the
    /// same values have to reach `jolt build` and the prover; that is what the
    /// `ERE_JOLT_*` variables are for.
    #[test]
    fn compile_execute_prove_verify() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("repo root");
        let guest_directory = repo_root.join("guests/stateless-validator-jolt");
        let fixture = repo_root.join(
            "bench/fixtures/eest/fixtures/blockchain_tests/for_amsterdam/amsterdam/\
             eip2780_reduce_intrinsic_tx_gas/value_moving_transactions/value_moving_transactions.json",
        );
        if !fixture.exists() {
            eprintln!("skipping: run scripts/fetch-fixtures.sh first");
            return;
        }

        // SAFETY: single-threaded test; nothing else reads these concurrently.
        unsafe {
            std::env::set_var("ERE_JOLT_STACK_SIZE", "4194304");
            std::env::set_var("ERE_JOLT_HEAP_SIZE", "134217728");
            std::env::set_var("ERE_JOLT_MAX_UNTRUSTED_ADVICE_SIZE", "16777216");
            std::env::set_var("ERE_JOLT_MAX_TRACE_LENGTH", "4194304");
        }

        let elf = JoltRustRv64imac
            .compile(&guest_directory, &["accel".to_string()])
            .expect("compile");
        assert!(!elf.is_empty(), "ELF bytes should not be empty");

        let (witness, expected_output) = read_fixture(&fixture);
        let input = Input::new().with_stdin(witness);

        let prover = JoltProver::new(elf, ProverResource::Cpu).expect("prover");
        assert_eq!(prover.name(), "jolt");

        let (public_values, report) = prover.execute(&input).expect("execute");
        assert!(report.total_num_cycles > 0, "execution reported no cycles");
        assert_eq!(
            decode_output(&public_values),
            expected_output,
            "executed output does not match the fixture"
        );

        let (public_values, proof, report) = prover.prove(&input).expect("prove");
        assert!(report.proving_time.as_millis() > 0);
        assert_eq!(decode_output(&public_values), expected_output);

        let verified = prover.verify(&proof).expect("verify");
        assert_eq!(
            decode_output(&verified),
            expected_output,
            "verified public values do not match the fixture"
        );
    }

    /// The guest returns `Vec<u8>`; the output region carries it postcard-encoded
    /// and zero-padded to `max_output_size`.
    fn decode_output(public_values: &PublicValues) -> Vec<u8> {
        match jolt_sdk::postcard::take_from_bytes::<Vec<u8>>(public_values) {
            Ok((value, _)) => value,
            Err(err) => panic!("failed to decode guest output: {err}"),
        }
    }

    fn read_fixture(path: &std::path::Path) -> (Vec<u8>, Vec<u8>) {
        let text = std::fs::read_to_string(path).expect("read fixture");
        let unhex = |s: &str| -> Vec<u8> {
            let s = s.strip_prefix("0x").unwrap_or(s);
            (0..s.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
                .collect()
        };
        // Deliberately hand-rolled: pulling serde_json in just to read one pair
        // of hex strings out of a fixture would be the tail wagging the dog.
        let find = |key: &str| -> String {
            let needle = format!("\"{key}\": \"");
            let start = text.find(&needle).expect("fixture key") + needle.len();
            let end = start + text[start..].find('"').expect("closing quote");
            text[start..end].to_string()
        };
        (unhex(&find("statelessInputBytes")), unhex(&find("statelessOutputBytes")))
    }
}

#[cfg(test)]
mod program_vk {
    use ere_prover_core::codec::Encode;

    use super::*;

    /// Two guests of very different sizes, both built at the same trace-length
    /// bound. Before the key became a digest these encoded to 98 MB and 244 KB;
    /// now both are 32 bytes, and they differ, which is the property the other
    /// Ere backends have.
    #[test]
    fn program_vk_is_a_32_byte_digest() {
        const GUESTS: [&str; 2] = [
            "/tmp/jolt-guest-targets/stateless-validator-reth-jolt-accel/\
             stateless-validator-reth-jolt-validate_block/riscv64imac-unknown-none-elf/\
             release/stateless-validator-reth-jolt",
            "/tmp/jolt-guest-targets/s6-trace-guest-spin/riscv64imac-unknown-none-elf/\
             release/s6-trace-guest",
        ];

        // SAFETY: single-threaded test.
        unsafe {
            std::env::set_var("ERE_JOLT_MAX_TRACE_LENGTH", "4194304");
        }

        let mut keys = Vec::new();
        for path in GUESTS {
            let path = path.replace(char::is_whitespace, "");
            let Ok(elf) = std::fs::read(&path) else {
                eprintln!("missing {path}; build the guests first");
                continue;
            };
            let prover = JoltProver::new(Elf(elf), ProverResource::Cpu).expect("prover");
            let encoded = prover.program_vk().encode_to_vec().expect("encode");
            assert_eq!(encoded.len(), 32, "verifying key must be a 32-byte digest");
            keys.push(encoded);
        }

        if keys.len() == 2 {
            assert_ne!(keys[0], keys[1], "different programs must have different keys");
        }
    }
}
