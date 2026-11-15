use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::protocol::{self, Reply, Request};
use super::{Deliver, IO_TIMEOUT, Place};

const MAX_SOCKET_PATH: usize = 100;

fn folder(place: &Place) -> PathBuf {
    let name = format!(
        "{}-instance",
        lumen_core::product::PRODUCT_NAME.to_lowercase()
    );
    let file = format!("{}.sock", place.key);
    let uid = unsafe { libc::getuid() };
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .map(|p| p.join(&name));
    let temporary = std::env::temp_dir().join(format!("{name}-{uid}"));
    let settings = place.folder.join(&name);
    let candidates: Vec<PathBuf> = runtime.into_iter().chain([temporary, settings]).collect();
    let fits = |dir: &&PathBuf| dir.join(&file).as_os_str().len() <= MAX_SOCKET_PATH;
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

pub(super) fn serve(place: &Place, deliver: Deliver) -> io::Result<()> {
    let dir = folder(place);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    let meta = std::fs::metadata(&dir)?;
    if meta.uid() != unsafe { libc::getuid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the instance folder belongs to another user",
        ));
    }
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    let path = socket(place);
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

pub(super) fn send(place: &Place, request: &Request, left: Duration) -> io::Result<Reply> {
    let deadline = Instant::now() + left;
    let mut conn = UnixStream::connect(socket(place))?;
    let timeout = left.max(Duration::from_millis(100));
    conn.set_read_timeout(Some(timeout))?;
    conn.set_write_timeout(Some(timeout))?;
    protocol::ask(&mut conn, request, deadline)
}

fn same_user(conn: &UnixStream) -> bool {
    peer_uid(conn) == Some(unsafe { libc::getuid() })
}

#[cfg(target_os = "linux")]
fn peer_uid(conn: &UnixStream) -> Option<libc::uid_t> {
    use std::os::fd::AsRawFd;
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
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
    let ok = unsafe { libc::getpeereid(conn.as_raw_fd(), &mut uid, &mut gid) } == 0;
    ok.then_some(uid)
}
