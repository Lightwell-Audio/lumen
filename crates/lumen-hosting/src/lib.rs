pub mod link;
pub mod manager;
pub mod process;

pub use link::{Counts, Link, Pair};
pub use lumen_plugin_host::client::HostLog;
pub use lumen_plugin_host::preset::{MAX_FILE, Preset, PresetError, info_xml};
pub use lumen_plugin_host::protocol::{
    Aux, BusInfo, Edit, EditKind, EditorBounds, EditorStatus, FLAG_CAN_AUTOMATE, FLAG_HIDDEN,
    FLAG_PROGRAM_CHANGE, FLAG_READ_ONLY, MAX_CHANNELS, ParamInfo, ProgramList, Taken, UnitInfo,
};
pub use manager::{Event, Manager};
pub use process::{
    CAPTURE, Captured, Deadlines, EditorEvent, Failure, Happened, Linked, Start, Step, WAKE,
};

pub fn parse_class_id(hex: &str) -> Option<[u8; 16]> {
    if hex.len() != 32 || !hex.is_ascii() {
        return None;
    }
    let mut id = [0_u8; 16];
    for (i, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_ids_parse_from_hex_in_either_case() {
        let id = parse_class_id("00112233445566778899aAbBcCdDeEfF").unwrap();
        assert_eq!(id[0], 0x00);
        assert_eq!(id[1], 0x11);
        assert_eq!(id[10], 0xAA);
        assert_eq!(id[15], 0xFF);
        assert_eq!(parse_class_id("1234"), None);
        assert_eq!(parse_class_id(&"G".repeat(32)), None);
        assert_eq!(parse_class_id(&"é".repeat(16)), None);
    }
}
