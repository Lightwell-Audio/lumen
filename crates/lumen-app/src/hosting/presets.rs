//! Programs and preset files as the bridge keeps them (M6 slice 5, ADR-0077):
//! the plugin's program lists, loading a state and trying it again.

use super::*;

impl Hosting {
    /// The programs a plugin offers, as it listed them when it loaded.
    pub fn programs(&self, device: Id) -> &[ProgramList] {
        self.tracked
            .get(&device.0)
            .map_or(&[], |t| t.params.programs.as_slice())
    }

    /// The user chose a program: for a moment the edits the plugin makes in
    /// answer (the values the program holds) are one undo step with the
    /// choice, named for it.
    pub fn begin_program(&mut self, device: Id, name: String) {
        if let Some(t) = self.tracked.get_mut(&device.0) {
            t.params.program = Some((name, std::time::Instant::now()));
            t.params.program_fresh = true;
        }
    }

    /// The program being chosen, while the edits it causes still count as
    /// part of the choice.
    pub fn program_loading(&self, device: Id) -> Option<&str> {
        let (name, since) = self.tracked.get(&device.0)?.params.program.as_ref()?;
        (since.elapsed() < PROGRAM_WINDOW).then_some(name.as_str())
    }

    /// Gives a running plugin a preset's state to try. The project is edited
    /// only when the plugin has taken it ([`PresetNews::Ready`]), so a state
    /// the plugin refuses changes nothing. False when the plugin is not running
    /// or is already taking one.
    pub fn try_preset(
        &mut self,
        device: Id,
        name: String,
        component: Vec<u8>,
        controller: Option<Vec<u8>>,
    ) -> bool {
        let (Some(manager), Some(t)) = (self.manager.as_ref(), self.tracked.get_mut(&device.0))
        else {
            return false;
        };
        if t.state != State::Ready
            || t.params.retry.is_some()
            || t.params.loads.iter().any(|l| l.preset.is_some())
        {
            return false;
        }
        manager.load_state(device.0, component.clone(), controller.clone(), Vec::new());
        t.params.loads.push_back(Load {
            state: Captured {
                component,
                controller,
            },
            preset: Some(name),
            tries: 0,
        });
        t.params.quiet_until = Some(std::time::Instant::now() + QUIET);
        true
    }

    /// When a refused preset is given to its plugin again, if one waits.
    pub fn next_retry(&self) -> Option<std::time::Duration> {
        self.tracked
            .values()
            .filter_map(|t| t.params.retry.as_ref())
            .map(|(at, _)| at.saturating_duration_since(std::time::Instant::now()))
            .min()
    }

    /// Presets plugins took or refused since the last call.
    pub fn take_presets(&mut self) -> Vec<PresetNews> {
        std::mem::take(&mut self.presets)
    }

    /// The project now holds the sound the plugin has (a preset it took was
    /// put into the project as revision `rev`): the plugin need not be given
    /// it again.
    pub fn synced(&mut self, device: Id, rev: u32) {
        if let Some(t) = self.tracked.get_mut(&device.0) {
            t.params.state_rev = rev;
        }
    }
}
