#![cfg_attr(feature = "guest", no_std)]

//! The EF stateless validator (reth flavour) as a Jolt guest.
//!
//! The witness arrives as untrusted advice rather than committed input: stateless
//! validation already authenticates it against the parent state root, which is
//! exactly what untrusted advice is for, and the advice region is memory-mapped
//! so a `&[u8]` deserializes by borrow instead of copying megabytes onto the heap.
//!
//! Sizing note: `max_untrusted_advice_size` must be a power of two
//! (`MemoryLayout::new` asserts it), and the whole I/O region has to fit below
//! `RAM_START_ADDRESS` = 2 GiB.

extern crate alloc;

use alloc::vec::Vec;

pub use ere_platform_jolt::JoltPlatform;

#[jolt::provable(
    stack_size = 4194304,
    heap_size = 134217728,
    max_input_size = 4096,
    max_output_size = 4096,
    max_untrusted_advice_size = 16777216,
    max_trace_length = 1073741824
)]
fn validate_block(witness: jolt::UntrustedAdvice<&[u8]>) -> Vec<u8> {
    #[cfg(feature = "accel")]
    {
        // Links `jolt-zkvm-accel`'s C ABI symbols and points revm and alloy at
        // them. Idempotent, so paying for it once per invocation is fine.
        let _ = jolt_zkvm_accel::zkvm_keccak256 as usize;
        stateless_validator_reth::guest::crypto::zkvm_interface::install_crypto();
    }
    stateless_validator_reth::guest::run_stateless_guest::<JoltPlatform>(&witness)
}
