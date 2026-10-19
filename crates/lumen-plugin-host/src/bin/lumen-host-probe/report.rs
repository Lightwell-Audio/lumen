//! What the probe found, as stages with a verdict each, and how it is shown:
//! a line of words, a JSON line for a file, a row of the compatibility table.

use std::fmt::Write as _;

/// How one stage ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Passed.
    Pass,
    /// Passed, with something worth reading in the detail (silence, a state
    /// that changed on restore).
    Note,
    /// The plugin or the host refused or failed the call.
    Fail,
    /// The host process died in this stage.
    Crashed,
    /// The host did not answer by the deadline and was ended.
    TimedOut,
    /// Not run, and why.
    Skipped,
}

impl Verdict {
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Note => "note",
            Verdict::Fail => "fail",
            Verdict::Crashed => "crashed",
            Verdict::TimedOut => "timed out",
            Verdict::Skipped => "skipped",
        }
    }

    /// The mark the compatibility table uses.
    pub fn mark(self) -> &'static str {
        match self {
            Verdict::Pass | Verdict::Note => "✅",
            Verdict::Fail | Verdict::Crashed | Verdict::TimedOut => "❌",
            Verdict::Skipped => "—",
        }
    }

    /// Whether the plugin got through the stage.
    pub fn passed(self) -> bool {
        matches!(self, Verdict::Pass | Verdict::Note)
    }
}

#[derive(Debug, Clone)]
pub struct Stage {
    pub name: &'static str,
    pub verdict: Verdict,
    pub detail: String,
    pub ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Instrument,
    Effect,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Instrument => "instrument",
            Kind::Effect => "effect",
        }
    }
}

/// Everything found about one class of one module.
#[derive(Debug, Clone)]
pub struct Found {
    pub module: String,
    pub name: String,
    pub vendor: String,
    pub subcategories: String,
    pub version: String,
    pub kind: Kind,
    pub stages: Vec<Stage>,
}

impl Found {
    /// The stage names, in the order they run.
    pub const STAGES: [&'static str; 10] = [
        "scan",
        "load",
        "buses",
        "configure",
        "process",
        "link",
        "state",
        "parameters",
        "editor",
        "shutdown",
    ];

    pub fn stage(&self, name: &str) -> Option<&Stage> {
        self.stages.iter().find(|s| s.name == name)
    }

    /// Every stage that ran passed (an editor a platform lacks is skipped,
    /// not failed) and the plugin was shown to process audio.
    pub fn compatible(&self) -> bool {
        self.stages
            .iter()
            .all(|s| s.verdict.passed() || s.verdict == Verdict::Skipped)
            && self.stage("process").is_some_and(|s| s.verdict.passed())
    }

    pub fn lines(&self) -> String {
        let mut text = format!(
            "{} ({}, {}) {}\n",
            self.name,
            self.kind.word(),
            if self.vendor.is_empty() {
                "no vendor"
            } else {
                &self.vendor
            },
            self.version
        );
        for stage in &self.stages {
            let _ = writeln!(
                text,
                "  {:<11} {:<9} {:>6} ms  {}",
                stage.name,
                stage.verdict.word(),
                stage.ms,
                stage.detail
            );
        }
        text
    }

    /// One line of JSON: every field, for a file that tools read.
    pub fn json(&self) -> String {
        let mut text = String::from("{");
        for (key, value) in [
            ("module", &self.module),
            ("name", &self.name),
            ("vendor", &self.vendor),
            ("subcategories", &self.subcategories),
            ("version", &self.version),
        ] {
            let _ = write!(text, "\"{key}\":{},", quote(value));
        }
        let _ = write!(
            text,
            "\"kind\":\"{}\",\"compatible\":{},\"stages\":[",
            self.kind.word(),
            self.compatible()
        );
        for (i, stage) in self.stages.iter().enumerate() {
            if i > 0 {
                text.push(',');
            }
            let _ = write!(
                text,
                "{{\"stage\":\"{}\",\"verdict\":\"{}\",\"ms\":{},\"detail\":{}}}",
                stage.name,
                stage.verdict.word(),
                stage.ms,
                quote(&stage.detail)
            );
        }
        text.push_str("]}");
        text
    }

    /// A row of the compatibility table under `label`: the real name stays
    /// out of the row, as the documentation rule asks.
    pub fn row(&self, label: &str) -> String {
        let mut text = format!("| {label} | {} |", self.kind.word());
        for name in Self::STAGES {
            let mark = self.stage(name).map_or("—", |s| s.verdict.mark());
            let _ = write!(text, " {mark} |");
        }
        text
    }
}

/// The header of the table [`Found::row`] fills.
pub fn table_header() -> String {
    let mut text = String::from("| Plugin | Kind |");
    let mut rule = String::from("|---|---|");
    for name in Found::STAGES {
        let _ = write!(text, " {name} |");
        rule.push_str("---|");
    }
    format!("{text}\n{rule}")
}

/// A JSON string.
pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(verdicts: &[(&'static str, Verdict)]) -> Found {
        Found {
            module: "a\\b.vst3".into(),
            name: "Quote \" name".into(),
            vendor: String::new(),
            subcategories: "Fx".into(),
            version: "1.0".into(),
            kind: Kind::Effect,
            stages: verdicts
                .iter()
                .map(|&(name, verdict)| Stage {
                    name,
                    verdict,
                    detail: "d\n".into(),
                    ms: 3,
                })
                .collect(),
        }
    }

    #[test]
    fn json_escapes_what_a_plugin_can_put_in_its_names() {
        let line = found(&[("process", Verdict::Pass)]).json();
        assert!(line.contains(r#""module":"a\\b.vst3""#), "{line}");
        assert!(line.contains(r#""name":"Quote \" name""#), "{line}");
        assert!(line.contains(r#""detail":"d\n""#), "{line}");
        assert!(!line.contains('\n'));
    }

    #[test]
    fn a_plugin_is_compatible_only_when_every_stage_that_ran_passed() {
        let ok = found(&[
            ("load", Verdict::Pass),
            ("process", Verdict::Note),
            ("editor", Verdict::Skipped),
        ]);
        assert!(ok.compatible());
        let bad = found(&[("load", Verdict::Pass), ("process", Verdict::Crashed)]);
        assert!(!bad.compatible());
        // A plugin that never reached processing has not been shown to work.
        let early = found(&[("load", Verdict::Pass), ("process", Verdict::Skipped)]);
        assert!(!early.compatible());
        let none = found(&[("load", Verdict::Pass)]);
        assert!(!none.compatible());
    }

    #[test]
    fn a_table_row_has_a_cell_for_every_stage() {
        let row = found(&[("load", Verdict::Pass), ("process", Verdict::Fail)]).row("Probe 1");
        assert_eq!(row.matches('|').count(), 3 + Found::STAGES.len());
        assert!(row.starts_with("| Probe 1 | effect |"));
        assert!(!row.contains("Quote"));
    }
}
