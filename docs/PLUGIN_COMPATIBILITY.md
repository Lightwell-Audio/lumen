# Lumen — Plugin Compatibility Matrix

Real third-party plugins tested against Lumen, stage by stage. A plugin is
"compatible" only when every stage passes. Stages beyond scanning arrive with
M6 (VST3 hosting).

Third-party product and vendor names are anonymized under the owner's
documentation rule (2026-10-14). Labels identify historical test records,
not Lumen-owned test plugins. Results and measurements are unchanged;
without the original binary identities these records alone cannot support
reproduction or product-specific compatibility claims.

The Lumen-owned deterministic test plugins (`crates/lumen-test-plugin`,
ADR-0065) are not part of this matrix: they test Lumen's host, not
third-party compatibility.

**Stages:** Scan (module loads and its factory enumerates in a sandboxed
process) · Instantiate · Process · State round trip · Editor · Automation.

From M6 slice 7 the stages are produced by `lumen-host-probe`
(`docs/manual-tests/M6.md`, MT-VST-7): scan, load, buses, configure,
process, link, state, parameters, editor and shutdown. The matrix below
still shows the earlier, coarser columns; rows are moved to the probe's
columns as the owner reports each run, under the anonymous labels already
in use. A probe run on the Lumen-owned test plugins is a check of the probe,
not a compatibility record.
**Legend:** ✅ pass · ❌ fail · — not tested yet

Machine: Windows 11 23H2 (22631), Ryzen 7 7700X. Scan by
`prototypes/vst3-scan` (GNU build), 2026-10-05: 40 modules, 4 parallel scan
processes, 30 s timeout, 10.4 s wall time.

| Plugin | Vendor | Type (sub-categories) | Layout | moduleinfo | Scan | Scan time | Instantiate | Process | State | Editor |
|---|---|---|---|---|---|---|---|---|---|---|
| Test instrument A | third-party vendor | Instrument\|Synth | bundle | ✅ matches factory | ✅ | 452 ms | — | — | — | — |
| Test effect A | third-party vendor | Fx | file | — | ✅ | 475 ms | — | — | — | — |
| Test instrument B (+ companion effect) | third-party vendor | Instrument\|Synth; Fx | file | — | ✅ | 993 ms | — | — | — | — |
| Test effect B | third-party vendor | Fx | bundle (no moduleinfo) | — | ✅ | 133 ms | — | — | — | — |
| Test instrument C | third-party vendor | Instrument\|Synth | bundle | ✅ matches factory | ✅ | 141 ms | — | — | — | — |
| Test instrument D | third-party vendor | Instrument | file | — | ✅ | 1,938 ms | — | — | — | — |
| Test effect C | third-party vendor | Fx\|Fx\|Guitar | bundle | ✅ matches factory | ✅ | 48 ms | — | — | — | — |
| Test effect D | third-party vendor | Fx\|Distortion\|Dynamics\|Reverb | file | — | ✅ | 478 ms | — | — | — | — |
| Test effect E | third-party vendor | Fx\|Fx\|Distortion\|Dynamics\|Reverb | file | — | ✅ | 741 ms | — | — | — | — |
| Test effect F | third-party vendor | Fx\|Fx\|Distortion\|Dynamics\|Reverb | file | — | ✅ | 721 ms | — | — | — | — |
| Test effect G | third-party vendor | Fx\|Fx\|Distortion\|Dynamics\|Reverb | file | — | ✅ | 418 ms | — | — | — | — |
| Test effect H | third-party vendor | Fx\|Fx\|Distortion\|Dynamics\|Reverb | file | — | ✅ | 794 ms | — | — | — | — |
| Test instrument E | third-party vendor | Instrument\|Synth | file | — | ✅ | 125 ms | — | — | — | — |
| **Test instrument I** | third-party vendor | — | file | — | ❌ `DllMain` failed (`ERROR_DLL_INIT_FAILED`) after 8.9 s | 8,851 ms | — | — | — | — |
| Test instrument F | third-party vendor | Instrument | file | — | ✅ | 226 ms | — | — | — | — |
| Test effect I | third-party vendor | Fx | bundle | ✅ matches factory | ✅ | 140 ms | — | — | — | — |
| Test instrument G | third-party vendor | Instrument\|Bass | file | — | ✅ | 569 ms | — | — | — | — |
| Test utility A | third-party vendor | Fx\|Tools | file | — | ✅ | 44 ms | — | — | — | — |
| Test instrument H | third-party vendor | Instrument | file | — | ✅ | 419 ms | ✅ in-process and isolated | ✅ 37 µs/block | ✅ round trip identical, incl. restore after a host crash | — |
| Test effect suite A (main + 20 modules) | third-party vendor | Fx (EQ, Dynamics, Mastering, Spatial, Distortion) | file | — | ✅ all 21 | 84–145 ms each | — | — | — | — |

## Open items
- **Test instrument I (third-party vendor):** fails inside its own `DllMain`. Check whether it loads
  in another host on this machine (licensing or authorization state?) before
  treating this as a Lumen issue. M6.
- **Test effect B** ships a bundle without `moduleinfo.json`, and
  **Test instrument A** puts it in `Contents/` instead of
  `Contents/Resources/`. Discovery handles both.
