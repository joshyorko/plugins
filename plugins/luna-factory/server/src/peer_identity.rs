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
            // Pin process lifetime before examining procfs. A readable pidfd means the
            // process exited; never resolve a subsequently reused numerical PID.
            // SAFETY: pidfd_open receives a kernel peer PID and flags zero; returned
            // ownership is transferred exactly once into OwnedFd below.
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
            if fd < 0 {
                return None;
            }
            let pidfd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
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
            // The credential PID and /proc must describe this same PID namespace.
            // Container PID invisibility, host procfs substitutes and hidepid fail closed.
            let local = std::fs::metadata("/proc/self/ns/pid").ok()?;
            let remote = std::fs::metadata(format!("/proc/{}/ns/pid", self.pid)).ok()?;
            if (local.dev(), local.ino()) != (remote.dev(), remote.ino()) {
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
