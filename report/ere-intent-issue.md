# Draft issue for eth-act/ere

Post this before opening the PR, per the plan: it is both diligence and the
cheapest possible way to find out early if the maintainers want the backend
shaped differently.

---

**Title:** Add a Jolt backend

**Body:**

Ere supports OpenVM, SP1 and ZisK today, and the zkEVM roadmap calls for
integrating more zkVMs into Ere and Ethproofs. I have a working Jolt backend and
would like to upstream it, but I would rather agree the shape with you before
opening a PR than after.

**What works already**, against `ere` v0.15.0, `ere-guests` v0.15.0 and Jolt
`915faf453f36871249615a7fdf2704d77a88f259`:

- the four backend crates - `ere-platform-jolt`, `ere-compiler-jolt`,
  `ere-verifier-jolt`, `ere-prover-jolt` - mirroring the SP1 backend crate for
  crate;
- `zkVMKind::Jolt` and its SDK-version detection;
- a `docker/jolt/Dockerfile.base` following `docker/sp1/`;
- `stateless-validator-reth`, unmodified and `no_std`, compiling and proving real
  EEST blocks in a Jolt guest, with the guest's output matching the fixture's
  `statelessOutputBytes`;
- an implementation of the zkvm-standards accelerator C ABI on Jolt's inline
  instructions (keccak256, sha256, secp256k1 verify and ecrecover, secp256r1),
  with everything lacking a Jolt substrate forwarded to revm's own software
  backend so enabling the seam can only remove cycles;
- an end-to-end test that compiles, executes, proves and verifies an EEST block
  through `zkVMProver`.

**Three places where Jolt does not fit the existing mould**, which is really what
I would like your opinion on:

1. **No ambient guest IO.** OpenVM, SP1 and ZisK export the zkvm-standards
   `read_input` / `write_output` symbols, so they inherit `Platform`'s default
   implementations. Jolt has no such symbols: `#[jolt::provable]` computes the
   memory layout and bakes the input and output addresses into the entrypoint it
   generates. My backend therefore defines a guest contract -

   ```rust
   #[jolt::provable(/* sizes */)]
   fn main(input: jolt::UntrustedAdvice<&[u8]>) -> Vec<u8> { .. }
   ```

   - with `Input::stdin` postcard-encoded into the untrusted advice region and
   the returned `Vec<u8>` read back as `PublicValues`. `JoltPlatform::read_input`
   and `write_output` are wired to a diagnostic panic rather than a wrong answer.
   Is a documented per-backend guest contract acceptable, or would you rather see
   the C ABI symbols implemented on top of Jolt's memory layout so the defaults
   work?

2. **The compiler shells out.** Jolt's guest ELF is only valid if the `jolt` CLI
   produced it - the CLI owns the linker script, the memory layout and the ZeroOS
   runtime wiring. `ere-compiler-jolt` therefore runs `jolt build` rather than
   assembling a rustflags recipe like the SP1 compiler does. The alternative is a
   second copy of a linker script that drifts silently, which seemed worse.

3. **The verifier needs the ELF, though the key is still 32 bytes.**
   `JoltProgramVk` is a SHA-256 digest of the guest's verifier preprocessing, so
   it matches SP1's and ZisK's 32 bytes and OpenVM's small commitment struct.
   `JoltVerifier` holds the preprocessing itself and is constructed by
   re-deriving it from the guest ELF and checking it against the key.

   The difference from SP1 worth flagging: SP1 verifies a *compressed, recursive*
   proof whose recursion circuit checks the real verifying key internally, so an
   SP1 verifier needs only the 32 bytes. Jolt at this pin verifies the raw proof
   directly, so a Jolt verifier additionally needs the ELF (or any bit-identical
   source of the preprocessing). That preprocessing is genuinely program-specific
   and large - I measured 98 MB for the stateless validator against 244 KB for a
   toy guest at the same trace bound - so it is not a universal parameter that
   could be factored out into the verifier setup.

   Is "the key is a digest, and constructing a verifier costs an ELF plus a
   preprocessing pass" acceptable? If Ere assumes a verifier can be built from
   the key alone, then a Jolt backend really wants proof compression first, and I
   would rather know that now.

**Two bugs found on the way**, unrelated to the backend but worth reporting:

- `ere-prover-sp1` cannot execute on aarch64 at v0.15.0. `SP1ExecutorPool` calls
  `reset()` on a pooled `MinimalExecutorEnum`, and SP1 v6.3.1 leaves `reset()` as
  `todo!()` in the portable executor, which is the one every non-x86_64 host
  gets. Happy to open this separately.
- `ere-compiler-sp1`'s stock-nightly path needs `-Zjson-target-spec`, which only
  very recent nightlies have, and those same nightlies fail to `-Zbuild-std` the
  `stateless-validator-reth` dependency tree (`serde_core` does not compile). The
  `SP1RustRv64imaCustomized` path works with `--ignore-rust-version`, since the
  released Succinct toolchain reports 1.93 against ere-guests' reth pin of 1.95.

**One caveat I want on the record before any numbers circulate.** Jolt's inlines
are virtual instruction sequences expanded into the main trace, not precompiles
with separate AIRs. Measured on a Jolt guest, keccak256 costs 24.5 cycles/byte
inline against 60.5 in software - 2.5x, not orders of magnitude. Anyone comparing
a Jolt "cycle" to an SP1 "cycle" without that context will draw the wrong
conclusion, and I would rather the backend land alongside that caveat than
without it.

Repository with all of the above, the measurements and the raw data:
https://github.com/andreacarotti9/jolt-eth
