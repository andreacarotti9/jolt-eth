# patches

Changes to upstream repositories that this work needs, kept as patch files so
`upstream/` stays a clean checkout of the pins in `../UPSTREAM.md` and so each
one can be lifted into a PR unchanged.

| patch | target | what it does |
|---|---|---|
| `ere-add-jolt-backend.patch` | eth-act/ere @ `5525959` | registers `zkVMKind::Jolt`, its SDK-version detection and the four backend crates in the workspace. The crate bodies live in `../crates/ere-{compiler,platform,prover,verifier}-jolt` and move to `crates/{compiler,platform,prover,verifier}/jolt` on the way upstream. |

Apply with:

```bash
git -C upstream/ere apply ../../patches/ere-add-jolt-backend.patch
```

## Not a patch, by choice

Adding `ecrecover` to `jolt-inlines/secp256k1` would have been the obvious fifth
patch. It is instead a standalone module in `crates/jolt-zkvm-accel/src/secp256k1.rs`
built only on that crate's *public* API, so it works against an unmodified Jolt
checkout and can be upstreamed as an addition rather than a fork. The only thing
it wants from upstream is that `div_assume_nonzero` become public on the RISC-V
path, which is a one-word diff and not worth carrying separately.
