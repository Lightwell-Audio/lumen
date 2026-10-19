//! What a device's thread is asked to do, and how the asks are tidied.

use super::*;

pub(super) enum Command {
    /// A fresh link, at a rate and a longest block, for a new engine or a
    /// new device block size, and with these auxiliary buses on when given
    /// (otherwise the ones it had).
    Relink(f64, u32, Option<Aux>),
    /// Set a parameter (normalized): the plugin's controller is told and the
    /// processor hears it at the start of its next block.
    Set(u32, f64),
    /// Ask the plugin for its text for a value.
    Text(u32, f64),
    /// Ask the plugin for its text for each of these values of one parameter
    /// (the steps of a list): every one is answered, none is dropped for a
    /// later one.
    Texts(u32, Vec<f64>),
    /// Ask the plugin to read text the user typed for a parameter.
    Typed(u32, String),
    /// Open the editor window where it was, or where the system puts it.
    OpenEditor(Option<EditorBounds>),
    CloseEditor,
    /// Hide the open editor window without closing it, or show it again.
    ShowEditor(bool),
    /// End the host, as the user chose for a plugin whose window stopped
    /// answering.
    End,
    /// Read the plugin's state now.
    Capture,
    /// Read every parameter's value now.
    Refresh,
    /// Load a state into the plugin (a preset, or the project's state after
    /// an undo): the component's, the controller's when given, then these
    /// parameter values over them.
    Load {
        state: Vec<u8>,
        controller: Option<Vec<u8>>,
        values: Vec<(u32, f64)>,
    },
    /// Take these keys from the plugin's window (none: give them back).
    Keys(Vec<u8>),
    Quit,
}

/// Drops the commands a later one makes pointless: of several values set for
/// one parameter, only the last matters (a drag sends many); of several asks
/// for one parameter's text, the last; of several sets of keys, the last.
/// Order is otherwise kept.
pub(super) fn coalesce(commands: Vec<Command>) -> Vec<Command> {
    let mut keep = vec![true; commands.len()];
    for (i, c) in commands.iter().enumerate() {
        let later = commands[i + 1..].iter();
        let superseded = match c {
            Command::Set(id, _) => later
                .into_iter()
                .any(|d| matches!(d, Command::Set(other, _) if other == id)),
            Command::Text(id, _) => later
                .into_iter()
                .any(|d| matches!(d, Command::Text(other, _) if other == id)),
            // The words for a stepped parameter's steps, asked again: the
            // later ask has the same steps.
            Command::Texts(id, _) => later
                .into_iter()
                .any(|d| matches!(d, Command::Texts(other, _) if other == id)),
            Command::Keys(_) => later.into_iter().any(|d| matches!(d, Command::Keys(_))),
            Command::Refresh => later.into_iter().any(|d| matches!(d, Command::Refresh)),
            Command::ShowEditor(_) => later
                .into_iter()
                .any(|d| matches!(d, Command::ShowEditor(_) | Command::CloseEditor)),
            _ => false,
        };
        keep[i] = !superseded;
    }
    commands
        .into_iter()
        .zip(keep)
        .filter_map(|(c, k)| k.then_some(c))
        .collect()
}

/// How often a running host is asked for the edits made inside it.
pub(super) const EDIT_POLL: Duration = Duration::from_millis(25);
/// How long an ask for edits or text may take; a miss is only a miss (the
/// audio side finds a host that has stopped answering).
pub(super) const QUICK: Duration = Duration::from_millis(300);
/// How often an open editor window is asked where it is.
pub(super) const EDITOR_POLL: Duration = Duration::from_millis(100);
/// How long a request about the editor window may take. The host answers
/// "not responding" after its own wait (five seconds:
/// `lumen_plugin_host::server::EDITOR_WAIT`), so this only guards a host that
/// is gone.
pub(super) const EDITOR_WAIT: Duration = Duration::from_secs(8);
/// How long reading a plugin's state may take before the answer is a failure
/// and the project is saved with the state it has (M6 slice 4g).
pub const CAPTURE: Duration = Duration::from_secs(2);
