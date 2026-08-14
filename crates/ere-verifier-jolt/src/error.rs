use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("jolt proof verification failed: {0}")]
    Verification(String),

    #[error("guest panicked; its outputs are not proven")]
    GuestPanicked,

    #[error("program verifying key must be {expected} bytes, got {got}")]
    InvalidProgramVkLength { expected: usize, got: usize },

    #[error(
        "preprocessing does not match the expected program verifying key \
         (expected {}, got {})",
        const_hex::encode(expected),
        const_hex::encode(got)
    )]
    ProgramVkMismatch {
        expected: [u8; 32],
        got: [u8; 32],
    },

    #[error("failed to encode: {0}")]
    Encode(#[from] bincode::error::EncodeError),

    #[error("failed to decode: {0}")]
    Decode(#[from] bincode::error::DecodeError),
}
