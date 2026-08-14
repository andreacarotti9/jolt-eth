#![cfg_attr(feature = "guest", no_std)]

//! S2: how much witness can actually be handed to a Jolt guest through the
//! untrusted advice region, and what does consuming it cost?
//!
//! Untrusted advice is the right channel for a stateless-validation witness: the
//! guest re-derives the parent state root from it, so it is authenticated by the
//! computation rather than trusted by the verifier. At 915faf4 the region is
//! memory-mapped below `RAM_START_ADDRESS`, not a tape read instruction by
//! instruction, so `&[u8]` deserializes by borrow and costs no heap.
//!
//! `max_untrusted_advice_size` is asserted to be a power of two by
//! `MemoryLayout::new`, and the whole I/O region has to fit below
//! `RAM_START_ADDRESS` (2 GiB). Those are the two hard walls.

use jolt::{end_cycle_tracking, start_cycle_tracking};

/// Dense sums are metered over at most this much, so the strided pass over a big
/// region does not have to pay for a full linear scan too.
const DENSE_WINDOW: usize = 1 << 20;

#[jolt::provable(
    stack_size = 1048576,
    heap_size = 8388608,
    max_untrusted_advice_size = 67108864,
    max_trace_length = 268435456
)]
fn advice_sink(len: u32, advice: jolt::UntrustedAdvice<&[u8]>) -> u64 {
    let data: &[u8] = &advice;
    assert!(data.len() == len as usize, "advice length mismatch");

    // Strided touch proves the whole region is mapped and addressable without
    // paying a linear scan over 64 MiB.
    start_cycle_tracking("advice_touch_strided");
    let mut strided = 0u64;
    let mut i = 0usize;
    while i < data.len() {
        strided = strided.wrapping_add(data[i] as u64);
        i += 4096;
    }
    end_cycle_tracking("advice_touch_strided");

    let window = if data.len() < DENSE_WINDOW {
        data.len()
    } else {
        DENSE_WINDOW
    };
    start_cycle_tracking("advice_sum_dense");
    let mut dense = 0u64;
    for &b in &data[..window] {
        dense = dense.wrapping_add(b as u64);
    }
    end_cycle_tracking("advice_sum_dense");

    strided ^ dense
}
