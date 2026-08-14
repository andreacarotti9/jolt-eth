# docker/jolt

Mirrors `ere/docker/sp1/` for the Jolt backend, to be moved there with the rest
of Track A. Build from an `ere` checkout so `ere-base` resolves:

```bash
docker build -t ere-base -f docker/Dockerfile.base .
docker build -t ere-jolt -f /path/to/jolt-eth/docker/jolt/Dockerfile.base .
```

Two differences from the other backends are structural, not incidental:

- Jolt's guest compiler is the `jolt` binary, not a set of rustflags, so the
  image installs the CLI rather than a target spec. `ere-compiler-jolt` shells
  out to it.
- There is no CUDA variant. `JoltProver::new` rejects any prover resource other
  than CPU rather than silently downgrading, so a GPU-labelled run fails loudly.
