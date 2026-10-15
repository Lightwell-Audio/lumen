//! The harness's command line.

use std::path::PathBuf;
use std::time::Duration;

use lumen_plugin_host::shm::{Waiting, Wake};

pub const HELP: &str = "\
lumen-host-bench: measures the audio path to plugin-host processes.

Every scenario is the cross product of the lists below. Lists are comma
separated. Each scenario starts the hosts, attaches them, runs paced blocks
the way an audio callback would, and reports the Lumen-side time per block
(submit to every answer in), deadline misses, stale answers, heap operations
in the loop, and what the hosts cost while idle.

  --plugin PATH      the VST3 module (default: the test plugin beside this exe)
  --host PATH        the host executable (default: lumen-plugin-host beside this exe)
  --hosts LIST       number of host processes        (default 1,8,16)
  --frames LIST      block sizes                     (default 64,128,512)
  --rate LIST        sample rates                    (default 48000)
  --wake LIST        wake-ups, HOST/LUMEN or one word for both. Each of
                     spin, event, hybrid<MICROSECONDS> (hybrid150).
                     (default event,spin,hybrid150,event/spin,hybrid150/spin)
  --load LIST        microseconds of busy work the test plugin does per block
                     (default 0,200)
  --contention LIST  busy threads competing for the cores   (default 0)
  --seconds N        measured seconds per scenario          (default 10)
  --warmup N         blocks discarded first                 (default 500)
  --idle N           seconds with hosts attached and no blocks, for idle CPU
                     (default 2)
  --cold N           seconds idle before the first block; the first 500 blocks
                     are then reported apart            (default 0)
  --deadline F       fraction of the period Lumen waits before using silence
                     (default 0.75)
  --fault KIND       crash or hang in host 0 after the warm-up (test plugin only)
  --csv PATH         also write one line per scenario
  --quick            one short scenario, for checking the setup
  --help
";

#[derive(Debug, Clone)]
pub struct Options {
    pub plugin: Option<PathBuf>,
    pub host: Option<PathBuf>,
    pub hosts: Vec<usize>,
    pub frames: Vec<u32>,
    pub rates: Vec<f64>,
    pub wakes: Vec<(String, Wake)>,
    pub loads: Vec<u64>,
    pub contention: Vec<usize>,
    pub seconds: f64,
    pub warmup: usize,
    pub idle: Duration,
    pub cold: Duration,
    pub deadline: f64,
    pub fault: Option<Fault>,
    pub csv: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    Crash,
    Hang,
}

fn list<T: std::str::FromStr>(name: &str, text: &str) -> Result<Vec<T>, String> {
    let items: Result<Vec<T>, _> = text.split(',').map(|s| s.trim().parse::<T>()).collect();
    match items {
        Ok(v) if !v.is_empty() => Ok(v),
        _ => Err(format!("{name}: `{text}` is not a list of numbers")),
    }
}

fn waiting(text: &str) -> Option<Waiting> {
    match text {
        "spin" => Some(Waiting::Spin),
        "event" => Some(Waiting::Event),
        _ => text
            .strip_prefix("hybrid")
            .and_then(|us| us.parse::<u64>().ok())
            .map(|us| Waiting::Hybrid(Duration::from_micros(us))),
    }
}

pub fn wake(text: &str) -> Result<(String, Wake), String> {
    let (host, lumen) = text.split_once('/').unwrap_or((text, text));
    match (waiting(host.trim()), waiting(lumen.trim())) {
        (Some(host), Some(lumen)) => Ok((text.trim().to_string(), Wake { host, lumen })),
        _ => Err(format!(
            "--wake: `{text}` is not spin, event, hybrid<us>, or HOST/LUMEN of those"
        )),
    }
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Option<Self>, String> {
        let mut o = Self {
            plugin: None,
            host: None,
            hosts: vec![1, 8, 16],
            frames: vec![64, 128, 512],
            rates: vec![48_000.0],
            wakes: Vec::new(),
            loads: vec![0, 200],
            contention: vec![0],
            seconds: 10.0,
            warmup: 500,
            idle: Duration::from_secs(2),
            cold: Duration::ZERO,
            deadline: 0.75,
            fault: None,
            csv: None,
        };
        let mut wakes: Option<String> = None;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
            match arg.as_str() {
                "--help" | "-h" => return Ok(None),
                "--quick" => {
                    o.hosts = vec![2];
                    o.frames = vec![128];
                    o.loads = vec![0];
                    o.seconds = 2.0;
                    o.warmup = 100;
                    o.idle = Duration::from_millis(500);
                    wakes = Some("event,spin".into());
                }
                "--plugin" => o.plugin = Some(value("--plugin")?.into()),
                "--host" => o.host = Some(value("--host")?.into()),
                "--csv" => o.csv = Some(value("--csv")?.into()),
                "--hosts" => o.hosts = list("--hosts", &value("--hosts")?)?,
                "--frames" => o.frames = list("--frames", &value("--frames")?)?,
                "--rate" => o.rates = list("--rate", &value("--rate")?)?,
                "--load" => o.loads = list("--load", &value("--load")?)?,
                "--contention" => o.contention = list("--contention", &value("--contention")?)?,
                "--wake" => wakes = Some(value("--wake")?),
                "--seconds" => o.seconds = list::<f64>("--seconds", &value("--seconds")?)?[0],
                "--warmup" => o.warmup = list::<usize>("--warmup", &value("--warmup")?)?[0],
                "--idle" => {
                    o.idle = Duration::from_secs_f64(list::<f64>("--idle", &value("--idle")?)?[0]);
                }
                "--cold" => {
                    o.cold = Duration::from_secs_f64(list::<f64>("--cold", &value("--cold")?)?[0]);
                }
                "--deadline" => o.deadline = list::<f64>("--deadline", &value("--deadline")?)?[0],
                "--fault" => {
                    o.fault = Some(match value("--fault")?.as_str() {
                        "crash" => Fault::Crash,
                        "hang" => Fault::Hang,
                        other => return Err(format!("--fault: `{other}` is not crash or hang")),
                    });
                }
                other => return Err(format!("unknown option `{other}` (see --help)")),
            }
        }
        let wakes =
            wakes.unwrap_or_else(|| "event,spin,hybrid150,event/spin,hybrid150/spin".into());
        o.wakes = wakes.split(',').map(wake).collect::<Result<_, _>>()?;
        if !(0.05..=1.0).contains(&o.deadline) {
            return Err("--deadline must be between 0.05 and 1".into());
        }
        if o.hosts.contains(&0) || o.frames.contains(&0) || o.seconds <= 0.0 {
            return Err("hosts, frames and seconds must be above zero".into());
        }
        if o.frames.iter().any(|&f| f > 4096) {
            return Err("--frames above 4096 are not supported here".into());
        }
        Ok(Some(o))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Option<Options>, String> {
        Options::parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults_cover_the_planned_grid() {
        let o = parse(&[]).unwrap().unwrap();
        assert_eq!(o.hosts, [1, 8, 16]);
        assert_eq!(o.frames, [64, 128, 512]);
        assert_eq!(o.wakes.len(), 5);
    }

    #[test]
    fn wakes_name_each_side() {
        assert_eq!(wake("event").unwrap().1, Wake::EVENT);
        assert_eq!(wake("spin").unwrap().1, Wake::SPIN);
        let w = wake("event/hybrid150").unwrap().1;
        assert_eq!(w.host, Waiting::Event);
        assert_eq!(w.lumen, Waiting::Hybrid(Duration::from_micros(150)));
        assert!(wake("sleepy").is_err());
        assert!(wake("event/").is_err());
    }

    #[test]
    fn bad_input_is_reported_not_ignored() {
        assert!(parse(&["--hosts", "x"]).is_err());
        assert!(parse(&["--nope"]).is_err());
        assert!(parse(&["--hosts"]).is_err());
        assert!(parse(&["--deadline", "2"]).is_err());
        assert!(parse(&["--fault", "melt"]).is_err());
        assert!(parse(&["--help"]).unwrap().is_none());
    }
}
