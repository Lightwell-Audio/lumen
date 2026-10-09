//! Dropouts as the transport bar tells them (owner's A, 2026-10-09): nothing
//! while audio is healthy; "Dropout" or "Dropouts ×3" in amber when they
//! happen, gone after [`QUIET`] of clean audio; the counts and what they
//! mean in the audio panel.
//!
//! The device layer only counts (`lumen_audio::Status::dropouts`); this
//! turns the count into episodes and a last-minute tally. Kept pure, with
//! the time passed in, so it is tested.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// The chip goes once audio has been clean for this long.
pub const QUIET: Duration = Duration::from_secs(10);
/// The panel's "in the last minute".
const WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Default)]
pub struct Dropouts {
    /// The device's count at the last update.
    count: u64,
    /// When dropouts arrived, and how many, over the last minute.
    recent: VecDeque<(Instant, u64)>,
    /// Dropouts in the current episode: since audio was last clean for
    /// [`QUIET`].
    episode: u64,
    last: Option<Instant>,
    /// The count was reset by the user (rather than counted from start).
    reset: bool,
}

impl Dropouts {
    /// Reads the device's running count at `now`.
    pub fn update(&mut self, count: u64, now: Instant) {
        if count < self.count {
            // Reset in the audio panel.
            self.recent.clear();
            self.episode = 0;
            self.last = None;
            self.reset = true;
        } else if count > self.count {
            let new = count - self.count;
            if self.showing(now).is_none() {
                self.episode = 0;
            }
            self.episode += new;
            self.last = Some(now);
            self.recent.push_back((now, new));
        }
        self.count = count;
        while self
            .recent
            .front()
            .is_some_and(|&(at, _)| now.duration_since(at) > WINDOW)
        {
            self.recent.pop_front();
        }
    }

    /// The user reset the counts.
    pub fn was_reset(&self) -> bool {
        self.reset
    }

    /// Dropouts in the episode the chip shows, while it shows.
    pub fn showing(&self, now: Instant) -> Option<u64> {
        let last = self.last?;
        (now.duration_since(last) < QUIET).then_some(self.episode)
    }

    /// How long until the chip goes, while it shows.
    pub fn hides_in(&self, now: Instant) -> Option<Duration> {
        let last = self.last?;
        QUIET.checked_sub(now.duration_since(last))
    }

    pub fn last_minute(&self) -> u64 {
        self.recent.iter().map(|&(_, n)| n).sum()
    }

    pub fn total(&self) -> u64 {
        self.count
    }
}

/// The chip's words: "Dropout", or "Dropouts ×3".
pub fn chip(episode: u64) -> String {
    if episode == 1 {
        "Dropout".into()
    } else {
        format!("Dropouts ×{episode}")
    }
}

/// The panel's line: "3 dropouts in the last minute · 72 since audio
/// started"; `None` when there have been none.
pub fn summary(dropouts: &Dropouts) -> Option<String> {
    let total = dropouts.total();
    if total == 0 {
        return None;
    }
    let plural = |n: u64| if n == 1 { "dropout" } else { "dropouts" };
    let since = if dropouts.was_reset() {
        "since you reset"
    } else {
        "since audio started"
    };
    let recent = dropouts.last_minute();
    Some(if recent == 0 {
        format!("{total} {} {since}", plural(total))
    } else {
        format!(
            "{recent} {} in the last minute · {total} {since}",
            plural(recent)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_shows_while_audio_is_clean() {
        let now = Instant::now();
        let mut d = Dropouts::default();
        d.update(0, now);
        assert_eq!(d.showing(now), None);
        assert_eq!(summary(&d), None);
    }

    #[test]
    fn dropouts_show_as_one_episode_until_audio_is_clean_for_10_s() {
        let t0 = Instant::now();
        let mut d = Dropouts::default();
        d.update(1, t0);
        assert_eq!(d.showing(t0).map(chip).as_deref(), Some("Dropout"));
        // Two more 5 s later: the same episode.
        let t1 = t0 + Duration::from_secs(5);
        d.update(3, t1);
        assert_eq!(d.showing(t1).map(chip).as_deref(), Some("Dropouts ×3"));
        // 10 s of clean audio: gone.
        assert_eq!(d.hides_in(t1), Some(QUIET));
        assert_eq!(d.showing(t1 + QUIET), None);
        // The next one starts a new episode.
        let t2 = t1 + Duration::from_secs(20);
        d.update(4, t2);
        assert_eq!(d.showing(t2), Some(1));
    }

    #[test]
    fn the_panel_counts_the_last_minute_and_the_total() {
        let t0 = Instant::now();
        let mut d = Dropouts::default();
        d.update(70, t0);
        let t1 = t0 + Duration::from_secs(90);
        d.update(72, t1);
        assert_eq!(
            summary(&d).as_deref(),
            Some("2 dropouts in the last minute · 72 since audio started")
        );
        d.update(72, t1 + Duration::from_secs(61));
        assert_eq!(
            summary(&d).as_deref(),
            Some("72 dropouts since audio started")
        );
    }

    #[test]
    fn a_reset_clears_the_chip_and_says_so() {
        let t0 = Instant::now();
        let mut d = Dropouts::default();
        d.update(5, t0);
        d.update(0, t0);
        assert_eq!(d.showing(t0), None);
        assert_eq!(summary(&d), None);
        d.update(1, t0 + Duration::from_secs(1));
        assert_eq!(
            summary(&d).as_deref(),
            Some("1 dropout in the last minute · 1 since you reset")
        );
    }
}
