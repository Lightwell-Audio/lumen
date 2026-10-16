//! What a scan child tells Lumen about one module, and how it is carried.
//!
//! The reply is one buffer on the child's real stdout: a magic, a version, a
//! kind and a body. Everything read from it is bounded, like the protocol's
//! messages, because a module that has been loaded may have damaged
//! anything. A reply of any other shape is "malformed", never trusted.

use crate::protocol::{ClassId, MAX_CLASSES, MAX_TEXT};
use crate::wire::{Reader, WireError, Writer};

/// First four bytes of a scan reply.
pub const SCAN_MAGIC: u32 = 0x4e43_534c; // "LSCN"
/// Bumped when the reply changes shape.
pub const SCAN_VERSION: u16 = 1;
/// The most bytes a reply may take: every class at its text limits, with room.
pub const MAX_REPLY: usize = 2 << 20;

const KIND_REPORT: u8 = 1;
const KIND_FAILED: u8 = 2;

/// One class a factory offers, with the details a browser needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanClass {
    pub id: ClassId,
    pub name: String,
    pub category: String,
    pub vendor: String,
    /// The factory's own sub-categories text, `|`-separated (`Instrument|Synth`).
    pub subcategories: String,
    pub version: String,
    pub sdk_version: String,
}

/// A module that loaded and answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanReport {
    pub vendor: String,
    pub url: String,
    pub classes: Vec<ScanClass>,
    /// How long loading and enumerating took, in milliseconds.
    pub load_ms: u32,
}

/// What the child says: a report, or why it could not make one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanReply {
    Report(ScanReport),
    Failed(String),
}

/// Cuts `text` to at most `max` bytes on a character boundary, so a module
/// with long names cannot make its own report malformed.
pub fn clip(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn put(w: &mut Writer, text: &str) {
    w.str(clip(text, MAX_TEXT));
}

impl ScanReply {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u32(SCAN_MAGIC);
        w.u16(SCAN_VERSION);
        match self {
            ScanReply::Report(report) => {
                w.u8(KIND_REPORT);
                put(&mut w, &report.vendor);
                put(&mut w, &report.url);
                w.u32(report.load_ms);
                let classes = &report.classes[..report.classes.len().min(MAX_CLASSES)];
                w.u32(classes.len() as u32);
                for class in classes {
                    w.raw(&class.id);
                    put(&mut w, &class.name);
                    put(&mut w, &class.category);
                    put(&mut w, &class.vendor);
                    put(&mut w, &class.subcategories);
                    put(&mut w, &class.version);
                    put(&mut w, &class.sdk_version);
                }
            }
            ScanReply::Failed(message) => {
                w.u8(KIND_FAILED);
                put(&mut w, message);
            }
        }
        w.into_bytes()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        if bytes.len() > MAX_REPLY {
            return Err(WireError::TooLarge {
                what: "scan reply",
                max: MAX_REPLY,
                got: bytes.len(),
            });
        }
        let mut r = Reader::new(bytes);
        if r.u32()? != SCAN_MAGIC {
            return Err(WireError::Invalid("scan reply magic"));
        }
        if r.u16()? != SCAN_VERSION {
            return Err(WireError::Invalid("scan reply version"));
        }
        let reply = match r.u8()? {
            KIND_REPORT => {
                let vendor = r.str("vendor", MAX_TEXT)?;
                let url = r.str("url", MAX_TEXT)?;
                let load_ms = r.u32()?;
                // A class takes at least its id and six lengths.
                let n = r.count("classes", MAX_CLASSES, 16 + 6 * 4)?;
                let mut classes = Vec::with_capacity(n);
                for _ in 0..n {
                    classes.push(ScanClass {
                        id: r.raw::<16>()?,
                        name: r.str("class name", MAX_TEXT)?,
                        category: r.str("class category", MAX_TEXT)?,
                        vendor: r.str("class vendor", MAX_TEXT)?,
                        subcategories: r.str("class sub-categories", MAX_TEXT)?,
                        version: r.str("class version", MAX_TEXT)?,
                        sdk_version: r.str("class sdk version", MAX_TEXT)?,
                    });
                }
                ScanReply::Report(ScanReport {
                    vendor,
                    url,
                    classes,
                    load_ms,
                })
            }
            KIND_FAILED => ScanReply::Failed(r.str("failure", MAX_TEXT)?),
            _ => return Err(WireError::Invalid("scan reply kind")),
        };
        r.finish()?;
        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(n: u8) -> ScanClass {
        ScanClass {
            id: [n; 16],
            name: format!("Class {n}"),
            category: "Audio Module Class".into(),
            vendor: "Vendor".into(),
            subcategories: "Instrument|Synth".into(),
            version: "1.2.3".into(),
            sdk_version: "VST 3.7".into(),
        }
    }

    fn report(classes: Vec<ScanClass>) -> ScanReply {
        ScanReply::Report(ScanReport {
            vendor: "Vendor".into(),
            url: "https://example.invalid".into(),
            classes,
            load_ms: 42,
        })
    }

    #[test]
    fn a_report_round_trips() {
        let reply = report(vec![class(1), class(2)]);
        assert_eq!(ScanReply::decode(&reply.encode()), Ok(reply));
    }

    #[test]
    fn a_failure_round_trips() {
        let reply = ScanReply::Failed("no factory".into());
        assert_eq!(ScanReply::decode(&reply.encode()), Ok(reply));
    }

    #[test]
    fn every_truncation_is_refused_without_panicking() {
        let bytes = report(vec![class(1), class(2)]).encode();
        for cut in 0..bytes.len() {
            assert!(ScanReply::decode(&bytes[..cut]).is_err(), "cut at {cut}");
        }
    }

    #[test]
    fn trailing_bytes_a_wrong_magic_and_a_wrong_version_are_refused() {
        let mut bytes = report(vec![class(1)]).encode();
        bytes.push(0);
        assert!(ScanReply::decode(&bytes).is_err());

        let mut bytes = report(vec![class(1)]).encode();
        bytes[0] ^= 0xff;
        assert_eq!(
            ScanReply::decode(&bytes),
            Err(WireError::Invalid("scan reply magic"))
        );

        let mut bytes = report(vec![class(1)]).encode();
        bytes[4] = 9;
        assert_eq!(
            ScanReply::decode(&bytes),
            Err(WireError::Invalid("scan reply version"))
        );
    }

    #[test]
    fn an_absurd_class_count_is_refused_before_anything_is_allocated() {
        let mut w = Writer::new();
        w.u32(SCAN_MAGIC);
        w.u16(SCAN_VERSION);
        w.u8(KIND_REPORT);
        w.str("v");
        w.str("u");
        w.u32(0);
        w.u32(u32::MAX);
        assert!(matches!(
            ScanReply::decode(&w.into_bytes()),
            Err(WireError::TooLarge {
                what: "classes",
                ..
            })
        ));
    }

    #[test]
    fn an_oversized_reply_is_refused() {
        let bytes = vec![0u8; MAX_REPLY + 1];
        assert!(matches!(
            ScanReply::decode(&bytes),
            Err(WireError::TooLarge { .. })
        ));
    }

    #[test]
    fn long_text_is_cut_on_a_character_boundary_and_still_decodes() {
        let long = "é".repeat(MAX_TEXT); // two bytes each
        let mut c = class(1);
        c.name = long;
        let ScanReply::Report(back) = ScanReply::decode(&report(vec![c]).encode()).unwrap() else {
            panic!("a report");
        };
        assert!(back.classes[0].name.len() <= MAX_TEXT);
        assert!(back.classes[0].name.chars().all(|ch| ch == 'é'));
    }

    #[test]
    fn invalid_text_is_refused() {
        let mut w = Writer::new();
        w.u32(SCAN_MAGIC);
        w.u16(SCAN_VERSION);
        w.u8(KIND_FAILED);
        w.bytes(&[0xff, 0xfe]);
        assert_eq!(
            ScanReply::decode(&w.into_bytes()),
            Err(WireError::Invalid("text"))
        );
    }

    #[test]
    fn the_class_list_is_capped_when_encoding() {
        let classes: Vec<ScanClass> = (0..MAX_CLASSES + 10).map(|n| class(n as u8)).collect();
        let ScanReply::Report(back) = ScanReply::decode(&report(classes).encode()).unwrap() else {
            panic!("a report");
        };
        assert_eq!(back.classes.len(), MAX_CLASSES);
    }
}
