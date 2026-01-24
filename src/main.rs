use std::env;
use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::time::Instant;

unsafe extern "C" {
    fn close(fd: i32) -> i32;
}

fn spawn_worker_if_needed(args: &[String]) -> io::Result<bool> {
    // This program runs in two modes:
    // - driver mode (default): spawns a worker and streams its stdout
    // - worker mode (--worker): performs the real work and exits
    //
    // The split avoids stdout buffering issues and allows the driver to
    // promptly stream results while the worker can close stdout early
    // to signal completion.
    if args.iter().any(|a| a == "--worker") {
        return Ok(false);
    }
    // Re-exec the current binary with the same arguments plus a worker marker.
    let exe = env::current_exe()?;
    let mut child = Command::new(exe)
        .args(args.iter().skip(1))
        .arg("--worker")
        .stdout(Stdio::piped())
        .spawn()?;
    // Stream the worker's stdout to our own stdout, which keeps the UX
    // identical to a single-process run while still allowing the worker
    // to close its pipe independently.
    if let Some(mut out) = child.stdout.take() {
        io::copy(&mut out, &mut io::stdout())?;
    }
    // Don't wait; worker closes stdout early to unblock us, and the
    // remaining work (stderr/timing) is handled by the worker itself.
    Ok(true)
}

fn main() -> io::Result<()> {
    // Collect CLI arguments once to allow multiple passes without
    // re-reading the environment.
    let args: Vec<String> = env::args().collect();
    if spawn_worker_if_needed(&args)? {
        return Ok(());
    }

    // Optional flag to suppress output generation, useful for profiling
    // or benchmarking when output I/O would dominate runtime.
    let no_output = args.iter().any(|a| a == "--no-output");
    // Find the first positional argument that is not an internal flag.
    // If none is provided, fall back to the canonical input file name.
    let path = args
        .iter()
        .skip(1)
        .find(|a| a.as_str() != "--worker" && a.as_str() != "--no-output")
        .cloned()
        .unwrap_or_else(|| "measurements.txt".to_string());

    // Wall-clock timing measures end-to-end worker processing time.
    let start = Instant::now();
    let result = one_b_rustc::run_worker(&path, no_output)?;

    if let Some(output) = result.output {
        // Buffered writes reduce syscalls and improve throughput when
        // printing large outputs (common in data-processing tasks).
        let mut stdout = io::BufWriter::new(io::stdout());
        stdout.write_all(&output)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
        drop(stdout);
    }

    // Close stdout early so the parent (if any) can finish copying.
    // This explicitly closes fd=1 at the OS level to signal EOF to the
    // driver, which may still be copying bytes from our stdout pipe.
    unsafe {
        close(1);
    }

    // Send timing and aggregate stats to stderr so they do not mingle
    // with data output on stdout (a common CLI design convention).
    eprintln!(
        "elapsed: {:.3?} (stations={}, checksum={})",
        start.elapsed(),
        result.stations,
        result.checksum
    );
    Ok(())
}
