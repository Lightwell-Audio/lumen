// A C interface to the vendored time-stretch library (M9.5 S4c, ADR-0132).
//
// The library is C++ and header-only (vendor/signalsmith-stretch, MIT, with
// the FFT it uses in vendor/signalsmith-linear, MIT; see vendor/VENDORED.md).
// This file is all of Lumen's C++: it makes the stretcher with a fixed seed
// (so a render repeats bit for bit), and lets no exception cross into Rust.

#include "signalsmith-stretch/signalsmith-stretch.h"

#include <exception>

namespace {
using Stretch = signalsmith::stretch::SignalsmithStretch<float>;
}

struct LumenStretch {
	explicit LumenStretch(long seed) : stretch(seed) {}
	Stretch stretch;
};

extern "C" {

// A stretcher for `channels` channels at `rate`. `preset` 0 is the library's
// default, 1 its cheaper one. Null if it could not be made.
LumenStretch *lumen_stretch_new(int channels, int rate, int preset) {
	try {
		if (channels < 1 || channels > 2 || rate < 8000) return nullptr;
		auto *s = new LumenStretch(0x4C554D454E); // "LUMEN"
		if (preset == 1) {
			s->stretch.presetCheaper(channels, float(rate), false);
		} else {
			s->stretch.presetDefault(channels, float(rate), false);
		}
		return s;
	} catch (...) {
		return nullptr;
	}
}

void lumen_stretch_free(LumenStretch *s) {
	delete s;
}

int lumen_stretch_block(const LumenStretch *s) { return s->stretch.blockSamples(); }
int lumen_stretch_interval(const LumenStretch *s) { return s->stretch.intervalSamples(); }
int lumen_stretch_input_latency(const LumenStretch *s) { return s->stretch.inputLatency(); }
int lumen_stretch_output_latency(const LumenStretch *s) { return s->stretch.outputLatency(); }

// Shifts the pitch by `semitones` without changing the speed (M9.5 S6). Set
// before the first hop, so every render starts from the same setting. 0 on
// success.
int lumen_stretch_set_transpose(LumenStretch *s, float semitones) {
	try {
		s->stretch.setTransposeSemitones(semitones);
		return 0;
	} catch (...) {
		return 1;
	}
}

// Forgets everything it has heard, as new.
int lumen_stretch_reset(LumenStretch *s) {
	try {
		s->stretch.reset();
		return 0;
	} catch (...) {
		return 1;
	}
}

// Feeds `in_len` frames of input per channel and returns `out_len` frames of
// output per channel. Planar: `in[c]` and `out[c]` are channel c. 0 on
// success.
int lumen_stretch_process(LumenStretch *s, const float *const *in, int in_len,
		float *const *out, int out_len) {
	try {
		s->stretch.process(in, in_len, out, out_len);
		return 0;
	} catch (...) {
		return 1;
	}
}

} // extern "C"
