#![cfg_attr(not(test), no_std)]

//! The eth-act zkvm-standards accelerator C ABI, implemented for Jolt.
//!
//! This is the guest-side half of Jolt's Ere backend and the counterpart of
//! SP1's `libzkevm`: `stateless-validator-{reth,ethrex}` compiled with the
//! `zkvm-interface` feature routes every hash, signature and curve operation
//! through these symbols.
//!
//! Four of them sit on Jolt inlines - keccak256, sha256, secp256k1 (verify and
//! [`secp256k1::ecrecover`]) and secp256r1 verify. The rest forward to
//! [`revm::precompile::DefaultCrypto`], the software backend the guest would
//! have used with the seam switched off, so turning the feature on can only ever
//! remove cycles, never add them. That also keeps the honest gaps honest: bn254
//! and BLS12-381 have no Jolt substrate at 915faf4, and the benchmark is
//! supposed to show what that costs rather than hide it.
//!
//! ABI note: these are `extern "C"` definitions matching
//! `standards/c-interface-accelerators/zkvm_accelerators.h`. `zkvm_status` is a C
//! enum with `ZKVM_EOK = 0` and `ZKVM_EFAIL = -1`, i.e. a 32-bit int.

extern crate alloc;

pub mod secp256k1;

use alloc::vec::Vec;

use revm::precompile::{
    bls12_381::{G1Point, G1PointScalar, G2Point, G2PointScalar},
    Crypto, DefaultCrypto,
};


/// Reads a C struct argument out from behind its raw pointer.
///
/// Rust 1.95 denies `dangerous_implicit_autorefs`, and every one of these
/// accelerators takes `*const zkvm_bytes_N`. Copying the fixed-size struct out
/// once is both cheaper to read and what the C contract already implies.
///
/// # Safety
/// `ptr` must be valid for reads of `T`.
#[inline(always)]
unsafe fn rd<T: Copy>(ptr: *const T) -> T {
    core::ptr::read(ptr)
}

/// `ZKVM_EOK`
const OK: i32 = 0;
/// `ZKVM_EFAIL`
const FAIL: i32 = -1;

macro_rules! bytes_struct {
    ($name:ident, $len:expr) => {
        #[repr(C, align(8))]
        #[derive(Clone, Copy)]
        pub struct $name {
            pub data: [u8; $len],
        }
    };
}

bytes_struct!(Bytes16, 16);
bytes_struct!(Bytes32, 32);
bytes_struct!(Bytes48, 48);
bytes_struct!(Bytes64, 64);
bytes_struct!(Bytes96, 96);
bytes_struct!(Bytes128, 128);
bytes_struct!(Bytes192, 192);

#[repr(C)]
pub struct Bn254PairingPair {
    pub g1: Bytes64,
    pub g2: Bytes128,
}

#[repr(C)]
pub struct Bls12G1MsmPair {
    pub point: Bytes96,
    pub scalar: Bytes32,
}

#[repr(C)]
pub struct Bls12G2MsmPair {
    pub point: Bytes192,
    pub scalar: Bytes32,
}

#[repr(C)]
pub struct Bls12PairingPair {
    pub g1: Bytes96,
    pub g2: Bytes192,
}

/// `alloy-primitives`' `native-keccak` feature calls this symbol for every
/// `keccak256`, which is the single highest-volume hash in block validation:
/// MPT node hashing, storage slots, `CREATE` addresses and the EVM opcode.
///
/// # Safety
/// `bytes` must be readable for `len`, and `output` writable for 32 bytes.
#[no_mangle]
pub unsafe extern "C" fn native_keccak256(bytes: *const u8, len: usize, output: *mut u8) {
    let input = core::slice::from_raw_parts(bytes, len);
    let hash = jolt_inlines_keccak256::Keccak256::digest(input);
    core::ptr::copy_nonoverlapping(hash.as_ptr(), output, 32);
}

/// # Safety
/// `data` must be readable for `len` and `output` writable.
#[no_mangle]
pub unsafe extern "C" fn zkvm_keccak256(
    data: *const u8,
    len: usize,
    output: *mut Bytes32,
) -> i32 {
    let input = core::slice::from_raw_parts(data, len);
    (*output).data = jolt_inlines_keccak256::Keccak256::digest(input);
    OK
}

/// # Safety
/// `data` must be readable for `len` and `output` writable.
#[no_mangle]
pub unsafe extern "C" fn zkvm_sha256(data: *const u8, len: usize, output: *mut Bytes32) -> i32 {
    let input = core::slice::from_raw_parts(data, len);
    (*output).data = jolt_inlines_sha2::Sha256::digest(input);
    OK
}

/// # Safety
/// All pointers must be valid for their types.
#[no_mangle]
pub unsafe extern "C" fn zkvm_secp256k1_verify(
    msg: *const Bytes32,
    sig: *const Bytes64,
    pubkey: *const Bytes64,
    verified: *mut bool,
) -> i32 {
    *verified = secp256k1::verify(&rd(msg).data, &rd(sig).data, &rd(pubkey).data);
    OK
}

/// # Safety
/// All pointers must be valid for their types.
#[no_mangle]
pub unsafe extern "C" fn zkvm_secp256k1_ecrecover(
    msg: *const Bytes32,
    sig: *const Bytes64,
    recid: u8,
    output: *mut Bytes64,
) -> i32 {
    match secp256k1::ecrecover(&rd(msg).data, &rd(sig).data, recid) {
        Some(pubkey) => {
            (*output).data = pubkey;
            OK
        }
        None => FAIL,
    }
}

/// # Safety
/// All pointers must be valid for their types.
#[no_mangle]
pub unsafe extern "C" fn zkvm_secp256r1_verify(
    msg: *const Bytes32,
    sig: *const Bytes64,
    pubkey: *const Bytes64,
    verified: *mut bool,
) -> i32 {
    *verified = p256_verify(&rd(msg).data, &rd(sig).data, &rd(pubkey).data);
    OK
}

/// secp256r1 verification on Jolt's p256 inline, which already exposes exactly
/// the operation RIP-7212 needs.
fn p256_verify(msg: &[u8; 32], sig: &[u8; 64], pubkey: &[u8; 64]) -> bool {
    use jolt_inlines_p256::{P256Fq, P256Fr, P256Point};

    use crate::secp256k1::{be_to_limbs, sub_if_ge};

    /// secp256r1 group order, little-endian limbs.
    const P256_N: [u64; 4] = [
        0xF3B9_CAC2_FC63_2551,
        0xBCE6_FAAD_A717_9E84,
        0xFFFF_FFFF_FFFF_FFFF,
        0xFFFF_FFFF_0000_0000,
    ];

    let limbs = |bytes: &[u8]| -> Option<[u64; 4]> {
        Some(be_to_limbs(bytes.try_into().ok()?))
    };
    let run = || -> Option<bool> {
        // The message hash is 256 bits and may legitimately exceed n; every ECDSA
        // implementation reduces rather than rejects. Rejecting would make a
        // ~2^-32 fraction of valid signatures verify as invalid, which is a
        // consensus bug waiting for the block that triggers it.
        let z = P256Fr::from_u64_arr(&sub_if_ge(be_to_limbs(msg), P256_N)).ok()?;
        let r = P256Fr::from_u64_arr(&limbs(&sig[..32])?).ok()?;
        let s = P256Fr::from_u64_arr(&limbs(&sig[32..])?).ok()?;
        let q = P256Point::new_unchecked(
            P256Fq::from_u64_arr(&limbs(&pubkey[..32])?).ok()?,
            P256Fq::from_u64_arr(&limbs(&pubkey[32..])?).ok()?,
        );
        Some(jolt_inlines_p256::ecdsa_verify(z, r, s, q).is_ok())
    };
    run().unwrap_or(false)
}

// ---------------------------------------------------------------------------
// No Jolt substrate at 915faf4: forwarded to revm's software backend.
// ---------------------------------------------------------------------------

/// # Safety
/// `data` readable for `len`, `output` writable.
#[no_mangle]
pub unsafe extern "C" fn zkvm_ripemd160(
    data: *const u8,
    len: usize,
    output: *mut Bytes32,
) -> i32 {
    let input = core::slice::from_raw_parts(data, len);
    (*output).data = DefaultCrypto.ripemd160(input);
    OK
}

/// # Safety
/// Each pointer/length pair must describe a readable slice; `output` must be
/// writable for `mod_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn zkvm_modexp(
    base: *const u8,
    base_len: usize,
    exp: *const u8,
    exp_len: usize,
    modulus: *const u8,
    mod_len: usize,
    output: *mut u8,
) -> i32 {
    let base = core::slice::from_raw_parts(base, base_len);
    let exp = core::slice::from_raw_parts(exp, exp_len);
    let modulus = core::slice::from_raw_parts(modulus, mod_len);
    match DefaultCrypto.modexp(base, exp, modulus) {
        Ok(result) => {
            // EIP-198 wants exactly `mod_len` big-endian bytes. The backend may
            // hand back a shorter minimal encoding, so right-align rather than
            // reject: treating a short result as failure halts the precompile
            // and silently changes the block's state root.
            let out = core::slice::from_raw_parts_mut(output, mod_len);
            out.fill(0);
            let n = result.len().min(mod_len);
            out[mod_len - n..].copy_from_slice(&result[result.len() - n..]);
            OK
        }
        Err(_) => FAIL,
    }
}

/// # Safety
/// All pointers must be valid for their types.
#[no_mangle]
pub unsafe extern "C" fn zkvm_bn254_g1_add(
    p1: *const Bytes64,
    p2: *const Bytes64,
    result: *mut Bytes64,
) -> i32 {
    match DefaultCrypto.bn254_g1_add(&rd(p1).data, &rd(p2).data) {
        Ok(point) => {
            (*result).data = point;
            OK
        }
        Err(_) => FAIL,
    }
}

/// # Safety
/// All pointers must be valid for their types.
#[no_mangle]
pub unsafe extern "C" fn zkvm_bn254_g1_mul(
    point: *const Bytes64,
    scalar: *const Bytes32,
    result: *mut Bytes64,
) -> i32 {
    match DefaultCrypto.bn254_g1_mul(&rd(point).data, &rd(scalar).data) {
        Ok(out) => {
            (*result).data = out;
            OK
        }
        Err(_) => FAIL,
    }
}

/// # Safety
/// `pairs` must point to `num_pairs` initialised elements.
#[no_mangle]
pub unsafe extern "C" fn zkvm_bn254_pairing(
    pairs: *const Bn254PairingPair,
    num_pairs: usize,
    verified: *mut bool,
) -> i32 {
    let pairs = core::slice::from_raw_parts(pairs, num_pairs);
    let borrowed: Vec<(&[u8], &[u8])> = pairs
        .iter()
        .map(|p| (p.g1.data.as_slice(), p.g2.data.as_slice()))
        .collect();
    match DefaultCrypto.bn254_pairing_check(&borrowed) {
        Ok(ok) => {
            *verified = ok;
            OK
        }
        Err(_) => FAIL,
    }
}

/// # Safety
/// All pointers must be valid for their types.
#[no_mangle]
pub unsafe extern "C" fn zkvm_blake2f(
    rounds: u32,
    h: *mut Bytes64,
    m: *const Bytes128,
    t: *const Bytes16,
    f: u8,
) -> i32 {
    let h_bytes = rd(h as *const Bytes64).data;
    let m_bytes = rd(m).data;
    let t_bytes = rd(t).data;

    let mut state = [0u64; 8];
    for (i, word) in state.iter_mut().enumerate() {
        *word = u64::from_le_bytes(h_bytes[8 * i..8 * i + 8].try_into().unwrap());
    }
    let mut message = [0u64; 16];
    for (i, word) in message.iter_mut().enumerate() {
        *word = u64::from_le_bytes(m_bytes[8 * i..8 * i + 8].try_into().unwrap());
    }
    let offset = [
        u64::from_le_bytes(t_bytes[..8].try_into().unwrap()),
        u64::from_le_bytes(t_bytes[8..].try_into().unwrap()),
    ];

    DefaultCrypto.blake2_compress(rounds, &mut state, &message, &offset, f != 0);

    let mut out = [0u8; 64];
    for (i, word) in state.iter().enumerate() {
        out[8 * i..8 * i + 8].copy_from_slice(&word.to_le_bytes());
    }
    core::ptr::write(h, Bytes64 { data: out });
    OK
}

/// # Safety
/// All pointers must be valid for their types.
#[no_mangle]
pub unsafe extern "C" fn zkvm_kzg_point_eval(
    commitment: *const Bytes48,
    z: *const Bytes32,
    y: *const Bytes32,
    proof: *const Bytes48,
    verified: *mut bool,
) -> i32 {
    let ok = DefaultCrypto
        .verify_kzg_proof(&rd(z).data, &rd(y).data, &rd(commitment).data, &rd(proof).data)
        .is_ok();
    *verified = ok;
    OK
}

/// # Safety
/// All pointers must be valid for their types.
#[no_mangle]
pub unsafe extern "C" fn zkvm_bls12_g1_add(
    p1: *const Bytes96,
    p2: *const Bytes96,
    result: *mut Bytes96,
) -> i32 {
    match DefaultCrypto.bls12_381_g1_add(split_g1(&rd(p1).data), split_g1(&rd(p2).data)) {
        Ok(point) => {
            (*result).data = point;
            OK
        }
        Err(_) => FAIL,
    }
}

/// # Safety
/// `pairs` must point to `num_pairs` initialised elements.
#[no_mangle]
pub unsafe extern "C" fn zkvm_bls12_g1_msm(
    pairs: *const Bls12G1MsmPair,
    num_pairs: usize,
    result: *mut Bytes96,
) -> i32 {
    let pairs = core::slice::from_raw_parts(pairs, num_pairs);
    let mut items = pairs
        .iter()
        .map(|p| -> Result<G1PointScalar, _> { Ok((split_g1(&p.point.data), p.scalar.data)) });
    match DefaultCrypto.bls12_381_g1_msm(&mut items) {
        Ok(point) => {
            (*result).data = point;
            OK
        }
        Err(_) => FAIL,
    }
}

/// # Safety
/// All pointers must be valid for their types.
#[no_mangle]
pub unsafe extern "C" fn zkvm_bls12_g2_add(
    p1: *const Bytes192,
    p2: *const Bytes192,
    result: *mut Bytes192,
) -> i32 {
    match DefaultCrypto.bls12_381_g2_add(split_g2(&rd(p1).data), split_g2(&rd(p2).data)) {
        Ok(point) => {
            (*result).data = point;
            OK
        }
        Err(_) => FAIL,
    }
}

/// # Safety
/// `pairs` must point to `num_pairs` initialised elements.
#[no_mangle]
pub unsafe extern "C" fn zkvm_bls12_g2_msm(
    pairs: *const Bls12G2MsmPair,
    num_pairs: usize,
    result: *mut Bytes192,
) -> i32 {
    let pairs = core::slice::from_raw_parts(pairs, num_pairs);
    let mut items = pairs
        .iter()
        .map(|p| -> Result<G2PointScalar, _> { Ok((split_g2(&p.point.data), p.scalar.data)) });
    match DefaultCrypto.bls12_381_g2_msm(&mut items) {
        Ok(point) => {
            (*result).data = point;
            OK
        }
        Err(_) => FAIL,
    }
}

/// # Safety
/// `pairs` must point to `num_pairs` initialised elements.
#[no_mangle]
pub unsafe extern "C" fn zkvm_bls12_pairing(
    pairs: *const Bls12PairingPair,
    num_pairs: usize,
    verified: *mut bool,
) -> i32 {
    let pairs = core::slice::from_raw_parts(pairs, num_pairs);
    let borrowed: Vec<(G1Point, G2Point)> = pairs
        .iter()
        .map(|p| (split_g1(&p.g1.data), split_g2(&p.g2.data)))
        .collect();
    match DefaultCrypto.bls12_381_pairing_check(&borrowed) {
        Ok(ok) => {
            *verified = ok;
            OK
        }
        Err(_) => FAIL,
    }
}

/// # Safety
/// All pointers must be valid for their types.
#[no_mangle]
pub unsafe extern "C" fn zkvm_bls12_map_fp_to_g1(
    field_element: *const Bytes48,
    result: *mut Bytes96,
) -> i32 {
    match DefaultCrypto.bls12_381_fp_to_g1(&rd(field_element).data) {
        Ok(point) => {
            (*result).data = point;
            OK
        }
        Err(_) => FAIL,
    }
}

/// # Safety
/// All pointers must be valid for their types.
#[no_mangle]
pub unsafe extern "C" fn zkvm_bls12_map_fp2_to_g2(
    field_element: *const Bytes96,
    result: *mut Bytes192,
) -> i32 {
    let fp2 = (
        rd(field_element).data[..48].try_into().unwrap(),
        rd(field_element).data[48..].try_into().unwrap(),
    );
    match DefaultCrypto.bls12_381_fp2_to_g2(fp2) {
        Ok(point) => {
            (*result).data = point;
            OK
        }
        Err(_) => FAIL,
    }
}

fn split_g1(bytes: &[u8; 96]) -> G1Point {
    (
        bytes[..48].try_into().unwrap(),
        bytes[48..].try_into().unwrap(),
    )
}

fn split_g2(bytes: &[u8; 192]) -> G2Point {
    (
        bytes[..48].try_into().unwrap(),
        bytes[48..96].try_into().unwrap(),
        bytes[96..144].try_into().unwrap(),
        bytes[144..].try_into().unwrap(),
    )
}

#[cfg(test)]
mod tests {
    use super::{zkvm_modexp, Bytes32};

    /// `zkvm_modexp` must always fill `mod_len` bytes, right-aligned, whatever
    /// length the backend returns. Rejecting a short result made the EIP-198
    /// `mod-even-declared-length-128-bytes` fixture produce the wrong state root.
    #[test]
    fn modexp_right_aligns_into_the_declared_modulus_length() {
        // 3^2 mod 1000 = 9, declared over 4 bytes.
        let (base, exp, modulus) = ([3u8], [2u8], [0u8, 0, 3, 232]);
        let mut out = [0xAAu8; 4];
        let status = unsafe {
            zkvm_modexp(
                base.as_ptr(),
                base.len(),
                exp.as_ptr(),
                exp.len(),
                modulus.as_ptr(),
                modulus.len(),
                out.as_mut_ptr(),
            )
        };
        assert_eq!(status, 0, "modexp reported failure");
        assert_eq!(out, [0, 0, 0, 9], "result must be zero-padded big-endian");
        let _ = core::mem::size_of::<Bytes32>();
    }

    /// secp256r1 verification must agree with the `p256` crate, accepting the
    /// same signatures and rejecting the same tampered ones. RIP-7212 makes this
    /// consensus-critical the moment a chain enables the precompile.
    #[test]
    fn p256_verify_matches_rustcrypto() {
        use p256::{
            ecdsa::{signature::hazmat::PrehashVerifier, SigningKey, VerifyingKey},
            elliptic_curve::sec1::ToSec1Point,
        };

        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || -> [u8; 32] {
            let mut out = [0u8; 32];
            for chunk in out.chunks_mut(8) {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                chunk.copy_from_slice(&seed.to_le_bytes());
            }
            out
        };

        let mut checked = 0;
        for _ in 0..32 {
            let Ok(sk) = SigningKey::from_slice(&next()) else {
                continue;
            };
            let msg = next();
            let sig: p256::ecdsa::Signature =
                p256::ecdsa::signature::hazmat::PrehashSigner::sign_prehash(&sk, &msg)
                    .expect("sign");
            let vk = VerifyingKey::from(&sk);
            let point = vk.as_affine().to_sec1_point(false);
            let pubkey: [u8; 64] = point.as_bytes()[1..].try_into().unwrap();
            let sig_bytes: [u8; 64] = sig.to_bytes().into();

            assert!(vk.verify_prehash(&msg, &sig).is_ok(), "p256 crate rejected its own signature");
            assert!(super::p256_verify(&msg, &sig_bytes, &pubkey), "inline rejected a valid signature");

            // Flip one bit of the message: both must reject.
            let mut tampered = msg;
            tampered[0] ^= 1;
            assert!(!super::p256_verify(&tampered, &sig_bytes, &pubkey), "inline accepted a tampered message");
            checked += 1;
        }
        assert!(checked >= 30, "only {checked} signatures exercised");
    }

    /// The inline hashes must agree with the crates they displace, or every
    /// state root downstream is wrong.
    #[test]
    fn inline_hashes_match_rustcrypto() {
        for len in [0usize, 1, 55, 56, 64, 135, 136, 137, 1000] {
            let data: Vec<u8> = (0..len).map(|i| (i * 31 + 7) as u8).collect();

            let keccak = jolt_inlines_keccak256::Keccak256::digest(&data);
            let expected: [u8; 32] = {
                use sha3::Digest as _;
                sha3::Keccak256::digest(&data).into()
            };
            assert_eq!(keccak, expected, "keccak256 mismatch at len {len}");

            let sha = jolt_inlines_sha2::Sha256::digest(&data);
            let expected: [u8; 32] = {
                use sha2::Digest as _;
                sha2::Sha256::digest(&data).into()
            };
            assert_eq!(sha, expected, "sha256 mismatch at len {len}");
        }
    }
}

/// Keeps the inline crates' `inventory` registrations in the host binary.
///
/// The tracer expands a guest's inline instructions by looking up
/// `(opcode, funct3, funct7)` in an `inventory` registry that each
/// `jolt-inlines-*` crate populates from its `host` module. Registrations live in
/// rlib object files the linker will drop unless something references them, and
/// a host that merely *depends* on this crate references nothing - so decoding an
/// accelerated ELF fails with `Expansion(UnsupportedInstruction)`.
///
/// Call this once before decoding a guest that uses these inlines.
#[cfg(feature = "host")]
pub fn register_host_inlines() {
    let _ = jolt_inlines_keccak256::store_inlines as fn() -> Result<(), alloc::string::String>;
    let _ = jolt_inlines_sha2::store_inlines as fn() -> Result<(), alloc::string::String>;
    let _ = jolt_inlines_secp256k1::store_inlines as fn() -> Result<(), alloc::string::String>;
    let _ = jolt_inlines_p256::store_inlines as fn() -> Result<(), alloc::string::String>;
}
