use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};

pub(super) const MAX_FILE: u64 = 1 << 20;
pub(super) const MAX_SIDE: u32 = 2048;

pub(super) fn snapshot_path(module: &Path, class: &str) -> Option<PathBuf> {
    if !module.is_dir() {
        return None;
    }
    let folder = module.join("Contents").join("Resources").join("Snapshots");
    let class = class.to_ascii_uppercase();
    [
        format!("{class}_snapshot_2.0x.png"),
        format!("{class}_snapshot.png"),
    ]
    .into_iter()
    .map(|name| folder.join(name))
    .find(|path| path.is_file())
}

pub(crate) fn decode(bytes: &[u8]) -> Option<ColorImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    let (width, height) = (info.width, info.height);
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return None;
    }
    let mut pixels = vec![0u8; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut pixels).ok()?;
    let pixels = &pixels[..frame.buffer_size()];
    let size = [width as usize, height as usize];
    let rgba: Vec<u8> = match frame.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    (rgba.len() == size[0] * size[1] * 4).then(|| ColorImage::from_rgba_unmultiplied(size, &rgba))
}

fn load(path: &Path) -> Option<ColorImage> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_FILE {
        return None;
    }
    decode(&bytes)
}

enum Slot {
    Loading,
    Absent,
    Ready(TextureHandle),
}

type Job = (String, PathBuf);
type Done = (String, Option<ColorImage>);

pub(crate) struct Pictures {
    slots: HashMap<String, Slot>,
    jobs: Sender<Job>,
    done: Receiver<Done>,
}

impl Pictures {
    pub(crate) fn new(wake: impl Fn() + Send + 'static) -> Self {
        let (jobs, requests) = channel::<Job>();
        let (finished, done) = channel::<Done>();
        let spawned = crate::spawn::spawn_named("plugin pictures", move || {
            while let Ok((class, module)) = requests.recv() {
                let image = snapshot_path(&module, &class).and_then(|path| load(&path));
                if finished.send((class, image)).is_err() {
                    break;
                }
                wake();
            }
        });
        if let Err(error) = spawned {
            crate::trace::log(|| {
                format!(
                    "pictures: {}",
                    crate::spawn::start_failed("the picture reader", &error)
                )
            });
        }
        Self {
            slots: HashMap::new(),
            jobs,
            done,
        }
    }

    pub(crate) fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((class, image)) = self.done.try_recv() {
            let slot = match image {
                Some(image) => Slot::Ready(ctx.load_texture(
                    format!("plugin picture {class}"),
                    image,
                    TextureOptions::LINEAR,
                )),
                None => Slot::Absent,
            };
            self.slots.insert(class, slot);
        }
    }

    pub(crate) fn put(&mut self, ctx: &egui::Context, class: &str, image: ColorImage) {
        let texture = ctx.load_texture(
            format!("plugin picture {class}"),
            image,
            TextureOptions::LINEAR,
        );
        self.slots.insert(class.to_owned(), Slot::Ready(texture));
    }

    pub(crate) fn state(&self, class: &str) -> &'static str {
        match self.slots.get(class) {
            None => "none",
            Some(Slot::Loading) => "loading",
            Some(Slot::Absent) => "absent",
            Some(Slot::Ready(_)) => "ready",
        }
    }

    pub(crate) fn get(&mut self, class: &str, module: Option<&Path>) -> Option<TextureHandle> {
        match self.slots.get(class) {
            Some(Slot::Ready(texture)) => return Some(texture.clone()),
            Some(_) => return None,
            None => {}
        }
        match module {
            Some(module) => {
                self.slots.insert(class.to_owned(), Slot::Loading);
                let _ = self.jobs.send((class.to_owned(), module.to_owned()));
            }
            None => {
                self.slots.insert(class.to_owned(), Slot::Absent);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(width: u32, height: u32, color: png::ColorType) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let channels = color.samples();
        writer
            .write_image_data(&vec![200; (width * height) as usize * channels])
            .unwrap();
        writer.finish().unwrap();
        out
    }

    #[test]
    fn colour_grey_and_transparent_pictures_decode_to_pixels() {
        for color in [
            png::ColorType::Rgba,
            png::ColorType::Rgb,
            png::ColorType::Grayscale,
            png::ColorType::GrayscaleAlpha,
        ] {
            let image = decode(&png_bytes(8, 4, color)).expect("a picture");
            assert_eq!(image.size, [8, 4]);
            assert_eq!(image.pixels.len(), 32);
        }
    }

    #[test]
    fn a_picture_that_is_too_large_damaged_or_not_a_picture_is_no_picture() {
        assert!(decode(&png_bytes(MAX_SIDE + 1, 1, png::ColorType::Grayscale)).is_none());
        assert!(decode(b"not a png at all").is_none());
        let mut damaged = png_bytes(8, 8, png::ColorType::Rgba);
        damaged.truncate(damaged.len() / 2);
        assert!(decode(&damaged).is_none());
        assert!(decode(&[]).is_none());
    }

    #[test]
    fn the_double_resolution_picture_is_preferred_and_a_lone_file_has_none() {
        let dir = lumen_project::testing::TempDir::new("pictures");
        let bundle = dir.path().join("Plug.vst3");
        let folder = bundle.join("Contents/Resources/Snapshots");
        std::fs::create_dir_all(&folder).unwrap();
        let class = "0123456789ABCDEF0123456789ABCDEF";
        assert_eq!(snapshot_path(&bundle, class), None);
        std::fs::write(folder.join(format!("{class}_snapshot.png")), b"x").unwrap();
        assert!(
            snapshot_path(&bundle, class)
                .unwrap()
                .ends_with(format!("{class}_snapshot.png"))
        );
        std::fs::write(folder.join(format!("{class}_snapshot_2.0x.png")), b"x").unwrap();
        assert!(
            snapshot_path(&bundle, class)
                .unwrap()
                .ends_with(format!("{class}_snapshot_2.0x.png"))
        );
        assert!(snapshot_path(&bundle, &class.to_ascii_lowercase()).is_some());
        let lone = dir.path().join("Lone.vst3");
        std::fs::write(&lone, b"x").unwrap();
        assert_eq!(snapshot_path(&lone, class), None);
    }

    #[test]
    fn a_file_over_the_limit_is_not_read() {
        let dir = lumen_project::testing::TempDir::new("pictures-big");
        let path = dir.path().join("big.png");
        std::fs::write(&path, vec![0u8; MAX_FILE as usize + 10]).unwrap();
        assert!(load(&path).is_none());
    }
}
