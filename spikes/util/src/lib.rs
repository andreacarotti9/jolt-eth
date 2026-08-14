//! Shared plumbing for the Phase 0 spikes.
//!
//! Jolt reports guest cycle-tracking regions only as `tracing` INFO lines from
//! `tracer::emulator::cpu` - there is no programmatic accessor at 915faf4. So we
//! install a subscriber that writes into a buffer we own and parse the lines back
//! out. Ugly, but it is the only channel that exists.

use std::{
    fs,
    io::{self, Write},
    path::Path,
    sync::{Arc, Mutex},
};

use serde::Serialize;
use tracing_subscriber::fmt::MakeWriter;

/// A `tracing` writer that accumulates into a shared buffer.
#[derive(Clone, Default)]
pub struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Capture {
        self.clone()
    }
}

/// One `start_cycle_tracking`/`end_cycle_tracking` region.
#[derive(Clone, Debug, Serialize)]
pub struct Marker {
    pub label: String,
    /// Real RV64IMAC instructions retired.
    pub real: u64,
    /// Virtual instructions the tracer expanded (inlines land here).
    pub virt: u64,
    /// Total trace rows, i.e. what `max_trace_length` is measured against.
    pub total: u64,
}

/// Installs the capturing subscriber. Call once per process.
pub fn init() -> Capture {
    let cap = Capture::default();
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(cap.clone())
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();
    cap
}

/// Parses and removes every cycle-tracking region seen so far.
pub fn drain(cap: &Capture) -> Vec<Marker> {
    let mut buf = cap.0.lock().unwrap();
    let text = String::from_utf8_lossy(&buf).into_owned();
    buf.clear();
    text.lines().filter_map(parse_marker).collect()
}

/// `"label": 504 RV64IMAC cycles + 0 virtual instructions = 504 total cycles`
fn parse_marker(line: &str) -> Option<Marker> {
    let (label, rest) = line.split_once("\": ")?;
    let label = label.rsplit_once('"')?.1.to_string();
    let num_before = |hay: &str, needle: &str| -> Option<u64> {
        hay.split_once(needle)?.0.rsplit(' ').next()?.parse().ok()
    };
    Some(Marker {
        label,
        real: num_before(rest, " RV64IMAC cycles")?,
        virt: num_before(rest, " virtual instructions")?,
        total: num_before(rest, " total cycles")?,
    })
}

/// Absolute path of `bench/results/<name>`.
///
/// Resolved from this crate's manifest rather than the working directory: the
/// drivers are run both from the repo root and from their own directories, and a
/// relative path silently wrote results outside the repo when the workspace root
/// moved.
pub fn result_path(name: impl AsRef<str>) -> std::path::PathBuf {
    // <root>/spikes/util -> <root>
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("spike-util lives at <root>/spikes/util");
    root.join("bench").join("results").join(name.as_ref())
}

/// Writes a spike result as pretty JSON and echoes the path.
pub fn write_json<T: Serialize>(path: impl AsRef<Path>, value: &T) {
    let path = path.as_ref();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).expect("create result dir");
    }
    fs::write(path, serde_json::to_string_pretty(value).unwrap()).expect("write result");
    eprintln!("wrote {}", path.display());
}

/// Environment fingerprint stamped into every result file.
#[derive(Debug, Serialize)]
pub struct Env {
    pub jolt_commit: &'static str,
    pub host: String,
    pub cpus: usize,
}

impl Env {
    pub fn capture() -> Self {
        Self {
            jolt_commit: "915faf453f36871249615a7fdf2704d77a88f259",
            host: std::env::var("SPIKE_HOST").unwrap_or_else(|_| "unknown".into()),
            cpus: std::thread::available_parallelism().map_or(0, |n| n.get()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_marker;

    #[test]
    fn parses_a_marker_line() {
        let line = "2026-08-14T15:15:43Z  INFO trace: tracer::emulator::cpu: \
                    \"fib_loop\": 504 RV64IMAC cycles + 12 virtual instructions = 516 total cycles";
        let m = parse_marker(line).expect("should parse");
        assert_eq!(m.label, "fib_loop");
        assert_eq!((m.real, m.virt, m.total), (504, 12, 516));
    }

    #[test]
    fn ignores_unrelated_lines() {
        assert!(parse_marker("INFO tracer: trace length: 1166 cycles").is_none());
    }
}
