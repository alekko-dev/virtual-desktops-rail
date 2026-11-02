use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const MAX_WORKER_BYTES: u64 = 8 * 1024 * 1024;

pub struct OwnedChild(pub Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        // Only our subprocess is terminated, including on error/panic paths.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn run_bounded(mut command: Command, timeout: Duration) -> Result<Vec<u8>, String> {
    let mut child = OwnedChild(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Cannot start isolated probe: {error}"))?,
    );
    let output = child.0.stdout.take().ok_or("Missing worker output pipe")?;
    // Drain concurrently so a full pipe cannot make a responsive worker time out.
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        output.take(MAX_WORKER_BYTES + 1).read_to_end(&mut bytes)?;
        Ok::<_, std::io::Error>(bytes)
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.0.try_wait() {
            Ok(Some(status)) => {
                let bytes = reader
                    .join()
                    .map_err(|_| "Probe output reader failed")?
                    .map_err(|error| error.to_string())?;
                if bytes.len() as u64 > MAX_WORKER_BYTES {
                    return Err("Probe output exceeded the diagnostic size limit".into());
                }
                if !status.success() {
                    return Err(format!("Isolated probe failed or crashed ({status})"));
                }
                return Ok(bytes);
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                drop(child);
                let _ = reader.join();
                return Err("Shell integration timed out; isolated probe terminated".into());
            }
            Err(error) => return Err(format!("Cannot observe probe process: {error}")),
        }
    }
}
