//! Percentiles over a run's timings.

/// A sorted copy of the samples, in nanoseconds.
pub struct Sorted(Vec<u32>);

impl Sorted {
    pub fn of(samples: &[u32]) -> Self {
        let mut v = samples.to_vec();
        v.sort_unstable();
        Self(v)
    }

    /// The `p` quantile (0 to 1) in microseconds.
    pub fn at(&self, p: f64) -> f64 {
        if self.0.is_empty() {
            return 0.0;
        }
        let i = ((self.0.len() - 1) as f64 * p).round() as usize;
        f64::from(self.0[i]) / 1e3
    }

    pub fn line(&self) -> String {
        format!(
            "p50 {:.0} / p99 {:.0} / p99.9 {:.0} / max {:.0} µs",
            self.at(0.5),
            self.at(0.99),
            self.at(0.999),
            self.at(1.0)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_pick_ranked_samples() {
        let s = Sorted::of(&(1..=1000).map(|n| n * 1000).collect::<Vec<u32>>());
        assert_eq!(s.at(0.0), 1.0);
        assert_eq!(s.at(1.0), 1000.0);
        assert_eq!(s.at(0.5), 501.0);
        assert_eq!(s.at(0.99), 990.0);
        assert_eq!(Sorted::of(&[]).at(0.5), 0.0);
    }
}
