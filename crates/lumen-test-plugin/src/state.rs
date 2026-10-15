//! The saved state of a component: a tag, the plugin kind and every
//! parameter's normalized value.

use crate::params::Kind;

const MAGIC: [u8; 4] = *b"LTP1";
pub const LEN: usize = 4 + 1 + 2 * 8;

pub fn encode(kind: Kind, values: [f64; 2]) -> [u8; LEN] {
    let mut out = [0; LEN];
    out[..4].copy_from_slice(&MAGIC);
    out[4] = match kind {
        Kind::Instrument => 0,
        Kind::Effect => 1,
    };
    out[5..13].copy_from_slice(&values[0].to_le_bytes());
    out[13..21].copy_from_slice(&values[1].to_le_bytes());
    out
}

/// The values saved by `encode` for the same kind, or `None` for anything
/// else (wrong tag, wrong plugin, wrong length, values that are not numbers).
pub fn decode(kind: Kind, bytes: &[u8]) -> Option<[f64; 2]> {
    if bytes.len() != LEN || bytes[..4] != MAGIC {
        return None;
    }
    let saved = match bytes[4] {
        0 => Kind::Instrument,
        1 => Kind::Effect,
        _ => return None,
    };
    if saved != kind {
        return None;
    }
    let value = |at: usize| f64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"));
    let values = [value(5), value(13)];
    values
        .iter()
        .all(|v| (0.0..=1.0).contains(v))
        .then_some(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let bytes = encode(Kind::Effect, [0.25, 0.5]);
        assert_eq!(decode(Kind::Effect, &bytes), Some([0.25, 0.5]));
    }

    #[test]
    fn rejects_what_it_did_not_write() {
        let bytes = encode(Kind::Effect, [0.25, 0.5]);
        assert_eq!(decode(Kind::Instrument, &bytes), None);
        assert_eq!(decode(Kind::Effect, &bytes[..LEN - 1]), None);
        assert_eq!(decode(Kind::Effect, &[0xAB; LEN]), None);
        assert_eq!(
            decode(Kind::Effect, &encode(Kind::Effect, [f64::NAN, 0.0])),
            None
        );
        assert_eq!(
            decode(Kind::Effect, &encode(Kind::Effect, [2.0, 0.0])),
            None
        );
    }
}
