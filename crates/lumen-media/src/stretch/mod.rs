//! Time-stretching for the Melodic and Full mix warp modes (M9.5 S4c,
//! ADR-0132): the file played at the song's tempo without changing its pitch.
//!
//! The algorithm is a vendored C++ library (`vendor/`, MIT) behind a C
//! interface of our own (`shim.cpp`). [`Stretcher`] is that interface made
//! safe: one stretcher, fed a hop at a time. [`Chunks`] is what the rest of
//! Lumen uses: it renders a warped stream as a [`repitch::Renderer`] does,
//! frames asked for by index, but from a stretcher that has state, so it
//! renders from a fixed grid of *chunks*, each started from nothing a little
//! ahead of itself. That is what makes a render repeat bit for bit however
//! the audio is asked for (in pages, after a jump, split between clips).
//!
//! Slow work for a background thread: it reads the file and allocates.
//!
//! [`repitch::Renderer`]: crate::repitch::Renderer

mod chunks;

pub use chunks::{CHUNK, Chunks, FADE};

use std::ptr::NonNull;

use crate::MediaError;

/// Which of the library's two settings a mode plays with. Both are tried by
/// ear in the owner's listening test (MT-WARP-1); the names say what each
/// is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Preset {
    /// The library's default: a longer block, the best sound for a full mix.
    Full,
    /// A shorter block and a longer hop: cheaper, for a single melodic line.
    Light,
}

#[repr(C)]
struct Raw {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn lumen_stretch_new(channels: i32, rate: i32, preset: i32) -> *mut Raw;
    fn lumen_stretch_free(s: *mut Raw);
    fn lumen_stretch_block(s: *const Raw) -> i32;
    fn lumen_stretch_interval(s: *const Raw) -> i32;
    fn lumen_stretch_input_latency(s: *const Raw) -> i32;
    fn lumen_stretch_output_latency(s: *const Raw) -> i32;
    fn lumen_stretch_reset(s: *mut Raw) -> i32;
    fn lumen_stretch_process(
        s: *mut Raw,
        input: *const *const f32,
        input_len: i32,
        output: *const *mut f32,
        output_len: i32,
    ) -> i32;
}

/// One stretcher: feed it a run of the file and take a run of the output.
pub struct Stretcher {
    raw: NonNull<Raw>,
    channels: usize,
    rate: u32,
    preset: Preset,
    block: usize,
    interval: usize,
    input_latency: usize,
    output_latency: usize,
}

// SAFETY: the stretcher is plain memory owned by this value, with no thread
// affinity, and every call takes `&mut self`.
unsafe impl Send for Stretcher {}

impl Stretcher {
    /// A stretcher for one or two channels at `rate`.
    pub fn new(channels: usize, rate: u32, preset: Preset) -> Result<Self, MediaError> {
        let failed = || MediaError::Resample("the time-stretcher could not be made".into());
        let code = match preset {
            Preset::Full => 0,
            Preset::Light => 1,
        };
        let signed = i32::try_from(rate).map_err(|_| failed())?;
        // SAFETY: plain values in; the result is checked for null below.
        let raw = unsafe { lumen_stretch_new(channels as i32, signed, code) };
        let raw = NonNull::new(raw).ok_or_else(failed)?;
        // SAFETY: `raw` is a live stretcher made above.
        let (block, interval, input, output) = unsafe {
            (
                lumen_stretch_block(raw.as_ptr()),
                lumen_stretch_interval(raw.as_ptr()),
                lumen_stretch_input_latency(raw.as_ptr()),
                lumen_stretch_output_latency(raw.as_ptr()),
            )
        };
        Ok(Self {
            raw,
            channels,
            rate,
            preset,
            block: block.max(1) as usize,
            interval: interval.max(1) as usize,
            input_latency: input.max(0) as usize,
            output_latency: output.max(0) as usize,
        })
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn preset(&self) -> Preset {
        self.preset
    }

    /// Frames in the analysis block.
    pub fn block(&self) -> usize {
        self.block
    }

    /// Output frames per hop: the stretcher is fed one hop's output at a
    /// time.
    pub fn interval(&self) -> usize {
        self.interval
    }

    /// How far the input it has been given runs ahead of the point it is
    /// playing, in input frames.
    pub fn input_latency(&self) -> usize {
        self.input_latency
    }

    /// How far the output runs behind the point it is playing, in output
    /// frames.
    pub fn output_latency(&self) -> usize {
        self.output_latency
    }

    /// Forgets everything it has heard.
    pub fn reset(&mut self) -> Result<(), MediaError> {
        // SAFETY: `raw` is live, and we hold the only reference.
        let code = unsafe { lumen_stretch_reset(self.raw.as_ptr()) };
        status(code)
    }

    /// Takes `input` (one slice per channel, all one length) and fills
    /// `output` (one slice per channel, all one length, normally
    /// [`interval`](Self::interval) frames). The ratio of the two lengths is
    /// the speed the file plays at.
    pub fn process(
        &mut self,
        input: &[&[f32]],
        output: &mut [&mut [f32]],
    ) -> Result<(), MediaError> {
        let channels = self.channels;
        let (n_in, n_out) = match (input.first(), output.first()) {
            (Some(i), Some(o)) => (i.len(), o.len()),
            _ => return Ok(()),
        };
        if input.len() != channels
            || output.len() != channels
            || input.iter().any(|c| c.len() != n_in)
            || output.iter().any(|c| c.len() != n_out)
        {
            return Err(MediaError::Resample(
                "the time-stretcher was given the wrong shape".into(),
            ));
        }
        let mut ins = [std::ptr::null::<f32>(); 2];
        let mut outs = [std::ptr::null_mut::<f32>(); 2];
        for c in 0..channels {
            ins[c] = input[c].as_ptr();
            outs[c] = output[c].as_mut_ptr();
        }
        // SAFETY: `ins` and `outs` hold one pointer per channel, each to
        // `n_in` readable and `n_out` writable frames (the lengths were
        // checked above); the slices outlive the call and the output slices
        // do not alias the input ones (they are `&mut` against `&`).
        let code = unsafe {
            lumen_stretch_process(
                self.raw.as_ptr(),
                ins.as_ptr(),
                n_in as i32,
                outs.as_ptr(),
                n_out as i32,
            )
        };
        status(code)
    }
}

impl Drop for Stretcher {
    fn drop(&mut self) {
        // SAFETY: `raw` came from `lumen_stretch_new` and is freed once.
        unsafe { lumen_stretch_free(self.raw.as_ptr()) }
    }
}

fn status(code: i32) -> Result<(), MediaError> {
    if code == 0 {
        Ok(())
    } else {
        Err(MediaError::Resample("the time-stretcher failed".into()))
    }
}

#[cfg(test)]
mod tests;
