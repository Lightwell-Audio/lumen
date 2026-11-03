//! What later starts and the instance say to each other: one line each
//! way, at most 64 KiB, starting with the protocol's name and version.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::time::Instant;

/// The protocol's name and version: a line that does not start with it is
/// refused.
const MAGIC: &str = "LUMEN-INSTANCE 1";
/// The longest line either side reads.
pub(crate) const MAX: usize = 64 * 1024;

/// What a later start asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Request {
    /// Open this project (an absolute path).
    Open(PathBuf),
    /// Only come to the front.
    Show,
}

/// What the instance answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reply {
    /// The window took the request.
    Accepted,
    /// It refused it, saying why.
    Rejected(String),
    /// The window did not take it in time.
    NotAnswering,
}

impl Request {
    /// The line for this request; `None` for a path that cannot be sent
    /// (not valid UTF-8, or holding a line break).
    pub(crate) fn encode(&self) -> Option<Vec<u8>> {
        let line = match self {
            Request::Show => format!("{MAGIC} show\n"),
            Request::Open(path) => {
                let path = path.to_str().filter(|p| !p.contains(['\n', '\r']))?;
                format!("{MAGIC} open {path}\n")
            }
        };
        (line.len() <= MAX).then(|| line.into_bytes())
    }

    pub(crate) fn decode(line: &[u8]) -> Result<Self, String> {
        let line = std::str::from_utf8(line).map_err(|_| "not UTF-8".to_owned())?;
        let rest = line
            .trim_end_matches(['\n', '\r'])
            .strip_prefix(MAGIC)
            .and_then(|r| r.strip_prefix(' '))
            .ok_or_else(|| "not a request from the app".to_owned())?;
        match rest.split_once(' ') {
            None if rest == "show" => Ok(Request::Show),
            Some(("open", path)) if !path.is_empty() => Ok(Request::Open(PathBuf::from(path))),
            _ => Err(format!("unknown request: {rest}")),
        }
    }
}

impl Reply {
    pub(crate) fn encode(&self) -> Vec<u8> {
        let line = match self {
            Reply::Accepted => "accepted".to_owned(),
            Reply::NotAnswering => "not-answering".to_owned(),
            Reply::Rejected(reason) => format!("rejected {}", reason.replace(['\n', '\r'], " ")),
        };
        format!("{MAGIC} {line}\n").into_bytes()
    }

    pub(crate) fn decode(line: &[u8]) -> Result<Self, String> {
        let line = std::str::from_utf8(line).map_err(|_| "not UTF-8".to_owned())?;
        let rest = line
            .trim_end_matches(['\n', '\r'])
            .strip_prefix(MAGIC)
            .and_then(|r| r.strip_prefix(' '))
            .ok_or_else(|| "not a reply from the app".to_owned())?;
        match rest.split_once(' ') {
            None if rest == "accepted" => Ok(Reply::Accepted),
            None if rest == "not-answering" => Ok(Reply::NotAnswering),
            Some(("rejected", reason)) => Ok(Reply::Rejected(reason.to_owned())),
            _ => Err(format!("unknown reply: {rest}")),
        }
    }
}

/// Reads one line (through its `\n`), at most [`MAX`] bytes, before
/// `deadline`. The reader's own timeouts end a stalled read.
pub(crate) fn read_line(reader: &mut impl Read, deadline: Instant) -> io::Result<Vec<u8>> {
    let mut line = Vec::new();
    let mut chunk = [0u8; 4096];
    while !line.contains(&b'\n') {
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        line.extend_from_slice(&chunk[..n]);
        if line.len() > MAX {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "too long"));
        }
    }
    Ok(line)
}

/// Serves one connection: reads a request, hands it to `deliver`, writes
/// the reply. A request that cannot be read is answered with a refusal
/// when it can be.
pub(crate) fn serve(
    conn: &mut (impl Read + Write),
    deliver: &dyn Fn(Request) -> Reply,
    deadline: Instant,
) -> io::Result<()> {
    let reply = match read_line(conn, deadline) {
        Ok(line) => match Request::decode(&line) {
            Ok(request) => deliver(request),
            Err(e) => Reply::Rejected(e),
        },
        Err(e) if e.kind() == io::ErrorKind::InvalidData => {
            Reply::Rejected("the request is too long".into())
        }
        Err(e) => return Err(e),
    };
    conn.write_all(&reply.encode())?;
    conn.flush()
}

/// Sends `request` on `conn` and reads the reply before `deadline`.
pub(crate) fn ask(
    conn: &mut (impl Read + Write),
    request: &Request,
    deadline: Instant,
) -> io::Result<Reply> {
    let bytes = request
        .encode()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path cannot be sent"))?;
    conn.write_all(&bytes)?;
    conn.flush()?;
    let line = read_line(conn, deadline)?;
    Reply::decode(&line).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn requests_and_replies_round_trip() {
        for r in [
            Request::Show,
            Request::Open(PathBuf::from("C:/Songs/Night Drive.lumen")),
        ] {
            assert_eq!(Request::decode(&r.encode().unwrap()).unwrap(), r);
        }
        for r in [
            Reply::Accepted,
            Reply::NotAnswering,
            Reply::Rejected("busy now".into()),
        ] {
            assert_eq!(Reply::decode(&r.encode()).unwrap(), r);
        }
    }

    #[test]
    fn a_path_with_a_line_break_is_not_sent() {
        assert!(
            Request::Open(PathBuf::from("a\nb.lumen"))
                .encode()
                .is_none()
        );
    }

    #[test]
    fn lines_without_the_protocol_name_are_refused() {
        assert!(Request::decode(b"open C:/x.lumen\n").is_err());
        assert!(Request::decode(b"LUMEN-INSTANCE 2 show\n").is_err());
        assert!(Request::decode(b"LUMEN-INSTANCE 1 open \n").is_err());
        assert!(Reply::decode(b"LUMEN-INSTANCE 1 maybe\n").is_err());
    }

    /// A connection: what the other side wrote, and what this side writes.
    struct Pipe {
        input: std::io::Cursor<Vec<u8>>,
        output: Vec<u8>,
    }
    impl Read for Pipe {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.input.read(buf)
        }
    }
    impl Write for Pipe {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.output.write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn later() -> Instant {
        Instant::now() + Duration::from_secs(1)
    }

    #[test]
    fn serving_hands_the_request_over_and_writes_the_reply() {
        let mut conn = Pipe {
            input: std::io::Cursor::new(Request::Show.encode().unwrap()),
            output: Vec::new(),
        };
        serve(
            &mut conn,
            &|r| {
                assert_eq!(r, Request::Show);
                Reply::Accepted
            },
            later(),
        )
        .unwrap();
        assert_eq!(Reply::decode(&conn.output).unwrap(), Reply::Accepted);
    }

    #[test]
    fn a_request_over_the_limit_is_refused() {
        let mut long = vec![b'x'; MAX + 10];
        long.push(b'\n');
        let mut conn = Pipe {
            input: std::io::Cursor::new(long),
            output: Vec::new(),
        };
        serve(&mut conn, &|_| unreachable!("never delivered"), later()).unwrap();
        assert!(matches!(
            Reply::decode(&conn.output).unwrap(),
            Reply::Rejected(_)
        ));
    }

    #[test]
    fn a_garbled_request_is_refused_without_reaching_the_window() {
        let mut conn = Pipe {
            input: std::io::Cursor::new(b"hello\n".to_vec()),
            output: Vec::new(),
        };
        serve(&mut conn, &|_| unreachable!("never delivered"), later()).unwrap();
        assert!(matches!(
            Reply::decode(&conn.output).unwrap(),
            Reply::Rejected(_)
        ));
    }

    #[test]
    fn a_connection_that_ends_early_is_an_error() {
        let mut conn = Pipe {
            input: std::io::Cursor::new(b"LUMEN-INST".to_vec()),
            output: Vec::new(),
        };
        assert!(serve(&mut conn, &|_| Reply::Accepted, later()).is_err());
    }
}
