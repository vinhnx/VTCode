//! A task-owned private endpoint; peers can only observe task lifetime.

use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const MAX_CONNECTIONS: usize = 256;
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) struct TaskProcessOwner {
    _directory: tempfile::TempDir,
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<io::Result<()>>>,
    shutdown_error: Option<io::Error>,
}

impl TaskProcessOwner {
    pub(super) fn new() -> io::Result<Self> {
        // TempDir is exclusively created with owner-only directory permissions.
        // A short name also respects macOS's small Unix socket pathname limit.
        let directory = tempfile::Builder::new()
            .prefix(vtcode_bash_runner::process_owner::OWNER_DIRECTORY_PREFIX)
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()?;
        let socket = directory.path().join(vtcode_bash_runner::process_owner::OWNER_SOCKET_FILE);
        let listener = UnixListener::bind(&socket)?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("vtcode-task-owner".into())
            .spawn(move || run(listener, &worker_stop))?;
        Ok(Self {
            _directory: directory,
            socket,
            stop,
            worker: Some(worker),
            shutdown_error: None,
        })
    }

    pub(super) fn socket(&self) -> &Path {
        &self.socket
    }

    pub(super) fn shutdown(&mut self) -> io::Result<()> {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            if let Err(error) = worker
                .join()
                .map_err(|_panic| io::Error::other("task process owner worker panicked"))
                .and_then(|result| result)
            {
                self.shutdown_error = Some(error);
            }
        }
        match &self.shutdown_error {
            Some(error) => Err(io::Error::new(error.kind(), error.to_string())),
            None => Ok(()),
        }
    }
}

impl Drop for TaskProcessOwner {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            tracing::warn!(%error, "task process ownership cleanup failed");
        }
    }
}

fn still_connected(stream: &mut UnixStream) -> bool {
    let mut byte = [0];
    match stream.read(&mut byte) {
        Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => true,
        // Peers never send data. Disconnecting an invalid peer fails closed.
        _ => false,
    }
}

fn run(listener: UnixListener, stop: &AtomicBool) -> io::Result<()> {
    let mut connections = Vec::new();
    let serving = (|| -> io::Result<()> {
        while !stop.load(Ordering::Acquire) {
            connections.retain_mut(still_connected);
            // Bound work per poll even if an unrelated local peer floods the
            // private endpoint. The endpoint cannot execute or signal anything.
            for _ in 0..16 {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if connections.len() >= MAX_CONNECTIONS {
                            continue;
                        }
                        stream.set_nonblocking(true)?;
                        if stream.write_all(&[1]).is_ok() {
                            connections.push(stream);
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error),
                }
            }
            thread::sleep(POLL_INTERVAL);
        }
        Ok(())
    })();
    // Stop admission before releasing any lifetime connections. Helpers still
    // in startup cannot execute without their readiness acknowledgement.
    drop(listener);
    let mut cleanup_error = None;
    for stream in &connections {
        if let Err(error) = stream.shutdown(Shutdown::Write)
            && error.kind() != io::ErrorKind::NotConnected
        {
            let _ = cleanup_error.get_or_insert(error);
        }
    }
    let deadline = Instant::now() + CLEANUP_TIMEOUT;
    loop {
        connections.retain_mut(still_connected);
        if connections.is_empty() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = cleanup_error
                .get_or_insert_with(|| io::Error::new(io::ErrorKind::TimedOut, "command lifetime peers did not close"));
            break;
        }
        thread::sleep(POLL_INTERVAL);
    }
    // Returning also closes every remaining connection on errors/timeouts.
    serving?;
    match cleanup_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_shutdown_retains_timeout_failure_on_repeated_calls() {
        let mut owner = TaskProcessOwner::new().unwrap();
        let mut peer = UnixStream::connect(owner.socket()).unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        peer.read_exact(&mut [0]).unwrap();
        // This peer deliberately does not close after EOF. A later shutdown
        // must not misreport the failed cleanup as successful.
        assert_eq!(owner.shutdown().unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert_eq!(owner.shutdown().unwrap_err().kind(), io::ErrorKind::TimedOut);
        drop(peer);
    }

    #[test]
    fn owner_closes_peers_joins_worker_and_removes_private_endpoint() {
        use std::os::unix::fs::PermissionsExt;
        let mut owner = TaskProcessOwner::new().unwrap();
        let socket = owner.socket().to_owned();
        assert_eq!(std::fs::metadata(socket.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        // The supervised endpoint must satisfy the launcher's provenance gate,
        // otherwise managed commands would silently spawn unowned.
        assert!(vtcode_bash_runner::process_owner::is_owned_endpoint(&socket));
        let mut peer = UnixStream::connect(&socket).unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut ready = [0];
        peer.read_exact(&mut ready).unwrap();
        assert_eq!(ready, [1]);
        let reader = thread::spawn(move || {
            let mut byte = [0];
            assert_eq!(peer.read(&mut byte).unwrap(), 0);
        });
        owner.shutdown().unwrap();
        owner.shutdown().unwrap();
        reader.join().unwrap();
        assert!(UnixStream::connect(&socket).is_err());
        drop(owner);
        assert!(!socket.exists());
    }
}
