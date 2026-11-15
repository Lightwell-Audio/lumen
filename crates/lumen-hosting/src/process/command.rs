use super::*;

pub(super) enum Command {
    Relink(f64, u32, Option<Aux>),
    Set(u32, f64),
    Text(u32, f64),
    Texts(u32, Vec<f64>),
    Typed(u32, String),
    OpenEditor(Option<EditorBounds>),
    CloseEditor,
    ShowEditor(bool),
    End,
    Capture,
    Refresh,
    Load {
        state: Vec<u8>,
        controller: Option<Vec<u8>>,
        values: Vec<(u32, f64)>,
    },
    Keys(Vec<u8>),
    Quit,
}

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

pub(super) const EDIT_POLL: Duration = Duration::from_millis(25);
pub(super) const QUICK: Duration = Duration::from_millis(300);
pub(super) const EDITOR_POLL: Duration = Duration::from_millis(100);
pub(super) const EDITOR_WAIT: Duration = Duration::from_secs(8);
pub const CAPTURE: Duration = Duration::from_secs(2);
