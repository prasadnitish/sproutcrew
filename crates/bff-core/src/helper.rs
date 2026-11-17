//! Small programs BFF starts to read formats Rust does not: macOS's own `textutil` for old Word files and
//! the text reader built from `ocr/main.swift`. A helper gets an empty environment, no input, a time limit
//! and a cap on what it may print. The file it reads is copied into a private folder first and removed
//! afterwards, so it never sits where another user could see it.

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// What a helper printed, and how it ended.
pub struct Output {
    pub stdout: Vec<u8>,
    pub timed_out: bool,
    pub success: bool,
    /// The helper printed more than the cap, so `stdout` is cut.
    pub too_long: bool,
}

/// A folder only this user can use, made if it is missing. Refused when it is a link or belongs to someone else.
pub fn private_dir() -> Result<PathBuf, String> {
    let uid = unsafe { libc::geteuid() };
    let dir = std::env::temp_dir().join(format!("bff-tools-{uid}"));
    match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    }
    let meta = std::fs::symlink_metadata(&dir).map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err(format!(
            "{} is not a private folder, so BFF will not use it",
            dir.display()
        ));
    }
    Ok(dir)
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A file in the private folder, removed when this is dropped.
pub struct TempFile(PathBuf);

impl TempFile {
    pub fn create(ext: &str, bytes: &[u8]) -> Result<TempFile, String> {
        let ext: String = ext
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(8)
            .collect();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let name = format!(
            "in-{}-{nanos}-{}.{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
            if ext.is_empty() { "bin" } else { &ext }
        );
        let path = private_dir()?.join(name);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| e.to_string())?;
        // From here the file exists, so dropping `temp` removes it even when the write fails.
        let temp = TempFile(path);
        file.write_all(bytes).map_err(|e| e.to_string())?;
        Ok(temp)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Runs `program` with an empty environment. After `limit` it is killed, and what it printed so far is
/// returned with `timed_out` set. Output past `max_out` bytes is dropped.
pub fn run<S: AsRef<OsStr>>(
    program: &Path,
    args: &[S],
    limit: Duration,
    max_out: usize,
) -> Result<Output, String> {
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    let mut pipe = child.stdout.take().expect("piped");
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut pipe).take(max_out as u64 + 1).read_to_end(&mut buf);
        // Keep draining, so a helper that prints more than the cap is not stuck on a full pipe.
        let _ = std::io::copy(&mut pipe, &mut std::io::sink());
        buf
    });
    let end = Instant::now() + limit;
    let (mut timed_out, mut success) = (false, false);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                success = status.success();
                break;
            }
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.to_string());
            }
        }
        if Instant::now() >= end {
            timed_out = true;
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    let mut stdout = reader.join().unwrap_or_default();
    let too_long = stdout.len() > max_out;
    stdout.truncate(max_out);
    Ok(Output {
        stdout,
        timed_out,
        success,
        too_long,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_helper_runs_with_an_empty_environment_and_its_output_is_returned() {
        std::env::set_var("BFF_HELPER_TEST", "visible");
        let out = run(
            Path::new("/usr/bin/env"),
            &[] as &[&str],
            Duration::from_secs(10),
            10_000,
        )
        .unwrap();
        assert!(out.success && !out.timed_out);
        assert!(
            !String::from_utf8_lossy(&out.stdout).contains("BFF_HELPER_TEST"),
            "the child saw the parent's environment"
        );
    }

    #[test]
    fn a_helper_that_runs_too_long_is_stopped_and_what_it_printed_is_kept() {
        let out = run(
            Path::new("/bin/sh"),
            &["-c", "echo first; sleep 30"],
            Duration::from_millis(600),
            10_000,
        )
        .unwrap();
        assert!(out.timed_out && !out.success);
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "first");
    }

    #[test]
    fn output_past_the_cap_is_cut_and_said_so() {
        let out = run(
            Path::new("/bin/sh"),
            &["-c", "yes aaaaaaaaaa | head -c 200000"],
            Duration::from_secs(10),
            1000,
        )
        .unwrap();
        assert!(out.too_long);
        assert_eq!(out.stdout.len(), 1000);
    }

    #[test]
    fn a_temporary_file_is_private_and_removed_when_dropped() {
        let path = {
            let f = TempFile::create("pdf", b"hello").unwrap();
            let meta = std::fs::metadata(f.path()).unwrap();
            assert_eq!(meta.mode() & 0o777, 0o600);
            assert_eq!(std::fs::read(f.path()).unwrap(), b"hello");
            assert!(f.path().extension().unwrap() == "pdf");
            f.path().to_path_buf()
        };
        assert!(!path.exists());
        // A hostile extension cannot reach outside the folder.
        let f = TempFile::create("../../x", b"").unwrap();
        assert_eq!(f.path().parent().unwrap(), private_dir().unwrap());
    }
}
