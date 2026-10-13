"""Writes the hand-made MIDI files in this folder, byte by byte, without
Lumen's writer (M5 slice 7, ADR-0062). Run from this folder:
`python3 make.py`. What each file holds is described below and checked by
`crates/lumen-project/tests/smf.rs`."""


def vlq(n):
    out = [n & 0x7F]
    n >>= 7
    while n:
        out.insert(0, (n & 0x7F) | 0x80)
        n >>= 7
    return bytes(out)


def track(events):
    """events: (delta, raw bytes) pairs; raw may rely on running status."""
    body = b"".join(vlq(d) + raw for d, raw in events) + b"\x00\xff\x2f\x00"
    return b"MTrk" + len(body).to_bytes(4, "big") + body


def header(fmt, ntracks, division):
    return b"MThd" + (6).to_bytes(4, "big") + fmt.to_bytes(2, "big") + ntracks.to_bytes(2, "big") + division


def meta(kind, data):
    return bytes([0xFF, kind]) + vlq(len(data)) + data


# groove-480.mid: type 0 at 480 ticks a quarter, one track named "Groove".
# Tempo 100 BPM, then 140 BPM at quarter 8; 3/4. Channel 1 (0): a bank
# select (MSB 1, LSB 2) and program 5 at 0; notes 60 (quarter 0, one
# quarter), 64 (quarter 1, ended by a zero-velocity note-on under running
# status), 62 struck twice (quarters 2 and 2.5, the second ending the first,
# itself ended at quarter 3); a pitch bend of +4096 at quarter 3; channel
# pressure 90 at quarter 3; polyphonic aftertouch and a sysex (both
# dropped). Channel 10 (9): kick 36 at quarters 0 and 1 (eighth notes,
# running status), and an open hi-hat 46 at quarter 3 never released (it
# ends with the track, at quarter 8, where the tempo changes).
groove = [
    (0, meta(0x03, b"Groove")),
    (0, meta(0x51, (600000).to_bytes(3, "big"))),
    (0, meta(0x58, bytes([3, 2, 24, 8]))),
    (0, bytes([0xB0, 0, 1])),
    (0, bytes([32, 2])),  # running status
    (0, bytes([0xC0, 5])),
    (0, bytes([0x90, 60, 100])),
    (0, bytes([0x99, 36, 120])),
    (240, bytes([0x89, 36, 0])),
    (240, bytes([0x80, 60, 0])),
    (0, bytes([0x90, 64, 90])),
    (0, bytes([0x99, 36, 110])),
    (240, bytes([36, 0])),  # running status, velocity 0: kick off
    (240, bytes([0x90, 64, 0])),  # zero-velocity note-on ends 64
    (0, bytes([62, 80])),  # running status: 62 on at quarter 2
    (0, bytes([0xA0, 62, 50])),  # poly aftertouch, dropped
    (240, bytes([0x90, 62, 70])),  # 62 again at 2.5: ends the first
    (0, bytes([0xF0, 3, 0x7E, 0x7F, 0xF7])),  # sysex, dropped
    (240, bytes([0x80, 62, 64])),
    (0, bytes([0xE0, 0x00, 0x60])),  # bend 0x3000 = +4096
    (0, bytes([0xD0, 90])),
    (0, bytes([0x99, 46, 100])),  # open hi-hat, never released
    (5 * 480, meta(0x51, (428571).to_bytes(3, "big"))),  # 140 BPM at quarter 8
]
open("groove-480.mid", "wb").write(header(0, 1, (480).to_bytes(2, "big")) + track(groove))

# smpte-25.mid: type 1, SMPTE 25 fps at 40 ticks a frame (1000 ticks a
# second). Track 1: no tempo until 60 BPM at 1.0 s. Track 2 ("Lead"): notes
# 72 at 0 s, 74 at 0.5 s, 76 at 2.0 s, each 0.25 s long. At 120 BPM then 60,
# they fall on quarters 0, 1 and 3.
division = bytes([(-25) & 0xFF, 40])
conductor = [(1000, meta(0x51, (1000000).to_bytes(3, "big")))]
lead = [
    (0, meta(0x03, b"Lead")),
    (0, bytes([0x90, 72, 100])),
    (250, bytes([0x80, 72, 0])),
    (250, bytes([0x90, 74, 100])),
    (250, bytes([0x80, 74, 0])),
    (1250, bytes([0x90, 76, 100])),
    (250, bytes([0x80, 76, 0])),
]
open("smpte-25.mid", "wb").write(header(1, 2, division) + track(conductor) + track(lead))

# sequential.mid: type 2 (separate songs), which Lumen refuses.
song = [(0, bytes([0x90, 60, 100])), (96, bytes([0x80, 60, 0]))]
open("sequential.mid", "wb").write(header(2, 2, (96).to_bytes(2, "big")) + track(song) + track(song))
