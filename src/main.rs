use std::env;
use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::time::Instant;

unsafe extern "C" {
    fn close(fd: i32) -> i32;
}

fn spawn_worker_if_needed(args: &[String]) -> io::Result<bool> {
    if args.iter().any(|a| a == "--worker") {
        return Ok(false);
    }
    let exe = env::current_exe()?;
    let mut child = Command::new(exe)
        .args(args.iter().skip(1))
        .arg("--worker")
        .stdout(Stdio::piped())
        .spawn()?;
    if let Some(mut out) = child.stdout.take() {
        io::copy(&mut out, &mut io::stdout())?;
    }
    // Don't wait; worker closes stdout early to unblock us.
    Ok(true)
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if spawn_worker_if_needed(&args)? {
        return Ok(());
    }

    let no_output = args.iter().any(|a| a == "--no-output");
    let path = args
        .iter()
        .skip(1)
        .find(|a| a.as_str() != "--worker" && a.as_str() != "--no-output")
        .cloned()
        .unwrap_or_else(|| "measurements.txt".to_string());

    let start = Instant::now();
    let result = one_b_rustc::run_worker(&path, no_output)?;

    if let Some(output) = result.output {
        let mut stdout = io::BufWriter::new(io::stdout());
        stdout.write_all(&output)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
        drop(stdout);
    }

    // Close stdout early so the parent (if any) can finish copying.
    unsafe {
        close(1);
    }

    eprintln!(
        "elapsed: {:.3?} (stations={}, checksum={})",
        start.elapsed(),
        result.stations,
        result.checksum
    );
    Ok(())
}
