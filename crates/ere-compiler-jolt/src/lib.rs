//! Ere [`Compiler`] implementation for the Jolt zkVM.
//!
//! Unlike the other three backends, Jolt does not compile guests with a stock
//! `cargo build` plus rustflags: the `jolt` CLI owns the linker script, the
//! memory layout and the ZeroOS runtime wiring, and the ELF is only valid if it
//! was produced that way. So this compiler shells out to `jolt build` rather
//! than reimplementing it - the alternative is a second copy of a linker script
//! that drifts silently.
//!
//! Requires the `jolt` binary on `PATH` (`cargo install --path .` in the Jolt
//! checkout, or `JOLT_PATH` pointing at it, which is the same environment
//! variable Jolt's own host code honours).

mod error;

use std::{path::Path, process::Command};

pub use ere_compiler_core::*;

pub use crate::error::Error;

/// Jolt's bare-metal guest target.
const TARGET: &str = "riscv64imac-unknown-none-elf";

/// Compiler for a Rust guest program to Jolt's RV64IMAC target.
///
/// Memory sizes come from `ERE_JOLT_STACK_SIZE` / `ERE_JOLT_HEAP_SIZE` when set.
/// They must agree with the guest's own `#[jolt::provable]` attributes: the CLI
/// values shape the linker script, the attributes shape the prover's memory
/// layout, and a disagreement shows up as a guest that traps rather than as a
/// build error.
#[derive(Clone, Copy, Debug, Default)]
pub struct JoltRustRv64imac;

impl Compiler for JoltRustRv64imac {
    type Error = Error;

    fn compile(
        &self,
        guest_directory: impl AsRef<Path>,
        args: &[String],
    ) -> Result<Elf, Self::Error> {
        let guest_directory = guest_directory.as_ref();
        let package = package_name(guest_directory)?;
        let target_dir = guest_directory.join("target").join("ere-jolt");

        let stack_size = env_size("ERE_JOLT_STACK_SIZE", 4096);
        let heap_size = env_size("ERE_JOLT_HEAP_SIZE", 32 * 1024 * 1024);

        // `guest` is always on: it is what switches the guest crate to `no_std`
        // and makes `#[jolt::provable]` emit the entrypoint.
        let mut features = vec!["guest".to_string()];
        features.extend(args.iter().cloned());

        let jolt = std::env::var("JOLT_PATH").unwrap_or_else(|_| "jolt".into());
        let mut cmd = Command::new(&jolt);
        cmd.current_dir(guest_directory)
            .args(["build", "-p", &package])
            .args(["--backtrace", "off"])
            .args(["--stack-size", &stack_size.to_string()])
            .args(["--heap-size", &heap_size.to_string()])
            .arg("--")
            .arg("--release")
            .args(["--target-dir", &target_dir.to_string_lossy()])
            .args(["--features", &features.join(",")]);

        let output = cmd.output().map_err(|err| Error::Spawn {
            program: jolt.clone(),
            err,
        })?;
        if !output.status.success() {
            return Err(Error::BuildFailed {
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }

        let elf_path = target_dir.join(TARGET).join("release").join(&package);
        let elf = std::fs::read(&elf_path).map_err(|err| Error::ReadElf {
            path: elf_path.display().to_string(),
            err,
        })?;
        Ok(Elf(elf))
    }
}

/// `jolt build` addresses guests by package name, so read it out of the
/// manifest rather than guessing it from the directory name.
fn package_name(guest_directory: &Path) -> Result<String, Error> {
    let manifest_path = guest_directory.join("Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path).map_err(|err| Error::ReadManifest {
        path: manifest_path.display().to_string(),
        err,
    })?;
    let manifest: toml::Value = manifest.parse().map_err(Error::ParseManifest)?;
    manifest
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(|name| name.as_str())
        .map(str::to_string)
        .ok_or(Error::NoPackageName)
}

fn env_size(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}
