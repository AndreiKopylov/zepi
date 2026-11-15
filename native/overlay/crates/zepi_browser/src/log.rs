use std::io::Write;

// Append-only diagnostics; the ACP host discards our stderr, so log to a file.
pub fn log(msg: &str) {
    let dir = std::env::var("LOCALAPPDATA")
        .map(|l| std::path::PathBuf::from(l).join("Zepi").join("logs"))
        .unwrap_or_else(|_| std::env::temp_dir());
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("zepi_browser.log")) {
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}
