use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
pub fn replace(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(source: *const u16, target: *const u16, flags: u32) -> i32;
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let temporary = parent.join(format!(".rail-probe-{}-{nonce}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let replace = (|| -> Result<(), Box<dyn std::error::Error>> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        let wide = |path: &Path| -> Result<Vec<u16>, std::io::Error> {
            let mut value: Vec<_> = path.as_os_str().encode_wide().collect();
            if value.contains(&0) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "NUL in output path",
                ));
            }
            value.push(0);
            Ok(value)
        };
        let source = wide(&temporary)?;
        let target = wide(path)?;
        // Same-directory replacement preserves the previous report on write failure.
        if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 1 | 8) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    })();
    if replace.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    replace
}
