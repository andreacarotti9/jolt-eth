#![cfg_attr(feature = "guest", no_std)]

//! S1: is the Jolt keccak256 inline reachable from ordinary guest code, and what
//! does it cost per byte against the software implementation it would replace?
//!
//! Both variants run in one trace so the two numbers come from the same build,
//! and the guest asserts they agree - the inline is only interesting if it is
//! also correct.

extern crate alloc;

use alloc::{vec, vec::Vec};

use jolt::{end_cycle_tracking, start_cycle_tracking};

/// Deterministic filler, so the host does not have to push megabytes through the
/// 4 KiB input region just to give keccak something to chew on.
fn fill(buf: &mut [u8], seed: u32) {
    let (mut s, a, c) = (seed, 1664525u32, 1013904223u32);
    for b in buf.iter_mut() {
        s = s.wrapping_mul(a).wrapping_add(c);
        *b = (s ^ (s >> 16)) as u8;
    }
}

#[jolt::provable(
    stack_size = 1048576,
    heap_size = 67108864,
    max_trace_length = 268435456
)]
fn keccak_bench(size_bytes: u32) -> [u8; 32] {
    let mut data: Vec<u8> = vec![0u8; size_bytes as usize];
    fill(&mut data, 0xC0FFEE);

    start_cycle_tracking("keccak_inline");
    let inline = jolt_inlines_keccak256::Keccak256::digest(&data);
    end_cycle_tracking("keccak_inline");

    start_cycle_tracking("keccak_soft");
    let soft: [u8; 32] = {
        use sha3::Digest as _;
        sha3::Keccak256::digest(&data).into()
    };
    end_cycle_tracking("keccak_soft");

    assert!(inline == soft, "inline keccak disagrees with sha3");
    inline
}
