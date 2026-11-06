"""Checks that every render in the pack lines up with its original in time
and level, and fails if one does not.

Usage: python3 -I check.py <the trial's out dir>

Time: the envelope (RMS over 5 ms hops) of each render against the
original's, stretched to the render's speed; the best lag over +-100 ms must
be within 10 ms. Level: the pack is loudness-matched, so each render's RMS
must be within 0.5 dB of the original's. Peak: no file in the pack may
reach full scale (the pack has one gain per source that leaves 1 dB).
"""
import os
import struct
import sys

import numpy as np

OUT = sys.argv[1]
HOP = 240  # 5 ms at 48 kHz
LAG_LIMIT_MS = 10
LEVEL_LIMIT_DB = 0.5
TRANSFORMS = [
    ("tempo-0.75", 0.75),
    ("tempo-1.5", 1.5),
    ("pitch-minus-5", 1.0),
    ("pitch-plus-3", 1.0),
]


def read(path):
    data = open(path, "rb").read()
    i = data.find(b"data")
    n = struct.unpack("<I", data[i + 4 : i + 8])[0]
    return np.frombuffer(data[i + 8 : i + 8 + n], dtype="<f4").reshape(-1, 2).astype(np.float64)


def envelope(x):
    m = x.mean(1)
    n = len(m) // HOP
    return np.sqrt((m[: n * HOP].reshape(n, HOP) ** 2).mean(1))


def rms_db(x):
    return 20 * np.log10(np.sqrt((x**2).mean()) + 1e-12)


failures = []
for source in sorted(os.listdir(f"{OUT}/pack")):
    original = read(f"{OUT}/pack/{source}/original.wav")
    eo = envelope(original)
    for transform, speed in TRANSFORMS:
        index = np.arange(int(len(eo) / speed)) * speed
        reference = np.interp(index, np.arange(len(eo)), eo)
        for letter in "ABCDE":
            x = read(f"{OUT}/pack/{source}/{transform}/{letter}.wav")
            e = envelope(x)[: len(reference)]
            r = reference[: len(e)]
            lags = range(-20, 21)
            corr = [np.corrcoef(np.roll(e, lag)[25:-25], r[25:-25])[0, 1] for lag in lags]
            lag_ms = lags[int(np.argmax(corr))] * HOP * 1000 // 48_000
            level = rms_db(x) - rms_db(original)
            line = f"{source:11} {transform:14} {letter}: lag {lag_ms:+4d} ms  r {max(corr):.2f}  level {level:+.2f} dB"
            print(line)
            if abs(lag_ms) > LAG_LIMIT_MS or abs(level) > LEVEL_LIMIT_DB:
                failures.append(line)
for root, _, files in os.walk(f"{OUT}/pack"):
    for name in files:
        if name.endswith(".wav"):
            peak = np.abs(read(os.path.join(root, name))).max()
            if peak >= 1.0:
                failures.append(f"{root}/{name}: peak {peak:.2f} reaches full scale")
if failures:
    print("\nOut of bounds:")
    print("\n".join(failures))
    sys.exit(1)
print("\nEvery render is within 10 ms and 0.5 dB of its original, and none clips.")
