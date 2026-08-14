#![cfg_attr(feature = "guest", no_std)]

//! S6: a synthetic loop whose trace length is linear in `iters`, so the host can
//! aim at a target trace length and find where Jolt's proving time and RAM knee.
//!
//! The body mixes ALU work with a data-dependent load/store so the trace is not
//! degenerately cheap - a pure arithmetic loop would flatter the RAM sumchecks.

use jolt::{end_cycle_tracking, start_cycle_tracking};

#[jolt::provable(
    stack_size = 1048576,
    heap_size = 8388608,
    max_trace_length = 1073741824
)]
fn spin(iters: u32) -> u64 {
    let mut buf = [0u64; 256];
    let mut x: u64 = 0x243F_6A88_85A3_08D3;

    start_cycle_tracking("spin_loop");
    for _ in 0..iters {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let idx = (x as usize) & 255;
        buf[idx] = buf[idx].wrapping_add(x);
    }
    end_cycle_tracking("spin_loop");

    buf.iter().fold(0u64, |a, b| a ^ b)
}
