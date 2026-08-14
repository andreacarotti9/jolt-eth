//! secp256k1 ECDSA recovery and verification on Jolt's secp256k1 inlines.
//!
//! `jolt-inlines-secp256k1` ships `ecdsa_verify` but no `recover` (checked at
//! 915faf4), and ecrecover is the single hottest precompile in Ethereum block
//! validation - every transaction pays for one. This module fills that gap using
//! only the inline crate's public API, so it can be lifted upstream as-is.
//!
//! Recovery is the textbook construction:
//!
//! ```text
//! x  = r  (+ n when recid >= 2)
//! y  = sqrt(x^3 + 7), sign chosen so that y's parity matches recid & 1
//! R  = (x, y), rejected unless it is on the curve
//! Q  = (s/r) * R + (-z/r) * G
//! ```
//!
//! `p = 3 (mod 4)` for secp256k1, so the square root is one fixed-exponent
//! power - about 500 inline field operations - and needs no advice tape. The
//! plan's advice-hinted variant would trade those for a tape read plus one
//! squaring; [`crate::secp256k1`]'s tests are written so that variant can be
//! swapped in behind the same signature and differentially tested the same way.

use jolt_inlines_sdk::ec::ECField;
use jolt_inlines_secp256k1::{Secp256k1Fq, Secp256k1Fr, Secp256k1Point, Secp256k1PointExt};

/// `(p + 1) / 4`, the square-root exponent for `p = 3 (mod 4)`, little-endian limbs.
const SQRT_EXP: [u64; 4] = [
    0xFFFF_FFFF_BFFF_FF0C,
    0xFFFF_FFFF_FFFF_FFFF,
    0xFFFF_FFFF_FFFF_FFFF,
    0x3FFF_FFFF_FFFF_FFFF,
];

/// Base field modulus `p`, little-endian limbs.
const P: [u64; 4] = [
    0xFFFF_FFFE_FFFF_FC2F,
    0xFFFF_FFFF_FFFF_FFFF,
    0xFFFF_FFFF_FFFF_FFFF,
    0xFFFF_FFFF_FFFF_FFFF,
];

/// Scalar field modulus `n`, little-endian limbs.
const N: [u64; 4] = [
    0xBFD2_5E8C_D036_4141,
    0xBAAE_DCE6_AF48_A03B,
    0xFFFF_FFFF_FFFF_FFFE,
    0xFFFF_FFFF_FFFF_FFFF,
];

/// Recovers the uncompressed public key (`x || y`, big-endian, 64 bytes) from a
/// signature, or `None` if no such key exists.
///
/// `msg` is the 32-byte message hash, `sig` is `r || s` big-endian, and `recid`
/// is the 0..=3 recovery id (Ethereum's `v - 27`).
pub fn ecrecover(msg: &[u8; 32], sig: &[u8; 64], recid: u8) -> Option<[u8; 64]> {
    if recid > 3 {
        return None;
    }

    let r_limbs = be_to_limbs(sig[..32].try_into().ok()?);
    let s_limbs = be_to_limbs(sig[32..].try_into().ok()?);
    let z_limbs = be_to_limbs(msg);

    // r and s must be canonical and non-zero; a zero r has no matching point.
    let r = Secp256k1Fr::from_u64_arr(&r_limbs).ok()?;
    let s = Secp256k1Fr::from_u64_arr(&s_limbs).ok()?;
    if r.is_zero() || s.is_zero() {
        return None;
    }
    // z is reduced mod n rather than rejected: the message hash is 256 bits and
    // may legitimately exceed n, which is what every ECDSA implementation does.
    let z = Secp256k1Fr::from_u64_arr(&sub_if_ge(z_limbs, N)).ok()?;

    // recid >= 2 means the x coordinate wrapped: x = r + n, valid only if it is
    // still below p.
    let x_limbs = if recid >= 2 {
        let (sum, carry) = add_limbs(r_limbs, N);
        if carry || !lt_limbs(sum, P) {
            return None;
        }
        sum
    } else {
        r_limbs
    };

    let x = Secp256k1Fq::from_u64_arr(&x_limbs).ok()?;
    let y = y_from_x(&x, recid & 1 == 1)?;

    let point = Secp256k1Point::new_unchecked(x, y);
    if !point.is_on_curve() || point.is_infinity() {
        return None;
    }

    // Q = (s/r) * R - (z/r) * G, computed as u2 * R + u1 * G. `div` rather than
    // `div_assume_nonzero` because only `div` is public off the host feature;
    // r is already known non-zero, so its guard never fires.
    let u1 = z.div(&r).neg();
    let u2 = s.div(&r);
    let q = shamir(&u1, &Secp256k1Point::generator(), &u2, &point);
    if q.is_infinity() {
        return None;
    }

    let mut out = [0u8; 64];
    out[..32].copy_from_slice(&limbs_to_be(&q.x().to_u64_arr()));
    out[32..].copy_from_slice(&limbs_to_be(&q.y().to_u64_arr()));
    Some(out)
}

/// Verifies an ECDSA signature against an uncompressed public key (`x || y`).
pub fn verify(msg: &[u8; 32], sig: &[u8; 64], pubkey: &[u8; 64]) -> bool {
    let to_fr = |b: &[u8]| -> Option<Secp256k1Fr> {
        Secp256k1Fr::from_u64_arr(&be_to_limbs(b.try_into().ok()?)).ok()
    };
    let to_fq = |b: &[u8]| -> Option<Secp256k1Fq> {
        Secp256k1Fq::from_u64_arr(&be_to_limbs(b.try_into().ok()?)).ok()
    };
    let run = || -> Option<bool> {
        let z_limbs = sub_if_ge(be_to_limbs(msg), N);
        let z = Secp256k1Fr::from_u64_arr(&z_limbs).ok()?;
        let r = to_fr(&sig[..32])?;
        let s = to_fr(&sig[32..])?;
        let q = Secp256k1Point::new_unchecked(to_fq(&pubkey[..32])?, to_fq(&pubkey[32..])?);
        Some(jolt_inlines_secp256k1::ecdsa_verify(z, r, s, q).is_ok())
    };
    run().unwrap_or(false)
}

/// The curve point's y for a given x, with the requested parity, or `None` when
/// `x^3 + 7` is not a quadratic residue.
fn y_from_x(x: &Secp256k1Fq, want_odd: bool) -> Option<Secp256k1Fq> {
    let rhs = x.square().mul(x).add(&Secp256k1Fq::seven());
    let y = pow(&rhs, &SQRT_EXP);
    // The exponentiation only yields a root when one exists; otherwise it lands
    // on some other element, so squaring back is the membership test.
    if y.square() != rhs {
        return None;
    }
    let is_odd = y.to_u64_arr()[0] & 1 == 1;
    Some(if is_odd == want_odd { y } else { y.neg() })
}

/// Square-and-multiply with a fixed public exponent.
fn pow(base: &Secp256k1Fq, exp: &[u64; 4]) -> Secp256k1Fq {
    // 1 as a field element; `Fq::seven().div(seven)` would cost an inversion, so
    // start from the base and skip the leading bit instead.
    let mut acc: Option<Secp256k1Fq> = None;
    for limb_index in (0..4).rev() {
        for bit in (0..64).rev() {
            acc = Some(match acc {
                None => {
                    if (exp[limb_index] >> bit) & 1 == 1 {
                        base.clone()
                    } else {
                        continue;
                    }
                }
                Some(a) => {
                    let sq = a.square();
                    if (exp[limb_index] >> bit) & 1 == 1 {
                        sq.mul(base)
                    } else {
                        sq
                    }
                }
            });
        }
    }
    acc.unwrap_or_else(Secp256k1Fq::zero)
}

/// `k1 * p1 + k2 * p2` by interleaved double-and-add, with each scalar first cut
/// to 128 bits by the GLV endomorphism - four half-width scalars sharing one
/// doubling chain instead of two full-width ones.
fn shamir(
    k1: &Secp256k1Fr,
    p1: &Secp256k1Point,
    k2: &Secp256k1Fr,
    p2: &Secp256k1Point,
) -> Secp256k1Point {
    let mut scalars = [0u128; 4];
    let mut points: [Secp256k1Point; 4] = [
        Secp256k1Point::infinity(),
        Secp256k1Point::infinity(),
        Secp256k1Point::infinity(),
        Secp256k1Point::infinity(),
    ];

    for (i, (k, p)) in [(k1, p1), (k2, p2)].into_iter().enumerate() {
        let parts = k.glv_decompose();
        let base = [p.clone(), p.endomorphism()];
        for (j, ((negative, magnitude), point)) in parts.into_iter().zip(base).enumerate() {
            scalars[2 * i + j] = magnitude;
            points[2 * i + j] = if negative { point.neg() } else { point };
        }
    }

    let mut acc = Secp256k1Point::infinity();
    for bit in (0..128).rev() {
        acc = acc.double();
        for (scalar, point) in scalars.iter().zip(points.iter()) {
            if (scalar >> bit) & 1 == 1 {
                acc = acc.add(point);
            }
        }
    }
    acc
}

pub(crate) fn be_to_limbs(bytes: &[u8; 32]) -> [u64; 4] {
    let mut limbs = [0u64; 4];
    for (i, limb) in limbs.iter_mut().enumerate() {
        let start = 24 - 8 * i;
        *limb = u64::from_be_bytes(bytes[start..start + 8].try_into().unwrap());
    }
    limbs
}

fn limbs_to_be(limbs: &[u64; 4]) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    for (i, limb) in limbs.iter().enumerate() {
        let start = 24 - 8 * i;
        bytes[start..start + 8].copy_from_slice(&limb.to_be_bytes());
    }
    bytes
}

pub(crate) fn add_limbs(a: [u64; 4], b: [u64; 4]) -> ([u64; 4], bool) {
    let mut out = [0u64; 4];
    let mut carry = false;
    for i in 0..4 {
        let (sum, c1) = a[i].overflowing_add(b[i]);
        let (sum, c2) = sum.overflowing_add(carry as u64);
        out[i] = sum;
        carry = c1 || c2;
    }
    (out, carry)
}

pub(crate) fn lt_limbs(a: [u64; 4], b: [u64; 4]) -> bool {
    for i in (0..4).rev() {
        if a[i] != b[i] {
            return a[i] < b[i];
        }
    }
    false
}

/// Reduces a 256-bit value modulo `m`, valid whenever `m > 2^255` so that one
/// conditional subtraction suffices - true for both curve orders here.
pub(crate) fn sub_if_ge(a: [u64; 4], m: [u64; 4]) -> [u64; 4] {
    if lt_limbs(a, m) {
        return a;
    }
    let mut out = [0u64; 4];
    let mut borrow = false;
    for i in 0..4 {
        let (diff, b1) = a[i].overflowing_sub(m[i]);
        let (diff, b2) = diff.overflowing_sub(borrow as u64);
        out[i] = diff;
        borrow = b1 || b2;
    }
    out
}

#[cfg(test)]
mod tests {
    use k256::{
        ecdsa::{SigningKey, VerifyingKey},
        elliptic_curve::sec1::ToSec1Point,
    };

    use super::{ecrecover, verify};

    /// Deterministic byte stream: the differential test must be reproducible from
    /// the repo alone, so no RNG.
    fn lcg(seed: u64) -> impl FnMut() -> [u8; 32] {
        let mut s = seed | 1;
        move || {
            let mut out = [0u8; 32];
            for chunk in out.chunks_mut(8) {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                chunk.copy_from_slice(&s.to_le_bytes());
            }
            out
        }
    }

    /// SEC1 uncompressed is `0x04 || x || y`; the C ABI wants just `x || y`.
    fn uncompressed(vk: &VerifyingKey) -> [u8; 64] {
        let point = vk.as_affine().to_sec1_point(false);
        let sec1 = point.as_bytes();
        assert_eq!(sec1.len(), 65, "expected uncompressed SEC1");
        sec1[1..].try_into().unwrap()
    }

    /// Every signature k256 can recover, we must recover identically. This is the
    /// differential test the plan makes mandatory before any published number.
    #[test]
    fn matches_k256_on_deterministic_signatures() {
        let mut next = lcg(0xEC5EED);
        let mut checked = 0;
        for _ in 0..64 {
            let Ok(sk) = SigningKey::from_slice(&next()) else {
                continue; // scalar out of range; astronomically rare, just skip
            };
            let msg = next();
            let (sig, recid) = sk.sign_prehash_recoverable(&msg);
            let expected = uncompressed(
                &VerifyingKey::recover_from_prehash(&msg, &sig, recid).expect("k256 recover"),
            );

            let sig_bytes: [u8; 64] = sig.to_bytes().into();
            let got = ecrecover(&msg, &sig_bytes, recid.to_byte()).expect("ecrecover");
            assert_eq!(got, expected, "recovery mismatch");
            assert!(verify(&msg, &sig_bytes, &expected), "verify");
            checked += 1;
        }
        assert!(checked >= 60, "only {checked} signatures exercised");
    }

    /// The wrong recovery id must never return the signer's key: if it could, any
    /// signature would impersonate any account.
    #[test]
    fn wrong_recid_never_returns_the_signer() {
        let mut next = lcg(7);
        let sk = SigningKey::from_slice(&next()).expect("key");
        let msg = next();
        let (sig, recid) = sk.sign_prehash_recoverable(&msg);
        let sig_bytes: [u8; 64] = sig.to_bytes().into();
        let truth = ecrecover(&msg, &sig_bytes, recid.to_byte()).expect("ecrecover");

        for other in 0..4u8 {
            if other == recid.to_byte() {
                continue;
            }
            assert_ne!(ecrecover(&msg, &sig_bytes, other), Some(truth));
        }
    }

    #[test]
    fn rejects_malformed_inputs() {
        let msg = [0x11u8; 32];
        // r = 0 and s = 0 are both unrecoverable.
        assert!(ecrecover(&msg, &[0u8; 64], 0).is_none());
        // recid out of range.
        assert!(ecrecover(&msg, &[0x22u8; 64], 4).is_none());
        // r >= n is not a valid signature component.
        let mut sig = [0u8; 64];
        sig[..32].copy_from_slice(&[0xFF; 32]);
        sig[32..].copy_from_slice(&[0x01; 32]);
        assert!(ecrecover(&msg, &sig, 0).is_none());
    }

    /// recid 2 or 3 means x wrapped past n; when r + n exceeds p there is no such
    /// point and recovery must fail rather than invent one.
    #[test]
    fn high_recid_is_rejected_when_x_exceeds_p() {
        let mut sig = [0u8; 64];
        sig[..32].copy_from_slice(
            &hex::decode("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364140")
                .unwrap(),
        );
        sig[32..].copy_from_slice(&[0x01; 32]);
        assert!(ecrecover(&[0x33u8; 32], &sig, 2).is_none());
    }
}
