use std::time::{Duration, Instant};

pub const QUIET: Duration = Duration::from_secs(10);

#[derive(Debug, Default)]
pub struct Dropouts {
    count: u64,
    episode: u64,
    last: Option<Instant>,
    reset: bool,
}

impl Dropouts {
    pub fn update(&mut self, count: u64, now: Instant) {
        if count < self.count {
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
        }
        self.count = count;
    }

    pub fn was_reset(&self) -> bool {
        self.reset
    }

    pub fn showing(&self, now: Instant) -> Option<u64> {
        let last = self.last?;
        (now.duration_since(last) < QUIET).then_some(self.episode)
    }

    pub fn hides_in(&self, now: Instant) -> Option<Duration> {
        let last = self.last?;
        QUIET.checked_sub(now.duration_since(last))
    }

    pub fn total(&self) -> u64 {
        self.count
    }
}

pub fn chip(episode: u64) -> String {
    if episode == 1 {
        "Dropout".into()
    } else {
        format!("Dropouts ×{episode}")
    }
}

pub fn summary(dropouts: &Dropouts, now: Instant) -> Option<String> {
    let episode = dropouts.showing(now)?;
    let total = dropouts.total();
    let plural = if episode == 1 { "dropout" } else { "dropouts" };
    let since = if dropouts.was_reset() {
        "since you reset"
    } else {
        "since audio started"
    };
    Some(format!("{episode} {plural} just now · {total} {since}"))
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
        assert_eq!(summary(&d, now), None);
    }

    #[test]
    fn dropouts_show_as_one_episode_until_audio_is_clean_for_10_s() {
        let t0 = Instant::now();
        let mut d = Dropouts::default();
        d.update(1, t0);
        assert_eq!(d.showing(t0).map(chip).as_deref(), Some("Dropout"));
        let t1 = t0 + Duration::from_secs(5);
        d.update(3, t1);
        assert_eq!(d.showing(t1).map(chip).as_deref(), Some("Dropouts ×3"));
        assert_eq!(d.hides_in(t1), Some(QUIET));
        assert_eq!(d.showing(t1 + QUIET), None);
        let t2 = t1 + Duration::from_secs(20);
        d.update(4, t2);
        assert_eq!(d.showing(t2), Some(1));
    }

    #[test]
    fn the_panels_warning_goes_with_the_chip() {
        let t0 = Instant::now();
        let mut d = Dropouts::default();
        d.update(70, t0);
        let t1 = t0 + Duration::from_secs(90);
        d.update(72, t1);
        assert_eq!(
            summary(&d, t1).as_deref(),
            Some("2 dropouts just now · 72 since audio started")
        );
        assert_eq!(summary(&d, t1 + QUIET), None);
    }

    #[test]
    fn a_reset_clears_the_chip_and_says_so() {
        let t0 = Instant::now();
        let mut d = Dropouts::default();
        d.update(5, t0);
        d.update(0, t0);
        assert_eq!(d.showing(t0), None);
        assert_eq!(summary(&d, t0), None);
        let t1 = t0 + Duration::from_secs(1);
        d.update(1, t1);
        assert_eq!(
            summary(&d, t1).as_deref(),
            Some("1 dropout just now · 1 since you reset")
        );
    }
}
