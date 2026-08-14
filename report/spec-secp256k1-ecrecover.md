# Spec: secp256k1 public key recovery (ecrecover)

| Field       | Value                          |
|-------------|--------------------------------|
| Author(s)   | @andreacarotti9                |
| Created     | 2026-08-14                     |
| Status      | proposed                       |
| PR          |                                |

## Summary

`jolt-inlines-secp256k1` provides field arithmetic, GLV decomposition and
`ecdsa_verify`, but no public key recovery. Recovery is the single highest
volume elliptic-curve operation in Ethereum block validation, since every
transaction's sender is recovered rather than supplied, and the `ecrecover`
precompile is reachable from any contract. Today a Jolt guest doing either must
fall back to a software implementation; both `examples/recover-ecdsa` and
`examples/sig-recovery` do exactly that, via `secp256k1` and reth's
`recover_signers` respectively.

This spec proposes adding `ecrecover` to the existing SDK, built entirely from
inlines that already exist. No new opcode is required: for secp256k1 the only
primitive `verify` lacks is a modular square root, and `p = 3 (mod 4)` makes
that a single fixed-exponent power over the existing field operations.

The reason to treat this as a spec rather than a patch is that recovery is
soundness-delicate in a way verification is not. `ecdsa_verify` is handed a
public key and checks it. `ecrecover` *manufactures* one, and there is nothing
to check the result against, so a bug produces a wrong signer silently. In
Ethereum that is funds moving from an account that never authorised it.

## Intent

### Goal

Add `ecrecover` to `jolt_inlines_secp256k1::sdk`, recovering a `Secp256k1Point`
from a message hash, signature and recovery id, using only the existing
secp256k1 field inlines.

Proposed signature, following the crate's existing convention of returning
`Result<_, Secp256k1Error>` so callers can use `UnwrapOrSpoilProof`:

```rust
pub fn ecrecover(
    z: Secp256k1Fr,
    r: Secp256k1Fr,
    s: Secp256k1Fr,
    recovery_id: u8,
) -> Result<Secp256k1Point, Secp256k1Error>;
```

New error variants as needed, in the style of the existing `NotOnCurve`,
`ROrSZero`, `RxMismatch`: at minimum a non-residue case (no square root exists
for the candidate x) and an out-of-range recovery id.

### Invariants

1. **Agreement with a reference implementation.** For every signature a
   reference implementation recovers, `ecrecover` returns the same point; for
   every input it rejects, `ecrecover` returns an error. `k256` is the natural
   reference and is already a widely used one.
2. **A wrong recovery id never yields the signer.** For a signature produced
   under recovery id `v`, calling with any other id must not return the correct
   public key. This is the property whose violation would let a signature
   impersonate.
3. **Recovery is the inverse of signing.** For a keypair `k` and message `m`,
   recovering from `sign(k, m)` returns `pubkey(k)`.
4. **Range discipline.** `r` and `s` must be canonical and non-zero; `z` is
   reduced modulo `n` rather than rejected, matching every other ECDSA
   implementation, since a 256-bit hash may legitimately exceed `n`.
5. **`x` overflow is handled.** `recovery_id >= 2` means the x-coordinate
   wrapped, so `x = r + n`, which must be rejected when it is not below `p`.

Invariants 1 through 3 should be added to `jolt-eval` via `/new-invariant`
during implementation, in the shape of `field_mul_scalar.rs`: an `Arbitrary`
input of (secret key bytes, message hash) and a check against the reference.
That yields a `#[test]`, a fuzz target and a red-team harness from one
description, which is the right level of assurance for code in this position.

### Non-Goals

- **No new inline instruction or opcode.** This is SDK-level composition of
  existing field operations. If profiling later justifies a dedicated square
  root or recovery inline, that is a separate spec.
- **No patching of `k256` or any RustCrypto crate.** Callers that want a drop-in
  `k256` are served by a patch crate outside this repository.
- **No revm or alloy integration.** Wiring recovery into an EVM crypto backend
  is downstream of this and belongs to whoever builds that guest.
- **Not other curves.** P-256 recovery is not part of RIP-7212 and is not
  proposed here.
- **No compressed point input.** Recovery takes `(z, r, s, v)` only.

## Evaluation

### Acceptance Criteria

- [ ] `ecrecover` is public from `jolt_inlines_secp256k1` and callable from a
      guest crate
- [ ] Agrees with `k256` on at least 64 deterministically generated signatures,
      compared as full uncompressed points
- [ ] For each of those, every wrong recovery id fails to return the signer
- [ ] Rejects `r = 0`, `s = 0`, non-canonical `r` or `s`, and `recovery_id > 3`
- [ ] Rejects `recovery_id >= 2` when `r + n >= p`
- [ ] Accepts a `z` that exceeds `n`, reducing rather than failing
- [ ] A `jolt-eval` invariant covering criteria 2 and 3 exists and passes in both
      test and fuzz modes
- [ ] Executes correctly through the tracer, not only in `host` mode, verified
      the way `tests::sequence_tests::test_ecdsa_verify` does for verification
- [ ] `cargo clippy -D warnings` and `cargo fmt` clean on host and
      `riscv64imac-unknown-none-elf`

### Testing Strategy

Existing `jolt-inlines-secp256k1` tests must continue to pass unchanged; this is
additive and touches no existing function.

New tests mirror the crate's existing split. Reference-comparison and edge-case
tests run under `--features host`, where the inlines execute their reference
implementations. A guest-execution test goes alongside
`tests::sequence_tests::test_ecdsa_verify`, so the inline expansion path is
exercised rather than only the host path.

The `zk` feature is not implicated: this is guest-side SDK code and introduces
no prover or verifier changes.

Deterministic inputs rather than an RNG, so a failure is reproducible from the
repository alone.

### Performance

Two numbers, one measured and one estimated, stated separately because only the
first is evidence.

**Measured, block level.** In an independent implementation built on this
crate's public API, enabling accelerated crypto took an EEST block that
exercises the `ecrecover` precompile from 10,591,427 to 3,986,529 Jolt cycles,
and proving from 42.8s to 18.3s on a 14-core CPU. That 2.66x is the whole
accelerated bundle, keccak256 and sha256 included, not recovery alone, so it
bounds rather than isolates the benefit.

**Estimated, operation level.** The construction is roughly one square root
(~256 squarings and ~250 multiplications for the fixed exponent) plus a
GLV-decomposed double scalar multiplication (~128 doublings and up to 256
additions). Call it 4,000 to 5,000 field operations. A software recovery in a
zkVM guest is far more, so a single-digit multiple is plausible, but this is
arithmetic on paper and should not be quoted as a result.

A `jolt-eval` objective for cycles per recovery, added via `/new-objective`,
would replace both of these with something measured in-tree, and give the
advice-hinted variant below a target to beat.

No regression is possible for existing callers: nothing existing is modified.

## Design

### Architecture

Entirely within `jolt-inlines/secp256k1/src/sdk.rs`, alongside `ecdsa_verify`,
using types and operations already there: `Secp256k1Fq`, `Secp256k1Fr`,
`Secp256k1Point`, `Secp256k1PointExt::endomorphism`, `Fr::glv_decompose`, and
the internal `secp256k1_4x128_inner_scalar_mul` helper that `ecdsa_verify`
already uses.

```text
(z, r, s, v)
   |
   +-- x  = r, or r + n when v >= 2, rejected unless x < p
   +-- y  = sqrt(x^3 + 7) via one fixed-exponent power, p = 3 (mod 4)
   |        sign chosen so parity(y) matches v & 1
   +-- R  = (x, y), rejected unless on curve and not infinity
   +-- Q  = (s/r) * R - (z/r) * G
            reusing the same 4x128 GLV multiplication shape as ecdsa_verify
```

The only new machinery is the square root. Everything else is a rearrangement of
what `ecdsa_verify` already does.

### Alternatives Considered

**Leave it out, as today.** The strongest argument for the status quo is the
soundness asymmetry in the Summary: shipping the primitive where the caller
holds the ground truth, and deferring the one that invents it, is defensible.
This spec exists to make that decision explicitly rather than by omission. If
the answer is that recovery should live outside the crate, that is a legitimate
outcome and the remaining sections still describe what such an implementation
must satisfy. In that case `div_assume_nonzero` would need to be public on the
RISC-V arm, where it is currently private while being public on the other two.

**Advice-hinted `y` instead of a square root.** Supply `y` on the advice tape
and constrain it with an on-curve check and a parity check against `v`. Cheaper
than ~500 field operations, but it requires a second emulation pass to populate
the tape and introduces a new thing to argue about soundness-wise. The square
root needs neither and is not obviously expensive enough to justify that. Worth
revisiting once the objective above exists to measure it against, which is why
this spec proposes the measurement.

**A dedicated square root inline.** A new opcode would make the root cheap, but
it is a much larger ask, and `p = 3 (mod 4)` means the operation decomposes into
existing inlines without one. Better justified by profiling than assumed.

**Patch `k256` in the SP1 style.** Reaches the same crypto through
`[patch.crates-io]`, but means maintaining forks against upstream RustCrypto
releases, and puts curve work outside the crate that owns the curve primitives.

## Documentation

`book/src/how/optimizations/inlines.md` needs two changes:

- the sentence at line 95, which currently says the secp256k1 and P-256 inlines
  are "wrapped in higher-level SDKs that provide field element types and ECDSA
  verification", to mention recovery;
- the "Error Handling in Secp256k1" section, to show `ecrecover` alongside
  `ecdsa_verify` if it gains error variants, since that section documents the
  `UnwrapOrSpoilProof` convention this function should follow.

No new page. The opcode table at line 69 is unchanged, since no opcode is added.

## Execution

A working implementation exists against the crate's **public** API only, so it
compiles against an unmodified checkout, along with the differential tests
against `k256` and the edge cases listed above. It is available to donate as a
starting point.

Two changes it needs before it is what should land here:

1. It uses a hand-written GLV double-and-add because
   `secp256k1_4x128_inner_scalar_mul` and `conditional_negate` are private. The
   in-crate version should use them, as `ecdsa_verify` does, and be faster for it.
2. It returns `Option<[u8; 64]>` rather than
   `Result<Secp256k1Point, Secp256k1Error>`, which does not match the crate's
   conventions.

## References

- SEC 1 v2.0, section 4.1.6, "Public Key Recovery Operation"
- EIP-2, on the `s` range restriction, relevant to callers rather than this function
- `jolt-inlines/secp256k1/src/sdk.rs`, `ecdsa_verify`, whose scalar
  multiplication shape this reuses
- `examples/recover-ecdsa` and `examples/sig-recovery`, both performing recovery
  in software today
- `jolt-eval/src/invariant/field_mul_scalar.rs`, as the shape for the proposed
  invariant
