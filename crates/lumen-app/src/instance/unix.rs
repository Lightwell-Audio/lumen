//! The instance's endpoint on macOS and Linux: a socket in a folder only
//! the current user can enter, the peer's user checked, one connection at
//! a time with bounded reads and writes. Only the lock holder removes a
//! stale socket, so a live one is never taken away.

use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::protocol::{self, Reply, Request};
use super::{Deliver, IO_TIMEOUT, Place};

/// The longest socket path used. A socket address holds 104 bytes on macOS
/// and 108 on Linux, the closing zero included; this stays under both.
const MAX_SOCKET_PATH: usize = 100;

/// The socket's folder: a private `instance` folder inside the first of the
/// runtime folder (Linux), this user's temporary folder (a private name
/// per user) and the settings folder where the whole socket path fits an
/// address. A long settings path (a macOS test or portable folder) must not
/// leave the lock held with no socket to reach. The folders are tried in a
/// fixed order, so the holder and a later start agree.
fn folder(place: &Place) -> PathBuf {
    let name = format!(
        "{}-instance",
        lumen_core::product::PRODUCT_NAME.to_lowercase()
    );
    let file = format!("{}.sock", place.key);
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .map(|p| p.join(&name));
    let temporary = std::env::temp_dir().join(format!("{name}-{uid}"));
    let settings = place.folder.join(&name);
    let candidates: Vec<PathBuf> = runtime.into_iter().chain([temporary, settings]).collect();
    let fits = |dir: &&PathBuf| dir.join(&file).as_os_str().len() <= MAX_SOCKET_PATH;
    // None fits: the settings folder, whose bind then fails and is reported.
    let last = candidates.len() - 1;
    candidates
        .iter()
        .find(fits)
        .unwrap_or(&candidates[last])
        .clone()
}

pub(super) fn socket(place: &Place) -> PathBuf {
    folder(place).join(format!("{}.sock", place.key))
}

/// Binds the socket (replacing a stale one) and starts the listener.
pub(super) fn serve(place: &Place, deliver: Deliver) -> io::Result<()> {
    let dir = folder(place);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    // A folder made by someone else, or opened up, is not used.
    let meta = std::fs::metadata(&dir)?;
    // SAFETY: getuid has no preconditions.
    if meta.uid() != unsafe { libc::getuid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the instance folder belongs to another user",
        ));
    }
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    let path = socket(place);
    // This process holds the lock, so a socket left here is stale.
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    std::thread::Builder::new()
        .name("lumen-instance".into())
        .spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { continue };
                if !same_user(&conn) {
                    continue;
                }
                let _ = conn.set_read_timeout(Some(IO_TIMEOUT));
                let _ = conn.set_write_timeout(Some(IO_TIMEOUT));
                let deadline = Instant::now() + IO_TIMEOUT * 2;
                if let Err(e) = protocol::serve(&mut conn, &*deliver, deadline) {
                    crate::trace::log(|| format!("instance: a request failed ({e})"));
                }
            }
        })?;
    Ok(())
}

/// Sends `request` and returns the reply, within `left`.
pub(super) fn send(place: &Place, request: &Request, left: Duration) -> io::Result<Reply> {
    let deadline = Instant::now() + left;
    let mut conn = UnixStream::connect(socket(place))?;
    let timeout = left.max(Duration::from_millis(100));
    conn.set_read_timeout(Some(timeout))?;
    conn.set_write_timeout(Some(timeout))?;
    protocol::ask(&mut conn, request, deadline)
}

/// Whether the peer runs as this user.
fn same_user(conn: &UnixStream) -> bool {
    // SAFETY: getuid has no preconditions.
    peer_uid(conn) == Some(unsafe { libc::getuid() })
}

#[cfg(target_os = "linux")]
fn peer_uid(conn: &UnixStream) -> Option<libc::uid_t> {
    use std::os::fd::AsRawFd;
    // SAFETY: an all-zero ucred is a valid out value.
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the socket is open; `cred` and `len` are valid out values.
    let ok = unsafe {
        libc::getsockopt(
            conn.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    } == 0;
    ok.then_some(cred.uid)
}

#[cfg(not(target_os = "linux"))]
fn peer_uid(conn: &UnixStream) -> Option<libc::uid_t> {
    use std::os::fd::AsRawFd;
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: the socket is open; the out values are valid.
    let ok = unsafe { libc::getpeereid(conn.as_raw_fd(), &mut uid, &mut gid) } == 0;
    ok.then_some(uid)
}
