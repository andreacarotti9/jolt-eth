use thiserror::Error;

use crate::MAX_TRACE_LENGTH_ENV;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to preprocess guest program: {0}")]
    Preprocessing(String),

    #[error("failed to produce proof: {0}")]
    Proving(String),

    #[error(
        "guest trace is {actual} cycles but the prover was built for {bound}; \
         raise {MAX_TRACE_LENGTH_ENV}"
    )]
    TraceTooLong { actual: usize, bound: usize },

    #[error("Jolt has no proof-composition support at this pin, so Input::proofs is unsupported")]
    ProofCompositionUnsupported,

    #[error(transparent)]
    Verifier(#[from] ere_verifier_jolt::Error),
}
