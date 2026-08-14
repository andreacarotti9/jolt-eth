#![no_std]

//! Ere [`Platform`] implementation for the Jolt zkVM.
//!
//! Mirrors `ere-platform-sp1`: the guest-side half of an Ere backend, giving
//! `stateless-validator-*` its cycle scopes and host printing.
//!
//! One deliberate difference from the other three backends. OpenVM, SP1 and ZisK
//! export the zkvm-standards `read_input` / `write_output` C ABI symbols, so they
//! inherit [`Platform`]'s default implementations. Jolt has no ambient guest IO
//! at 915faf4: input and output are regions of the memory layout whose addresses
//! `#[jolt::provable]` computes and bakes into the generated entrypoint. So a
//! Jolt guest takes its bytes as a function argument and returns them, and these
//! two methods are wired to a diagnostic panic rather than a wrong answer. See
//! `guests/stateless-validator-jolt` for the shape that replaces them.

use core::ops::Deref;

pub use ere_platform_core::Platform;

/// [`Platform`] for guests running under Jolt.
#[derive(Clone, Copy, Debug, Default)]
pub struct JoltPlatform;

impl Platform for JoltPlatform {
    fn read_input() -> impl Deref<Target = [u8]> {
        panic!(
            "Jolt has no ambient guest input: take the bytes as an argument of a \
             #[jolt::provable] function (untrusted advice) instead of calling read_input"
        );
        // Unreachable, but the signature needs a concrete Deref target.
        #[allow(unreachable_code)]
        [].as_slice()
    }

    fn write_output(_output: &[u8]) {
        panic!(
            "Jolt has no ambient guest output: return the bytes from the \
             #[jolt::provable] function instead of calling write_output"
        );
    }

    #[inline]
    fn print(message: &str) {
        #[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
        jolt_platform::puts(message);
        #[cfg(not(any(target_arch = "riscv32", target_arch = "riscv64")))]
        let _ = message;
    }

    /// Jolt exposes no cycle counter to the guest; regions are reported by the
    /// tracer against the markers below, so `0` here is the honest answer.
    #[inline]
    fn cycle_count() -> u64 {
        0
    }

    #[inline]
    fn cycle_scope_start(name: &str) {
        jolt_platform::start_cycle_tracking(name);
    }

    #[inline]
    fn cycle_scope_end(name: &str) {
        jolt_platform::end_cycle_tracking(name);
    }
}

/// `critical-section` for a zkVM guest.
///
/// The guest is single-threaded and cannot be interrupted, so acquiring a
/// critical section is a no-op. Without this, linking any guest that reaches
/// `once_cell::sync::OnceCell` - which `reth_primitives_traits::SealedHeader`
/// does on every header hash - fails on `_critical_section_1_0_acquire`.
#[cfg(all(
    feature = "critical-section-impl",
    any(target_arch = "riscv32", target_arch = "riscv64")
))]
mod critical_section_impl {
    struct SingleThreadedGuest;

    critical_section::set_impl!(SingleThreadedGuest);

    unsafe impl critical_section::Impl for SingleThreadedGuest {
        unsafe fn acquire() -> critical_section::RawRestoreState {}
        unsafe fn release(_: critical_section::RawRestoreState) {}
    }
}
