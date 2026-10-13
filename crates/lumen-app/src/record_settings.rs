//! Recording settings (M5 slice 6c): the count-in, the record-only
//! metronome and the Overdub switch. They live in the app settings file
//! (`settings.record`), not in the project, so they cost no format change.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// How many bars play before a take when Record starts from stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CountIn {
    #[default]
    Off,
    OneBar,
    TwoBars,
    FourBars,
}

impl CountIn {
    pub const ALL: [CountIn; 4] = [
        CountIn::Off,
        CountIn::OneBar,
        CountIn::TwoBars,
        CountIn::FourBars,
    ];

    /// The number of bars the count-in lasts.
    #[allow(dead_code, reason = "the recorder (M5 slice 6c) calls it when wired")]
    pub fn bars(self) -> u32 {
        match self {
            CountIn::Off => 0,
            CountIn::OneBar => 1,
            CountIn::TwoBars => 2,
            CountIn::FourBars => 4,
        }
    }

    /// The beats the count-in lasts in a bar of `numerator` beats.
    #[allow(dead_code, reason = "the recorder (M5 slice 6c) calls it when wired")]
    pub fn beats(self, numerator: u32) -> u32 {
        self.bars().saturating_mul(numerator)
    }

    #[allow(
        dead_code,
        reason = "the settings page (M5 slice 6c) shows it when wired"
    )]
    pub fn name(self) -> &'static str {
        match self {
            CountIn::Off => "Off",
            CountIn::OneBar => "1 bar",
            CountIn::TwoBars => "2 bars",
            CountIn::FourBars => "4 bars",
        }
    }

    /// The name stored in the settings file.
    fn key(self) -> &'static str {
        match self {
            CountIn::Off => "off",
            CountIn::OneBar => "one_bar",
            CountIn::TwoBars => "two_bars",
            CountIn::FourBars => "four_bars",
        }
    }
}

impl Serialize for CountIn {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.key())
    }
}

/// An unknown name reads as Off, so a file written by a later version (or
/// edited by hand) still loads.
impl<'de> Deserialize<'de> for CountIn {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Ok(CountIn::ALL
            .into_iter()
            .find(|count_in| count_in.key() == name)
            .unwrap_or_default())
    }
}

/// The record page of the settings. Every field has a default, so older
/// files read as before (no version change).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordSettings {
    pub count_in: CountIn,
    /// The metronome sounds only while recording and in the count-in, even
    /// when it is otherwise off or on.
    pub only_while_recording: bool,
    /// Overdub merges a take into what is recorded already; off, Replace
    /// overwrites it.
    pub overdub: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_in_beats_follow_the_bars_and_the_numerator() {
        assert_eq!(CountIn::Off.beats(4), 0);
        assert_eq!(CountIn::OneBar.beats(4), 4);
        assert_eq!(CountIn::TwoBars.beats(3), 6);
        assert_eq!(CountIn::FourBars.beats(7), 28);
        assert_eq!(CountIn::FourBars.bars(), 4);
    }

    #[test]
    fn count_in_names_are_as_shown_to_the_user() {
        let names: Vec<_> = CountIn::ALL.into_iter().map(CountIn::name).collect();
        assert_eq!(names, ["Off", "1 bar", "2 bars", "4 bars"]);
    }

    #[test]
    fn count_in_round_trips_through_its_stored_name() {
        for count_in in CountIn::ALL {
            let text = serde_json::to_string(&count_in).unwrap();
            assert_eq!(serde_json::from_str::<CountIn>(&text).unwrap(), count_in);
        }
        assert_eq!(
            serde_json::to_string(&CountIn::TwoBars).unwrap(),
            r#""two_bars""#
        );
    }

    #[test]
    fn an_unknown_count_in_name_reads_as_off() {
        assert_eq!(
            serde_json::from_str::<CountIn>(r#""eight_bars""#).unwrap(),
            CountIn::Off
        );
        assert_eq!(
            serde_json::from_str::<CountIn>(r#""""#).unwrap(),
            CountIn::Off
        );
    }

    #[test]
    fn record_settings_default_to_off_and_replace() {
        let settings = RecordSettings::default();
        assert_eq!(settings.count_in, CountIn::Off);
        assert!(!settings.only_while_recording);
        assert!(!settings.overdub);
    }

    #[test]
    fn missing_record_fields_take_their_defaults() {
        let settings: RecordSettings = serde_json::from_str(r#"{"overdub": true}"#).unwrap();
        assert_eq!(
            settings,
            RecordSettings {
                count_in: CountIn::Off,
                only_while_recording: false,
                overdub: true,
            }
        );
    }
}
