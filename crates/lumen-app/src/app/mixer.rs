//! The mixer's live side (M4 slice 5, ADR-0040): every frame, the levels
//! the engine measured move the meters on, and integrated loudness starts
//! again when playback starts from stop (the owner's B5).

use std::time::Instant;

use lumen_project::routing as rules;

use crate::App;

impl App {
    /// Moves every meter on with what the engine measured since the last
    /// frame. Returns whether a meter is still moving, so the frame loop
    /// keeps drawing until they rest.
    pub(crate) fn update_meters(&mut self) -> bool {
        let now = Instant::now();
        let dt = self
            .meters_at
            .map_or(0.0, |at| now.duration_since(at).as_secs_f32());
        self.meters_at = Some(now);
        let project = self.document.project();
        let ids: Vec<u64> = rules::all_tracks(project).map(|t| t.id.0).collect();
        if std::mem::take(&mut self.transport.played_from_stop) {
            self.arrangement.meters.reset_loudness();
        }
        let running = matches!(
            self.transport.audio.status,
            crate::audio::Status::Running { .. }
        );
        let meters = &mut self.arrangement.meters;
        meters.update(&mut self.transport.audio.mixer, &ids, dt, running);
        self.arrangement.track_colour_meters = self.settings.mixing.track_colour_meters;
        self.arrangement.meters.moving()
    }
}
