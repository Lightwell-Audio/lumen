//! Lumen's side of plugin hosting (M6 slice 2c, ADR-0071). A plugin device
//! runs in a `lumen-plugin-host` process, started and loaded in the
//! background; this crate keeps those processes and gives the engine a
//! [`Link`] to play each through.
//!
//! - [`Link`] is a [`lumen_devices::Hosted`] over the shared-memory audio
//!   path. It runs on the audio thread and does nothing there that could
//!   block, allocate or take a lock.
//! - [`process`] is one device's host and the thread that owns it: the load
//!   sequence with a deadline on every step, then a look at the process every
//!   10 ms, so the callback reads a flag instead of asking the system.
//! - [`Manager`] is the set of devices, polled each frame.
//!
//! Nothing here knows the project, the catalog or the interface: the app's
//! bridge turns devices into [`Start`]s and the engine's `Guard`s.

pub mod link;
pub mod manager;
pub mod process;

pub use link::{Counts, Link, Pair};
pub use lumen_plugin_host::client::HostLog;
pub use lumen_plugin_host::preset::{MAX_FILE, Preset, PresetError, info_xml};
pub use lumen_plugin_host::protocol::{
    Aux, BusInfo, Edit, EditKind, EditorBounds, EditorStatus, MAX_CHANNELS, ParamInfo, ProgramList,
    Taken, UnitInfo,
};
pub use manager::{Event, Manager};
pub use process::{
    CAPTURE, Captured, Deadlines, EditorEvent, Failure, Happened, Linked, Start, Step, WAKE,
};

/// The 32 hexadecimal digits a project stores for a class id, as bytes.
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
