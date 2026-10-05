# Lumen — Plugin Compatibility Matrix

Real third-party plugins tested against Lumen, stage by stage. A plugin is
"compatible" only when every stage passes. Stages beyond scanning arrive with
M6 (VST3 hosting).

**Stages:** Scan (module loads and its factory enumerates in a sandboxed
process) · Instantiate · Process · State round trip · Editor · Automation.
**Legend:** ✅ pass · ❌ fail · — not tested yet

Machine: Windows 11 23H2 (22631), Ryzen 7 7700X. Scan by
`prototypes/vst3-scan` (GNU build), 2026-10-05: 40 modules, 4 parallel scan
processes, 30 s timeout, 10.4 s wall time.

| Plugin | Vendor | Type (sub-categories) | Layout | moduleinfo | Scan | Scan time | Instantiate | Process | State | Editor |
|---|---|---|---|---|---|---|---|---|---|---|
| BBC Symphony Orchestra | Spitfire Audio | Instrument\|Synth | bundle | ✅ matches factory | ✅ | 452 ms | — | — | — | — |
| Melodyne | Celemony | Fx | file | — | ✅ | 475 ms | — | — | — | — |
| FM8 (+ FM8 FX) | Native Instruments | Instrument\|Synth; Fx | file | — | ✅ | 993 ms | — | — | — | — |
| Gateway | Atkinson Advanced Modeling | Fx | bundle (no moduleinfo) | — | ✅ | 133 ms | — | — | — | — |
| MODO BASS 2 | IK Multimedia | Instrument\|Synth | bundle | ✅ matches factory | ✅ | 141 ms | — | — | — | — |
| Kontakt 8 | Native Instruments | Instrument | file | — | ✅ | 1,938 ms | — | — | — | — |
| NAM Rig | Nouratone | Fx\|Fx\|Guitar | bundle | ✅ matches factory | ✅ | 48 ms | — | — | — | — |
| Archetype Plini | Neural DSP | Fx\|Distortion\|Dynamics\|Reverb | file | — | ✅ | 478 ms | — | — | — | — |
| Archetype Rabea X | Neural DSP | Fx\|Fx\|Distortion\|Dynamics\|Reverb | file | — | ✅ | 741 ms | — | — | — | — |
| Archetype Tim Henson X | Neural DSP | Fx\|Fx\|Distortion\|Dynamics\|Reverb | file | — | ✅ | 721 ms | — | — | — | — |
| Fortin Nameless Suite X | Neural DSP | Fx\|Fx\|Distortion\|Dynamics\|Reverb | file | — | ✅ | 418 ms | — | — | — | — |
| Morgan Amps Suite | Neural DSP | Fx\|Fx\|Distortion\|Dynamics\|Reverb | file | — | ✅ | 794 ms | — | — | — | — |
| Omnisphere | Spectrasonics | Instrument\|Synth | file | — | ✅ | 125 ms | — | — | — | — |
| **Opus** | EastWest | — | file | — | ❌ `DllMain` failed (`ERROR_DLL_INIT_FAILED`) after 8.9 s | 8,851 ms | — | — | — | — |
| Podolski | u-he | Instrument | file | — | ✅ | 226 ms | — | — | — | — |
| Toneforge Ben Bruce | Joey Sturgis Tones | Fx | bundle | ✅ matches factory | ✅ | 140 ms | — | — | — | — |
| EZbass | Toontrack | Instrument\|Bass | file | — | ✅ | 569 ms | — | — | — | — |
| Toontrack Audio Sender | Toontrack | Fx\|Tools | file | — | ✅ | 44 ms | — | — | — | — |
| TyrellN6 | u-he | Instrument | file | — | ✅ | 419 ms | — | — | — | — |
| Ozone 12 (main + 20 modules) | iZotope | Fx (EQ, Dynamics, Mastering, Spatial, Distortion) | file | — | ✅ all 21 | 84–145 ms each | — | — | — | — |

## Open items
- **Opus (EastWest):** fails inside its own `DllMain`. Check whether it loads
  in another host on this machine (licensing or authorization state?) before
  treating this as a Lumen issue. M6.
- **Gateway** ships a bundle without `moduleinfo.json`, and
  **BBC Symphony Orchestra** puts it in `Contents/` instead of
  `Contents/Resources/`. Discovery handles both.
