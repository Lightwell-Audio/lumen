mod chunks;

pub use chunks::{CHUNK, Chunks, FADE};

use std::ptr::NonNull;

use crate::MediaError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Preset {
    Full,
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
    fn lumen_stretch_set_transpose(s: *mut Raw, semitones: f32) -> i32;
    fn lumen_stretch_reset(s: *mut Raw) -> i32;
    fn lumen_stretch_process(
        s: *mut Raw,
        input: *const *const f32,
        input_len: i32,
        output: *const *mut f32,
        output_len: i32,
    ) -> i32;
}

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

unsafe impl Send for Stretcher {}

impl Stretcher {
    pub fn new(channels: usize, rate: u32, preset: Preset) -> Result<Self, MediaError> {
        let failed = || MediaError::Resample("the time-stretcher could not be made".into());
        let code = match preset {
            Preset::Full => 0,
            Preset::Light => 1,
        };
        let signed = i32::try_from(rate).map_err(|_| failed())?;
        let raw = unsafe { lumen_stretch_new(channels as i32, signed, code) };
        let raw = NonNull::new(raw).ok_or_else(failed)?;
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

    pub fn block(&self) -> usize {
        self.block
    }

    pub fn interval(&self) -> usize {
        self.interval
    }

    pub fn input_latency(&self) -> usize {
        self.input_latency
    }

    pub fn output_latency(&self) -> usize {
        self.output_latency
    }

    pub fn set_transpose(&mut self, cents: f64) -> Result<(), MediaError> {
        let code =
            unsafe { lumen_stretch_set_transpose(self.raw.as_ptr(), (cents / 100.0) as f32) };
        status(code)
    }

    pub fn reset(&mut self) -> Result<(), MediaError> {
        let code = unsafe { lumen_stretch_reset(self.raw.as_ptr()) };
        status(code)
    }

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
