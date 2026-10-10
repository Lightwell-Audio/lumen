//! The delay compensation readout (M4 slice 6, the owner's D1, ADR-0041):
//! nothing while no device adds latency; then the audio status ends with
//! "12.4 ms compensation", and hovering it names the longest path in the
//! info line. The audio panel always shows it.

use lumen_engine::Compensation;
use lumen_project::model::Project;
use lumen_project::routing as rules;

/// Milliseconds of `frames` at `rate`.
pub(super) fn ms(frames: u32, rate: u32) -> f64 {
    f64::from(frames) * 1e3 / f64::from(rate.max(1))
}

/// What the audio status adds: `None` while nothing is compensated.
pub(super) fn readout(frames: u32, rate: u32) -> Option<String> {
    (frames > 0).then(|| format!("{:.1} ms compensation", ms(frames, rate)))
}

/// The tracks along the longest path that add latency, first to last
/// ("Kick → Drums → Master").
pub(super) fn path(project: &Project, compensation: &Compensation) -> String {
    compensation
        .path
        .iter()
        .map(|&(id, _)| {
            rules::all_tracks(project)
                .find(|t| t.id.0 == id)
                .map_or("a removed track", |t| t.name.as_str())
        })
        .collect::<Vec<_>>()
        .join(" → ")
}

/// What the info line says over the readout.
pub(super) fn about(project: &Project, compensation: &Compensation, rate: u32) -> String {
    let ms = ms(compensation.frames, rate);
    let path = path(project, compensation);
    if path.is_empty() {
        format!("Delay compensation {ms:.1} ms")
    } else {
        format!("Delay compensation {ms:.1} ms · longest path: {path}")
    }
}

#[cfg(test)]
mod tests {
    use lumen_project::model::TrackKind;

    use super::*;
    use crate::document::Document;

    #[test]
    fn nothing_shows_until_a_device_adds_latency() {
        assert_eq!(readout(0, 48_000), None);
        assert_eq!(
            readout(595, 48_000).as_deref(),
            Some("12.4 ms compensation")
        );
    }

    #[test]
    fn the_info_line_names_the_longest_path() {
        let mut doc = Document::new(crate::empty_project());
        doc.add_track(TrackKind::Audio, None);
        doc.add_track(TrackKind::Audio, None);
        let project = doc.project();
        let (kick, master) = (&project.tracks[1], &project.master);
        let compensation = Compensation {
            frames: 595,
            path: vec![(kick.id.0, 240), (master.id.0, 355)],
        };
        assert_eq!(
            about(project, &compensation, 48_000),
            format!(
                "Delay compensation 12.4 ms · longest path: {} → {}",
                kick.name, master.name
            )
        );
        assert_eq!(
            about(project, &Compensation::default(), 48_000),
            "Delay compensation 0.0 ms"
        );
    }
}
