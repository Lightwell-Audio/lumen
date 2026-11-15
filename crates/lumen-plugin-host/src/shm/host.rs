use std::io;
use std::sync::Arc;
#[cfg(not(target_os = "macos"))]
use std::time::Duration;
use std::time::Instant;

use super::region::{Answer, Layout, Region};
use super::sys::{Names, Signal};
use super::{Waiting, Wake};

#[cfg(not(target_os = "macos"))]
const LOOK: Duration = Duration::from_millis(100);

pub enum Next {
    Block(u64),
    Quit,
}

pub struct HostLink {
    region: Arc<Region>,
    go: Arc<Signal>,
    done: Signal,
    wake: Wake,
    seen: u64,
}

#[derive(Clone)]
pub struct Stopper {
    region: Arc<Region>,
    go: Arc<Signal>,
}

impl Stopper {
    pub fn stop(&self) {
        self.region.set_quit();
        self.go.set();
    }
}

impl HostLink {
    pub fn open(base: &str, layout: Layout, wake: Wake) -> io::Result<(Self, Stopper)> {
        let names = Names::new(base);
        let region = Arc::new(Region::open(&names.region, layout)?);
        let go = Arc::new(Signal::open(&names.go)?);
        let done = Signal::open(&names.done)?;
        let stopper = Stopper {
            region: Arc::clone(&region),
            go: Arc::clone(&go),
        };
        Ok((
            Self {
                region,
                go,
                done,
                wake,
                seen: 0,
            },
            stopper,
        ))
    }

    pub fn region(&self) -> &Region {
        &self.region
    }

    pub fn mark_ready(&self) {
        self.region.set_ready();
    }

    pub fn wait_for_block(&mut self) -> Next {
        let entered = Instant::now();
        loop {
            if self.region.quitting() {
                return Next::Quit;
            }
            let request = self.region.request();
            if request != self.seen {
                self.seen = request;
                return Next::Block(request);
            }
            match self.wake.host {
                Waiting::Spin => std::hint::spin_loop(),
                Waiting::Event => {
                    self.wait_for_request();
                }
                Waiting::Hybrid(spin) => {
                    if entered.elapsed() < spin {
                        std::hint::spin_loop();
                    } else {
                        self.wait_for_request();
                    }
                }
            }
        }
    }

    fn wait_for_request(&self) {
        #[cfg(target_os = "macos")]
        {
            if self.go.wait_for_post().is_err() {
                crate::process::log("the worker's semaphore wait failed");
                crate::process::end_now(1);
            }
        }
        #[cfg(not(target_os = "macos"))]
        self.go.wait(LOOK);
    }

    pub fn finish(&self, answer: &Answer, frames: u32, output: &[Vec<f32>]) {
        self.region.finish(answer, frames, output);
        if self.wake.lumen.needs_signal() {
            self.done.set();
        }
    }
}
