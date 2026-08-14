use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to read guest manifest at {path}: {err}")]
    ReadManifest {
        path: String,
        #[source]
        err: io::Error,
    },

    #[error("failed to parse guest manifest: {0}")]
    ParseManifest(#[source] toml::de::Error),

    #[error("guest manifest has no [package] name")]
    NoPackageName,

    #[error("failed to spawn `{program}` (install the Jolt CLI or set JOLT_PATH): {err}")]
    Spawn {
        program: String,
        #[source]
        err: io::Error,
    },

    #[error("jolt build failed:\n{stderr}")]
    BuildFailed { stderr: String },

    #[error("failed to read built ELF at {path}: {err}")]
    ReadElf {
        path: String,
        #[source]
        err: io::Error,
    },
}
