//! Frames: a magic number, a version, a kind and a bounded payload.

use std::io::{self, Read, Write};

use super::*;
use crate::wire::WireError;

/// Why reading a frame failed.
#[derive(Debug)]
pub enum FrameError {
    /// The other side closed the stream between frames.
    Closed,
    Io(io::Error),
    Wire(WireError),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Closed => write!(f, "stream closed"),
            FrameError::Io(e) => write!(f, "{e}"),
            FrameError::Wire(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<WireError> for FrameError {
    fn from(e: WireError) -> Self {
        FrameError::Wire(e)
    }
}

pub(super) fn write_frame<W: Write>(out: &mut W, kind: u16, payload: &[u8]) -> io::Result<()> {
    let mut header = [0_u8; 12];
    header[0..4].copy_from_slice(&MAGIC.to_le_bytes());
    header[4..6].copy_from_slice(&VERSION.to_le_bytes());
    header[6..8].copy_from_slice(&kind.to_le_bytes());
    header[8..12].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    out.write_all(&header)?;
    out.write_all(payload)?;
    out.flush()
}

fn read_frame<R: Read>(input: &mut R) -> Result<(u16, Vec<u8>), FrameError> {
    let mut header = [0_u8; 12];
    // End of stream before the first byte is an orderly close; partway
    // through a header it is a torn frame.
    let mut got = 0;
    while got < header.len() {
        match input.read(&mut header[got..]) {
            Ok(0) if got == 0 => return Err(FrameError::Closed),
            Ok(0) => return Err(FrameError::Wire(WireError::Truncated)),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(FrameError::Io(e)),
        }
    }
    let magic = u32::from_le_bytes(header[0..4].try_into().expect("four bytes"));
    let version = u16::from_le_bytes(header[4..6].try_into().expect("two bytes"));
    let kind = u16::from_le_bytes(header[6..8].try_into().expect("two bytes"));
    let len = u32::from_le_bytes(header[8..12].try_into().expect("four bytes")) as usize;
    if magic != MAGIC {
        return Err(WireError::Invalid("frame marker").into());
    }
    if version != VERSION {
        return Err(WireError::Invalid("protocol version").into());
    }
    if len > MAX_MESSAGE {
        return Err(WireError::TooLarge {
            what: "message",
            max: MAX_MESSAGE,
            got: len,
        }
        .into());
    }
    let mut payload = vec![0; len];
    input.read_exact(&mut payload).map_err(|e| {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            FrameError::Wire(WireError::Truncated)
        } else {
            FrameError::Io(e)
        }
    })?;
    Ok((kind, payload))
}

pub fn write_request<W: Write>(out: &mut W, request: &Request) -> io::Result<()> {
    let (kind, payload) = request.encode();
    write_frame(out, kind, &payload)
}

pub fn read_request<R: Read>(input: &mut R) -> Result<Request, FrameError> {
    let (kind, payload) = read_frame(input)?;
    Ok(Request::decode(kind, &payload)?)
}

pub fn write_response<W: Write>(out: &mut W, response: &Response) -> io::Result<()> {
    let (kind, payload) = response.encode();
    write_frame(out, kind, &payload)
}

pub fn read_response<R: Read>(input: &mut R) -> Result<Response, FrameError> {
    let (kind, payload) = read_frame(input)?;
    Ok(Response::decode(kind, &payload)?)
}
