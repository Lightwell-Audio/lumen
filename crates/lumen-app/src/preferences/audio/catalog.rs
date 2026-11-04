//! The Audio page's device lists and details, loaded in the background so
//! they never block the window (ASIO drivers through `probe.rs`).

use super::*;

pub(super) enum Loaded {
    List(String, DeviceList),
    Details(String, Result<DeviceInfo, String>),
}

/// Device lists per driver and details per device, loaded in the
/// background.
pub struct Catalog {
    lists: HashMap<String, Option<DeviceList>>,
    details: HashMap<String, Option<Result<DeviceInfo, String>>>,
    tx: Sender<Loaded>,
    rx: Receiver<Loaded>,
}

impl Default for Catalog {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            lists: HashMap::new(),
            details: HashMap::new(),
            tx,
            rx,
        }
    }
}

impl Catalog {
    pub(super) fn poll(&mut self) {
        while let Ok(loaded) = self.rx.try_recv() {
            match loaded {
                Loaded::List(driver, list) => {
                    self.lists.insert(driver, Some(list));
                }
                Loaded::Details(id, details) => {
                    self.details.insert(id, Some(details));
                }
            }
        }
    }

    /// Forgets everything (after plugging a device in or out).
    pub(super) fn refresh(&mut self) {
        self.lists.clear();
        self.details.clear();
    }

    pub(super) fn list(&mut self, driver: &str, ctx: &egui::Context) -> Option<&DeviceList> {
        if !self.lists.contains_key(driver) {
            self.lists.insert(driver.to_owned(), None);
            let (tx, ctx, driver) = (self.tx.clone(), ctx.clone(), driver.to_owned());
            std::thread::spawn(move || {
                // WASAPI and the system hosts describe every device as they
                // list them; keep those details too.
                let mut details = Vec::new();
                let list = match audio::backend(&driver) {
                    None => Err(format!("{driver} is not available here")),
                    Some(make) if driver == "asio" => Ok(audio::listed(&driver, make)),
                    Some(make) => {
                        let devices = make().devices();
                        let list = devices
                            .iter()
                            .map(|d| (d.id.clone(), d.name.clone()))
                            .collect();
                        details = devices;
                        Ok(list)
                    }
                };
                for info in details {
                    let _ = tx.send(Loaded::Details(info.id.id.clone(), Ok(info)));
                }
                let _ = tx.send(Loaded::List(driver, list));
                ctx.request_repaint();
            });
        }
        self.lists.get(driver).and_then(Option::as_ref)
    }

    /// Details of device `id`: probed in a helper for ASIO; already known
    /// from the list for the others.
    pub(super) fn details(
        &mut self,
        driver: &str,
        id: &str,
        ctx: &egui::Context,
    ) -> Option<&Result<DeviceInfo, String>> {
        if !self.details.contains_key(id) && driver == "asio" {
            self.details.insert(id.to_owned(), None);
            let (tx, ctx, clsid) = (self.tx.clone(), ctx.clone(), id.to_owned());
            std::thread::spawn(move || {
                let result = crate::probe::probe_asio(&clsid);
                let _ = tx.send(Loaded::Details(clsid, result));
                ctx.request_repaint();
            });
        }
        self.details.get(id).and_then(Option::as_ref)
    }
}
