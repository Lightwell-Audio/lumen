//! VST3 preset files (`.vstpreset`): reading, checking and writing.
//!
//! A file is a 48-byte header (the tag `VST3`, a version, the plugin class id
//! as 32 hexadecimal characters, and the offset of the chunk list), the chunks'
//! data, and the chunk list: the tag `List`, a count, and for each chunk its
//! four-character id, its offset and its size. All numbers are little-endian.
//! The chunks Lumen uses are `Comp` (the component's state), `Cont` (the edit
//! controller's state) and `Info` (descriptive XML); others are skipped.
//!
//! A preset file comes from outside Lumen, so nothing in it is trusted: every
//! offset, size and count is checked against the file and against the limits
//! below before anything is read or allocated, and a damaged file is an error
//! that says what is wrong, never a panic.

use std::fmt;

use crate::protocol::{ClassId, MAX_STATE};

/// The longest file read: both states and the description, with room.
pub const MAX_FILE: usize = 2 * MAX_STATE + MAX_INFO + 4096;
/// The longest description chunk kept.
pub const MAX_INFO: usize = 1 << 16;
/// The most chunks a file may list.
pub const MAX_CHUNKS: usize = 64;

const TAG: &[u8; 4] = b"VST3";
const LIST: &[u8; 4] = b"List";
const VERSION: i32 = 1;
const HEADER: usize = 4 + 4 + 32 + 8;
const ENTRY: usize = 4 + 8 + 8;

const COMP: [u8; 4] = *b"Comp";
const CONT: [u8; 4] = *b"Cont";
const INFO: [u8; 4] = *b"Info";

/// What Lumen takes from a preset file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    /// The plugin class the preset was made for.
    pub class_id: ClassId,
    /// The component's state: what `IComponent::setState` reads.
    pub component: Vec<u8>,
    /// The edit controller's own state, when the file holds it.
    pub controller: Option<Vec<u8>>,
    /// The description (XML), when the file holds it.
    pub info: Option<Vec<u8>>,
}

/// Why a file is not a preset Lumen can use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetError {
    /// Shorter than a header, or longer than Lumen reads.
    Size,
    /// Not a VST3 preset file.
    NotAPreset,
    /// A format version Lumen does not know.
    Version(i32),
    /// The class id is not 32 hexadecimal characters.
    ClassId,
    /// The chunk list is missing, past the end of the file or too long.
    List,
    /// A chunk lies outside the file or over the list.
    Chunk([u8; 4]),
    /// The same chunk is listed twice.
    Duplicate([u8; 4]),
    /// A state chunk or the description is larger than Lumen accepts.
    TooLarge([u8; 4]),
    /// The file has no component state.
    NoState,
}

impl fmt::Display for PresetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = |id: &[u8; 4]| String::from_utf8_lossy(id).into_owned();
        match self {
            Self::Size => write!(f, "the file is not the size of a preset"),
            Self::NotAPreset => write!(f, "this is not a VST3 preset file"),
            Self::Version(v) => write!(f, "preset file version {v} is not supported"),
            Self::ClassId => write!(f, "the file does not name a plugin"),
            Self::List => write!(f, "the file's chunk list is damaged"),
            Self::Chunk(id) => write!(f, "the file's {} data is damaged", name(id)),
            Self::Duplicate(id) => write!(f, "the file lists {} twice", name(id)),
            Self::TooLarge(id) => write!(f, "the file's {} data is too large", name(id)),
            Self::NoState => write!(f, "the file holds no plugin state"),
        }
    }
}

impl std::error::Error for PresetError {}

fn hex(id: &ClassId) -> String {
    id.iter().map(|b| format!("{b:02X}")).collect()
}

fn unhex(text: &[u8]) -> Option<ClassId> {
    if text.len() != 32 {
        return None;
    }
    let mut out = [0_u8; 16];
    for (byte, pair) in out.iter_mut().zip(text.chunks(2)) {
        let digit = |c: u8| (c as char).to_digit(16);
        *byte = (digit(pair[0])? * 16 + digit(pair[1])?) as u8;
    }
    Some(out)
}

fn i64_at(bytes: &[u8], at: usize) -> Option<i64> {
    Some(i64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

impl Preset {
    /// Checks and reads a preset file.
    pub fn parse(bytes: &[u8]) -> Result<Self, PresetError> {
        if bytes.len() < HEADER + 8 || bytes.len() > MAX_FILE {
            return Err(PresetError::Size);
        }
        if &bytes[..4] != TAG {
            return Err(PresetError::NotAPreset);
        }
        let version = i32::from_le_bytes(bytes[4..8].try_into().expect("4 bytes"));
        if version != VERSION {
            return Err(PresetError::Version(version));
        }
        let class_id = unhex(&bytes[8..40]).ok_or(PresetError::ClassId)?;
        let list_at = i64_at(bytes, 40).ok_or(PresetError::List)?;
        let list_at = usize::try_from(list_at).map_err(|_| PresetError::List)?;
        if list_at < HEADER || list_at.checked_add(8).is_none_or(|end| end > bytes.len()) {
            return Err(PresetError::List);
        }
        if &bytes[list_at..list_at + 4] != LIST {
            return Err(PresetError::List);
        }
        let count = i32::from_le_bytes(bytes[list_at + 4..list_at + 8].try_into().expect("4"));
        let count = usize::try_from(count).map_err(|_| PresetError::List)?;
        if count > MAX_CHUNKS || list_at + 8 + count * ENTRY > bytes.len() {
            return Err(PresetError::List);
        }
        let mut seen: Vec<[u8; 4]> = Vec::with_capacity(count);
        let (mut component, mut controller, mut info) = (None, None, None);
        for k in 0..count {
            let at = list_at + 8 + k * ENTRY;
            let id: [u8; 4] = bytes[at..at + 4].try_into().expect("4 bytes");
            let offset = i64_at(bytes, at + 4).ok_or(PresetError::List)?;
            let size = i64_at(bytes, at + 12).ok_or(PresetError::List)?;
            let (Ok(offset), Ok(size)) = (usize::try_from(offset), usize::try_from(size)) else {
                return Err(PresetError::Chunk(id));
            };
            // The data sits between the header and the list.
            if offset < HEADER || offset.checked_add(size).is_none_or(|end| end > list_at) {
                return Err(PresetError::Chunk(id));
            }
            if seen.contains(&id) {
                return Err(PresetError::Duplicate(id));
            }
            seen.push(id);
            let data = &bytes[offset..offset + size];
            match id {
                COMP | CONT if size > MAX_STATE => return Err(PresetError::TooLarge(id)),
                INFO if size > MAX_INFO => return Err(PresetError::TooLarge(id)),
                COMP => component = Some(data.to_vec()),
                CONT => controller = Some(data.to_vec()),
                INFO => info = Some(data.to_vec()),
                _ => {}
            }
        }
        let component = component
            .filter(|c: &Vec<u8>| !c.is_empty())
            .ok_or(PresetError::NoState)?;
        Ok(Self {
            class_id,
            component,
            controller,
            info,
        })
    }

    /// The preset as a file.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut chunks: Vec<([u8; 4], &[u8])> = vec![(COMP, &self.component)];
        if let Some(c) = &self.controller {
            chunks.push((CONT, c));
        }
        if let Some(i) = &self.info {
            chunks.push((INFO, i));
        }
        let data: usize = chunks.iter().map(|(_, d)| d.len()).sum();
        let list_at = HEADER + data;
        let mut out = Vec::with_capacity(list_at + 8 + chunks.len() * ENTRY);
        out.extend_from_slice(TAG);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(hex(&self.class_id).as_bytes());
        out.extend_from_slice(&(list_at as i64).to_le_bytes());
        let mut entries = Vec::with_capacity(chunks.len());
        for (id, d) in &chunks {
            entries.push((*id, out.len() as i64, d.len() as i64));
            out.extend_from_slice(d);
        }
        out.extend_from_slice(LIST);
        out.extend_from_slice(&(chunks.len() as i32).to_le_bytes());
        for (id, offset, size) in entries {
            out.extend_from_slice(&id);
            out.extend_from_slice(&offset.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
        }
        out
    }
}

/// A description chunk naming the plugin the preset is for, as other hosts
/// write it.
pub fn info_xml(plugin_name: &str) -> Vec<u8> {
    let name: String = plugin_name
        .chars()
        .filter(|c| !c.is_control())
        .flat_map(|c| match c {
            '&' => "&amp;".chars().collect::<Vec<_>>(),
            '<' => "&lt;".chars().collect(),
            '>' => "&gt;".chars().collect(),
            '"' => "&quot;".chars().collect(),
            c => vec![c],
        })
        .collect();
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<MetaInfo>\n  \
         <Attr id=\"MediaType\" value=\"VstPreset\" type=\"string\" flags=\"writeonce\"/>\n  \
         <Attr id=\"PlugInName\" value=\"{name}\" type=\"string\" flags=\"writeonce\"/>\n</MetaInfo>\n"
    );
    let mut bytes = xml.into_bytes();
    bytes.truncate(MAX_INFO);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Preset {
        Preset {
            class_id: [
                0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54,
                0x32, 0x10,
            ],
            component: vec![1, 2, 3, 4, 5],
            controller: Some(vec![9, 8]),
            info: Some(info_xml("Glass pad")),
        }
    }

    #[test]
    fn a_preset_written_is_the_preset_read() {
        let preset = sample();
        assert_eq!(Preset::parse(&preset.to_bytes()), Ok(preset));
        let bare = Preset {
            controller: None,
            info: None,
            ..sample()
        };
        assert_eq!(Preset::parse(&bare.to_bytes()), Ok(bare));
    }

    #[test]
    fn the_file_has_the_layout_other_hosts_write() {
        let bytes = sample().to_bytes();
        assert_eq!(&bytes[..4], b"VST3");
        assert_eq!(&bytes[4..8], &1_i32.to_le_bytes());
        assert_eq!(&bytes[8..40], b"0123456789ABCDEFFEDCBA9876543210");
        let list_at = i64::from_le_bytes(bytes[40..48].try_into().unwrap()) as usize;
        assert_eq!(&bytes[list_at..list_at + 4], b"List");
        assert_eq!(&bytes[HEADER..HEADER + 5], &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn unknown_chunks_are_skipped() {
        let mut bytes = sample().to_bytes();
        // Add a chunk "Prog" after the data, before the list: rebuild by hand.
        let list_at = i64::from_le_bytes(bytes[40..48].try_into().unwrap()) as usize;
        let list = bytes.split_off(list_at);
        bytes.extend_from_slice(&[7, 7, 7]);
        let new_list_at = bytes.len();
        bytes[40..48].copy_from_slice(&(new_list_at as i64).to_le_bytes());
        let count = i32::from_le_bytes(list[4..8].try_into().unwrap()) + 1;
        bytes.extend_from_slice(b"List");
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&list[8..]);
        bytes.extend_from_slice(b"Prog");
        bytes.extend_from_slice(&(list_at as i64).to_le_bytes());
        bytes.extend_from_slice(&3_i64.to_le_bytes());
        assert_eq!(
            Preset::parse(&bytes).unwrap().component,
            vec![1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn damaged_files_are_refused_with_a_reason() {
        let good = sample().to_bytes();
        assert_eq!(Preset::parse(&good[..20]), Err(PresetError::Size));
        assert_eq!(Preset::parse(&[0; 4096]), Err(PresetError::NotAPreset));
        let mut bad = good.clone();
        bad[4] = 2;
        assert_eq!(Preset::parse(&bad), Err(PresetError::Version(2)));
        let mut bad = good.clone();
        bad[8] = b'Z';
        assert_eq!(Preset::parse(&bad), Err(PresetError::ClassId));
        let mut bad = good.clone();
        bad[40..48].copy_from_slice(&i64::MAX.to_le_bytes());
        assert_eq!(Preset::parse(&bad), Err(PresetError::List));
        let mut bad = good.clone();
        bad[40..48].copy_from_slice(&(-5_i64).to_le_bytes());
        assert_eq!(Preset::parse(&bad), Err(PresetError::List));
        // A truncated file loses its list.
        assert_eq!(
            Preset::parse(&good[..good.len() - 30]),
            Err(PresetError::List)
        );
    }

    #[test]
    fn hostile_chunk_entries_are_refused() {
        let good = sample().to_bytes();
        let list_at = i64::from_le_bytes(good[40..48].try_into().unwrap()) as usize;
        let entry = list_at + 8;
        let with = |f: &dyn Fn(&mut Vec<u8>)| {
            let mut b = good.clone();
            f(&mut b);
            Preset::parse(&b)
        };
        // An offset before the header, a size past the list, a negative size,
        // an offset plus size that overflows.
        assert_eq!(
            with(&|b| b[entry + 4..entry + 12].copy_from_slice(&0_i64.to_le_bytes())),
            Err(PresetError::Chunk(COMP))
        );
        assert_eq!(
            with(&|b| b[entry + 12..entry + 20].copy_from_slice(&1_000_000_i64.to_le_bytes())),
            Err(PresetError::Chunk(COMP))
        );
        assert_eq!(
            with(&|b| b[entry + 12..entry + 20].copy_from_slice(&(-1_i64).to_le_bytes())),
            Err(PresetError::Chunk(COMP))
        );
        assert_eq!(
            with(&|b| {
                b[entry + 4..entry + 12].copy_from_slice(&(HEADER as i64).to_le_bytes());
                b[entry + 12..entry + 20].copy_from_slice(&i64::MAX.to_le_bytes());
            }),
            Err(PresetError::Chunk(COMP))
        );
        // The same id twice.
        assert_eq!(
            with(&|b| {
                let (first, second) = (entry, entry + ENTRY);
                let id: [u8; 4] = b[first..first + 4].try_into().unwrap();
                b[second..second + 4].copy_from_slice(&id);
            }),
            Err(PresetError::Duplicate(COMP))
        );
        // A count that claims more entries than the file holds or allows.
        assert_eq!(
            with(&|b| b[list_at + 4..list_at + 8].copy_from_slice(&1000_i32.to_le_bytes())),
            Err(PresetError::List)
        );
        assert_eq!(
            with(&|b| b[list_at + 4..list_at + 8].copy_from_slice(&(-1_i32).to_le_bytes())),
            Err(PresetError::List)
        );
    }

    #[test]
    fn a_preset_without_state_is_refused() {
        let empty = Preset {
            component: Vec::new(),
            ..sample()
        };
        assert_eq!(Preset::parse(&empty.to_bytes()), Err(PresetError::NoState));
    }

    #[test]
    fn an_oversized_state_is_refused() {
        let big = Preset {
            component: vec![0; MAX_STATE + 1],
            ..sample()
        };
        assert_eq!(
            Preset::parse(&big.to_bytes()),
            Err(PresetError::TooLarge(COMP))
        );
    }

    #[test]
    fn the_description_escapes_the_name() {
        let xml = String::from_utf8(info_xml("A & <B> \"c\"\n")).unwrap();
        assert!(xml.contains("A &amp; &lt;B&gt; &quot;c&quot;"));
    }
}
