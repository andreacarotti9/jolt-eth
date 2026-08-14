//! Ere [`zkVMVerifier`] implementation for the Jolt zkVM.
//!
//! Two Jolt facts shape this crate.
//!
//! **The verifying key is a digest, and the preprocessing lives in the verifier.**
//! Verifying a Jolt proof needs the guest's full `JoltVerifierPreprocessing`,
//! which is program-specific and large - measured at 98 MB for the stateless
//! validator against 244 KB for a toy guest at the same trace bound, so it scales
//! with the program, not with a universal parameter that could be factored out.
//!
//! Publishing that as [`JoltProgramVk`] would be absurd next to SP1's and ZisK's
//! 32 bytes, so [`JoltProgramVk`] is a 32-byte digest *over* the preprocessing
//! and [`JoltVerifier`] holds the preprocessing itself. Constructing a verifier
//! re-derives the preprocessing from the guest ELF and checks it against the
//! key, which is what makes the key binding: a preprocessing that hashes to the
//! key is the one that program produces.
//!
//! The honest cost, and the difference from SP1 worth stating plainly: an SP1
//! verifier needs only 32 bytes, because SP1 verifies a *compressed, recursive*
//! proof whose recursion circuit checks the real verifying key internally. Jolt
//! at this pin verifies the raw proof directly, so a Jolt verifier additionally
//! needs the ELF (or any bit-identical source of the preprocessing). Closing that
//! gap is a proof-compression question, not a struct-sizing one.
//!
//! Jolt verification also needs the guest's IO device (public inputs, outputs,
//! panic flag) as well as the proof, so [`JoltProof`] bundles both. That bundle
//! is what makes `verify` able to *return* the public values, which is the
//! contract Ere expects.

mod error;

use core::convert::Infallible;

use ere_verifier_core::codec::{Decode, Encode};
use jolt_sdk::{JoltDevice, JoltVerifierPreprocessing, RV64IMACProof};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use ere_verifier_core::{codec, PublicValues, zkVMVerifier};

pub use crate::error::Error;

/// Name reported through the Ere API.
pub const NAME: &str = "jolt";

/// Jolt publishes no SDK version; every number in this repo is tied to a commit,
/// so the commit is the version.
pub const SDK_VERSION: &str = "0.1.0+915faf453f36871249615a7fdf2704d77a88f259";

/// Length of a [`JoltProgramVk`], matching SP1's and ZisK's.
pub const PROGRAM_VK_SIZE: usize = 32;

/// Verifying key identifying a specific compiled guest program: a SHA-256 digest
/// of that program's serialized [`JoltVerifierPreprocessing`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JoltProgramVk(pub [u8; PROGRAM_VK_SIZE]);

impl JoltProgramVk {
    /// Derives the key from a program's verifier preprocessing.
    pub fn from_preprocessing(preprocessing: &JoltVerifierPreprocessing) -> Result<Self, Error> {
        let encoded = bincode::serde::encode_to_vec(preprocessing, bincode::config::legacy())?;
        let digest: [u8; PROGRAM_VK_SIZE] = Sha256::digest(&encoded).into();
        Ok(Self(digest))
    }
}

impl Encode for JoltProgramVk {
    type Error = Infallible;

    fn encode_to_vec(&self) -> Result<Vec<u8>, Self::Error> {
        Ok(self.0.to_vec())
    }
}

impl Decode for JoltProgramVk {
    type Error = Error;

    fn decode_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        slice
            .try_into()
            .map(Self)
            .map_err(|_| Error::InvalidProgramVkLength {
                expected: PROGRAM_VK_SIZE,
                got: slice.len(),
            })
    }
}

ere_verifier_core::codec::impl_try_from_bytes_by_decode!(JoltProgramVk);
ere_verifier_core::codec::impl_into_bytes_by_encode!(JoltProgramVk);

/// A Jolt proof together with the IO device it was produced against.
#[derive(Clone, Serialize, Deserialize)]
pub struct JoltProof {
    /// The proof itself.
    pub proof: RV64IMACProof,
    /// Public inputs, outputs and panic flag, as the guest left them.
    pub io_device: JoltDevice,
}

impl core::fmt::Debug for JoltProof {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("JoltProof")
            .field("output_len", &self.io_device.outputs.len())
            .field("panic", &self.io_device.panic)
            .finish_non_exhaustive()
    }
}

ere_verifier_core::codec::impl_codec_by_bincode_legacy!(JoltProof, reject_trailing_bytes);

/// Verifier bound to a specific compiled guest program.
///
/// Holds the preprocessing verification actually needs, and publishes only its
/// digest as the [`JoltProgramVk`].
#[derive(Clone)]
pub struct JoltVerifier {
    program_vk: JoltProgramVk,
    preprocessing: JoltVerifierPreprocessing,
}

impl core::fmt::Debug for JoltVerifier {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("JoltVerifier")
            .field("program_vk", &self.program_vk)
            .finish_non_exhaustive()
    }
}

impl JoltVerifier {
    /// Creates a verifier from a program's verifier preprocessing, deriving the
    /// key from it.
    pub fn new(preprocessing: JoltVerifierPreprocessing) -> Result<Self, Error> {
        let program_vk = JoltProgramVk::from_preprocessing(&preprocessing)?;
        Ok(Self {
            program_vk,
            preprocessing,
        })
    }

    /// Creates a verifier from preprocessing that must match `expected`.
    ///
    /// This is the check that makes the key binding: a caller who holds only the
    /// 32-byte key can re-derive preprocessing from the guest ELF and confirm it
    /// is the preprocessing that key names, rather than trusting whatever it was
    /// handed.
    pub fn new_checked(
        preprocessing: JoltVerifierPreprocessing,
        expected: JoltProgramVk,
    ) -> Result<Self, Error> {
        let verifier = Self::new(preprocessing)?;
        if verifier.program_vk != expected {
            return Err(Error::ProgramVkMismatch {
                expected: expected.0,
                got: verifier.program_vk.0,
            });
        }
        Ok(verifier)
    }
}

impl zkVMVerifier for JoltVerifier {
    type ProgramVk = JoltProgramVk;
    type Proof = JoltProof;
    type Error = Error;

    fn program_vk(&self) -> &JoltProgramVk {
        &self.program_vk
    }

    fn verify(&self, proof: &JoltProof) -> Result<PublicValues, Error> {
        jolt_sdk::jolt_verifier::verify::<
            jolt_sdk::VerifierField,
            jolt_sdk::VerifierPCS,
            jolt_sdk::VerifierVC,
            jolt_sdk::VerifierTranscript,
        >(&self.preprocessing, &proof.io_device, &proof.proof, None)
        .map_err(|err| Error::Verification(format!("{err:?}")))?;

        // A guest that panicked proved nothing about its outputs, so refuse to
        // hand them back as public values.
        if proof.io_device.panic {
            return Err(Error::GuestPanicked);
        }
        // Padded to the guest's declared output size for the same reason the
        // prover pads: a serialized value ending in zeroes is otherwise handed
        // back truncated and cannot be decoded.
        let mut outputs = proof.io_device.outputs.clone();
        outputs.resize(proof.io_device.memory_layout.max_output_size as usize, 0);
        Ok(outputs.into())
    }

    fn name(&self) -> &'static str {
        NAME
    }

    fn sdk_version(&self) -> &'static str {
        SDK_VERSION
    }
}
