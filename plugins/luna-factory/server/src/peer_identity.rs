//! Linux-local attestation for app-server builds without serverInfo. No protocol
//! response fields participate. An invisible host PID is deliberately unqualified.
#[cfg(target_os = "linux")]
mod linux {
    use crate::qualification::NativeServerIdentity;
    use std::{
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd},
            unix::fs::MetadataExt,
        },
        path::{Path, PathBuf},
    };

    pub(crate) struct PeerIdentity {
        pidfd: OwnedFd,
        pid: i32,
        executable: PathBuf,
        configured_binary: PathBuf,
        identity: NativeServerIdentity,
    }
    impl PeerIdentity {
        pub(crate) fn capture(stream: &tokio::net::UnixStream, binary: &Path) -> Option<Self> {
            let credentials = stream.peer_cred().ok()?;
            let pid = credentials.pid().filter(|pid| *pid > 0)?;
            // SAFETY: geteuid has no arguments or memory access requirements.
            if credentials.uid() != unsafe { libc::geteuid() } {
                return None;
            }
            // SO_PEERPIDFD pins the actual connected peer, closing the PID-reuse
            // race of peer_cred followed by pidfd_open. Linux >= 6.5 is required.
            let mut fd: libc::c_int = -1;
            let mut length = std::mem::size_of_val(&fd) as libc::socklen_t;
            // SAFETY: fd and length point to initialized, correctly sized objects.
            let result = unsafe {
                libc::getsockopt(
                    stream.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_PEERPIDFD,
                    (&mut fd as *mut libc::c_int).cast(),
                    &mut length,
                )
            };
            if result != 0 || fd < 0 {
                return None;
            }
            // SAFETY: successful SO_PEERPIDFD transfers one owned fd to the caller.
            let pidfd = unsafe { OwnedFd::from_raw_fd(fd) };
            if length as usize != std::mem::size_of_val(&fd) {
                return None;
            }
            let executable = PathBuf::from(format!("/proc/{pid}/exe"));
            let configured_binary = binary.to_path_buf();
            let binary_hash = crate::qualification::binary_identity(binary);
            if binary_hash == "unavailable" {
                return None;
            }
            let identity = NativeServerIdentity {
                name: "linux-unix-peer-executable".into(),
                version: "1".into(),
                build_hash: None,
                binary_hash: Some(binary_hash),
            };
            let peer = Self {
                pidfd,
                pid,
                executable,
                configured_binary,
                identity,
            };
            peer.current()?;
            Some(peer)
        }
        fn alive(&self) -> Option<()> {
            let mut descriptor = libc::pollfd {
                fd: self.pidfd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one valid pollfd is borrowed for this nonblocking poll.
            (unsafe { libc::poll(&mut descriptor, 1, 0) } == 0).then_some(())
        }
        pub(crate) fn current(&self) -> Option<NativeServerIdentity> {
            self.alive()?;
            let info =
                std::fs::read_to_string(format!("/proc/self/fdinfo/{}", self.pidfd.as_raw_fd()))
                    .ok()?;
            if info.len() > 4096
                || !info.lines().any(|line| {
                    line.strip_prefix("Pid:")
                        .and_then(|v| v.trim().parse::<i32>().ok())
                        == Some(self.pid)
                })
            {
                return None;
            }
            // The credential PID and /proc must describe this same PID namespace.
            // Container PID invisibility, host procfs substitutes and hidepid fail closed.
            let local = std::fs::metadata("/proc/self/ns/pid").ok()?;
            let remote = std::fs::metadata(format!("/proc/{}/ns/pid", self.pid)).ok()?;
            if (local.dev(), local.ino()) != (remote.dev(), remote.ino()) {
                return None;
            }
            // Reject an obviously different image before hashing; this also keeps
            // passive fixture connections from hashing a large unrelated executable.
            if std::fs::metadata(&self.executable).ok()?.len()
                != std::fs::metadata(&self.configured_binary).ok()?.len()
            {
                return None;
            }
            let expected = self.identity.binary_hash.as_ref()?;
            if &crate::qualification::binary_identity(&self.executable) != expected
                || &crate::qualification::binary_identity(&self.configured_binary) != expected
            {
                return None;
            }
            self.alive()?;
            Some(self.identity.clone())
        }
    }
}
#[cfg(target_os = "linux")]
pub(crate) use linux::PeerIdentity;
#[cfg(not(target_os = "linux"))]
pub(crate) struct PeerIdentity;
#[cfg(not(target_os = "linux"))]
impl PeerIdentity {
    #[cfg(unix)]
    pub(crate) fn capture(_: &tokio::net::UnixStream, _: &std::path::Path) -> Option<Self> {
        None
    }
    pub(crate) fn current(&self) -> Option<crate::qualification::NativeServerIdentity> {
        None
    }
}
