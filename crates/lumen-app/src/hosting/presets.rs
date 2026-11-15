use super::*;

impl Hosting {
    pub fn programs(&self, device: Id) -> &[ProgramList] {
        self.tracked
            .get(&device.0)
            .map_or(&[], |t| t.params.programs.as_slice())
    }

    pub fn begin_program(&mut self, device: Id, name: String) {
        if let Some(t) = self.tracked.get_mut(&device.0) {
            t.params.program = Some((name, std::time::Instant::now()));
            t.params.program_fresh = true;
        }
    }

    pub fn program_loading(&self, device: Id) -> Option<&str> {
        let (name, since) = self.tracked.get(&device.0)?.params.program.as_ref()?;
        (since.elapsed() < PROGRAM_WINDOW).then_some(name.as_str())
    }

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

    pub fn next_retry(&self) -> Option<std::time::Duration> {
        self.tracked
            .values()
            .filter_map(|t| t.params.retry.as_ref())
            .map(|(at, _)| at.saturating_duration_since(std::time::Instant::now()))
            .min()
    }

    pub fn take_presets(&mut self) -> Vec<PresetNews> {
        std::mem::take(&mut self.presets)
    }

    pub fn synced(&mut self, device: Id, rev: u32) {
        if let Some(t) = self.tracked.get_mut(&device.0) {
            t.params.state_rev = rev;
        }
    }
}
