//! A cancellable, bounded, read-only Git subprocess. No shell or detached Git.
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
pub struct GitRunner(Arc<Inner>);

#[derive(Default)]
struct Inner {
    cancelled: AtomicBool,
    child: Mutex<Option<Child>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.get_mut().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub(super) struct Output {
    pub bytes: Vec<u8>,
    pub success: bool,
    pub stderr: String,
    pub stopped: Option<&'static str>,
}

impl GitRunner {
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
        if let Some(mut child) = self.0.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    pub fn cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    pub(super) fn run(
        &self,
        binary: &OsStr,
        root: &Path,
        args: &[OsString],
        limit: usize,
        deadline: Instant,
    ) -> Result<Output, String> {
        let (stdout, stderr) = {
            // Serialize spawn with cancellation: quit cannot race past a new child.
            let mut active = self.0.child.lock().unwrap();
            if self.cancelled() {
                return Err("cancelled".into());
            }
            if Instant::now() >= deadline {
                return Err("timeout".into());
            }
            let mut child = Command::new(binary)
                .current_dir(root)
                .args([
                    "--no-pager",
                    "-c",
                    "core.fsmonitor=false",
                    "-c",
                    "color.ui=false",
                ])
                .args(args)
                .env("GIT_OPTIONAL_LOCKS", "0")
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GIT_NO_LAZY_FETCH", "1")
                .env("GIT_ALLOW_PROTOCOL", "") // Never fetch missing objects, even on older Git.
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        "Git executable not found".into()
                    } else {
                        format!("Git spawn: {e}")
                    }
                })?;
            let streams = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
            *active = Some(child);
            streams
        };
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let reader = std::thread::spawn(move || {
            let result = capture(stdout, limit);
            let _ = tx.send(result.as_ref().is_ok_and(|r| r.1));
            result
        });
        let errors = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut chunk = [0; 8192];
            let mut input = stderr;
            while let Ok(n) = input.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let keep = n.min(8192_usize.saturating_sub(bytes.len()));
                bytes.extend_from_slice(&chunk[..keep]);
            }
            bytes
        });
        let mut stopped = None;
        let mut success = false;
        loop {
            if self.cancelled() {
                stopped = Some("cancelled");
                break;
            }
            if Instant::now() >= deadline {
                stopped = Some("timeout");
                break;
            }
            if reader.is_finished() {
                // The reader also finishes immediately when its byte cap is hit.
                let mut child = self.0.child.lock().unwrap();
                if let Some(child) = child.as_mut() {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            success = status.success();
                            break;
                        }
                        Err(_) => {
                            stopped = Some("Git wait failed");
                            break;
                        }
                        _ => {}
                    }
                } else {
                    stopped = Some("cancelled");
                    break;
                }
                // A reader finishing before EOF can mean the output cap was hit.
                if rx.try_recv() == Ok(true) {
                    stopped = Some("outputBudget");
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if let Some(mut child) = self.0.child.lock().unwrap().take() {
            if stopped.is_some() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        let result = reader
            .join()
            .map_err(|_| "Git output reader stopped".to_owned())??;
        let stderr = String::from_utf8_lossy(&errors.join().unwrap_or_default())
            .trim()
            .chars()
            .take(500)
            .collect();
        if result.1 {
            stopped = Some("outputBudget");
        }
        Ok(Output {
            bytes: result.0,
            success,
            stderr,
            stopped,
        })
    }
}

fn capture(mut input: impl Read, limit: usize) -> Result<(Vec<u8>, bool), String> {
    let cap = limit.saturating_add(1);
    let mut bytes = Vec::with_capacity(cap.min(65536));
    let mut chunk = [0; 8192];
    while bytes.len() < cap {
        let allowed = chunk.len().min(cap - bytes.len());
        let n = input
            .read(&mut chunk[..allowed])
            .map_err(|e| format!("Git output: {e}"))?;
        if n == 0 {
            return Ok((bytes, false));
        }
        if bytes.len() + n > bytes.capacity() {
            let capacity = (bytes.capacity().max(1) * 2).min(cap).max(bytes.len() + n);
            bytes.reserve_exact(capacity - bytes.len());
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    bytes.truncate(limit);
    Ok((bytes, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_output_is_explicit_and_exact_limit_is_complete() {
        assert_eq!(capture(&b"1234"[..], 4).unwrap(), (b"1234".to_vec(), false));
        assert_eq!(capture(&b"12345"[..], 4).unwrap(), (b"1234".to_vec(), true));
        assert_eq!(capture(&b"x"[..], 0).unwrap(), (vec![], true));
    }
    #[cfg(unix)]
    #[test]
    fn timeout_and_cancellation_reap_active_process() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join("mock-git");
        std::fs::write(&binary, "#!/bin/sh\nexec /bin/sleep 30\n").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let runner = GitRunner::default();
        let out = runner
            .run(
                binary.as_os_str(),
                temp.path(),
                &[],
                1024,
                Instant::now() + Duration::from_millis(30),
            )
            .unwrap();
        assert_eq!(out.stopped, Some("timeout"));
        assert!(runner.0.child.lock().unwrap().is_none());
        let background = runner.clone();
        let root = temp.path().to_path_buf();
        let task = std::thread::spawn(move || {
            background
                .run(
                    binary.as_os_str(),
                    &root,
                    &[],
                    1024,
                    Instant::now() + Duration::from_secs(30),
                )
                .unwrap()
        });
        while runner.0.child.lock().unwrap().is_none() {
            std::thread::yield_now();
        }
        runner.cancel();
        assert_eq!(task.join().unwrap().stopped, Some("cancelled"));
        assert!(runner.0.child.lock().unwrap().is_none());
    }
}
