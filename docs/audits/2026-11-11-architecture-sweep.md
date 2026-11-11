# Architecture sweep after M9.5 — 2026-11-11

Branch: `m9.5-architecture-sweep` (cut from `origin/main` at `63e11a8c`, after
PR #112). This is the full-repository review the `lumen-review` skill asks for
at the end of a milestone. The M9.5 review of 2026-11-10 swept only that
milestone's diff; this one reads every crate, as the M9 review did. Per the
skill, **nothing found here has been fixed**: the owner decides first.

## Method and honesty about evidence

- **Gates** were run by the parent on this branch (table below).
- **Seven read-only sweeps** ran in parallel, one per crate group or concern
  (real-time crates; project, core and midi; media, audio and tools; plugin
  crates; app logic; app UI and design; docs drift and test hygiene). Each
  followed `lumen-architecture` and `lumen-design`, ran the mechanical scans,
  and read the central files. They ran on Claude Haiku, as
  `.claude/skills/references/delegation.md` asks, and did not run cargo.
- **Verification.** Reviewers over-claim, so the parent re-opened the lines
  behind every finding that mattered. Findings marked **checked** were
  re-read by the parent; the rest are as reported by the sweep (the sweep
  read the cited lines too). Three findings a sweep called High were
  **downgraded** after reading (M5, M6 and M7 below say why). No finding is
  High.
- **Not covered by anyone:** `stretch/chunks.rs`, `lumen-render`'s `unsafe`
  block, the platform `mac.rs` and `com/` modules of the plugin host, the
  bodies of `edit/record*`, `warp`, `bounce`, `freeze`, `takes`, the largest
  UI files read only in outline (`app/view.rs`, `app/palette.rs`,
  `chain/presets.rs`), the pub-visibility audit beyond a spot check, and
  every visual or accessibility check (these need a GPU and a person).

## Gates

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | Clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | Clean |
| `cargo test --workspace --no-fail-fast` (includes the E2E suite) | **4,060 passed, 0 failed**, 134 ignored (look-at renders, hardware and investigation tests, each with a reason) |
| `cargo test -p lumen-app --test smoke -- --ignored` | 13 passed |
| WSL mirror (`clippy.sh`, `perf.sh`) | **Not run:** no WSL distribution on this machine (as on 2026-11-10) |
| macOS cross-target clippy | **Not run:** `lumen-media`'s build script needs a C++ compiler for the Apple target (plan risk 2 of M9.5). CI's macOS job is the check |
| Screenshots, 100/150/200% in both themes | **Not run:** needs a GPU |

## What holds

| Standard | Evidence |
|---|---|
| Audio thread allocation-free | `lumen-engine/tests/realtime.rs` (14 tests under the allocation guard, including `midi_never_touches_the_heap`); `export/live/tests.rs` runs `Tap::process` guarded; fixed pools in `mixer/hosts.rs`, `midi/hold.rs`; plan-time allocations are kept out of render (real-time sweep) |
| Replaced data is freed off the audio thread | `retired` queues with a one-slot stash in `engine.rs`, `mixer/mod.rs`, `input.rs` |
| Never lose work | Format v34; every version v1 to v34 has a frozen `golden.lumen`, each loaded by a test; `history::unchanged` destructures `Project` (15 fields) and `Track` (23 fields) exhaustively so a new field fails to compile; saves are temp file, `sync_all`, rename, `sync_dir`, with tests of failed commits and stale temps (`io.rs`) |
| Every edit is one undo step | `Document::apply` is `#[cfg(test)]` (`document/mod.rs:324`); no production `history.apply` outside `document/`; refused edits record nothing and a gesture that returns to its start is dropped (`history.rs`); a randomised edit test exists |
| Disk work off the UI thread (the main paths) | Saves, settings writer, autosave tick, take worker, file dialogs, template listing, media pool, handover check, ASIO probing in a child process; see M8 and M9 for the exceptions |
| Plugin isolation | Third-party code loads only in `lumen-plugin-host` processes; the scan path has a deadline, a reply cap and a stale-host check; the shared-memory region checks every count, offset and finiteness, and the host scrubs its output; parent death is covered on all three platforms |
| Layering | `tests/layering.rs` table matches every crate's manifest (checked for engine, devices, rt, render, plugin crates) |
| Design tokens | No literal font sizes; one `egui::Window::new` (`widgets/dialog.rs:79`); both `DragValue` uses call `update_while_editing(false)`; `PRODUCT_NAME` / `product_name!()` used in about 40 places |
| Docs | ADRs are contiguous 0001 to 0140; DESIGN.md's 53 colour tokens equal the `Palette` fields; the `clip_grid` values match in both themes; every `@m9.5` tag in `undocumented.txt` is a real command or preference; the timing list has no stale entry; no `#[ignore]` lacks a reason |

## Findings

No High findings. The three a sweep rated High are downgraded below.

### Medium

**M1. A failed backup aborts the save.** `crates/lumen-project/src/io.rs:209-211`
(**checked**). `back_up(...)?` runs before the `fs::rename`. If the Backups
folder cannot be made or the copy fails (full disk, a locked file), the new
content is discarded and the project survives only in memory. Priority 1,
never lose work, says the save should win. The user does get an error, so
nothing is lost silently, which is why this is Medium and not High. No test
covers a failing backup. Fix: make the backup best-effort (report a warning,
then rename) and add the test.

**M2. Two migrations write live defaults.** `crates/lumen-project/src/migrate.rs:38`,
`:42`, `:87` (**checked**). `v1_to_v2` and `v3_to_v4` fill old files with
`TransportState::default()`, `ViewState::default()` and `Grid::default()`.
The file's own rule is that a migration never changes, and `v4_to_v5` and
`v5_to_v6` already use literals. Changing a default later silently changes
how v1 and v3 projects open. The golden expectation also uses `Default`, so
the tests would not notice. Fix: write the literal values, and make
`golden_v1_expected` literal too.

**M3. Rendered files are deleted on the UI thread.** `app/clip_bounce.rs:352`,
`app/track_freeze.rs:323` and `:333`, `app/clip_consolidate.rs:150` call
`remove_files`, a loop of `std::fs::remove_file` (`bounce.rs:538`,
`consolidate.rs:252`) (**checked**). Rule 4. A large WAV on a slow drive
stalls a frame. Fix: use `recording::take_notes::delete_later`, which
`app/audio_record.rs` and `app/recovery.rs` already use.

**M4. More UI-thread disk and registry work.** (a) `app/recovery.rs:128-129`
and `app/startup.rs:312` run `autosave::discard` and the leftover scan
(`read_dir`, lock probes, JSON parse, `autosave.rs:311-333`) on the UI thread.
(b) `app/preferences/files.rs:34` and `app/welcome/ready.rs:89` ask the
registry for the association status every frame (`association/mod.rs:374`,
a mutex plus hive reads); cost not measured. (c) `app/projects.rs:83-88`
`made()` calls `create_dir_all` on the UI thread under a comment that says it
does not. (d) `app/streaming.rs:43` drops `Streaming` on the UI thread, which
joins the disk thread (`disk.rs:184`); the wait is about one 5 ms round.
Rule 4. Fix: a worker for the scan, a cached `Status`, and hand the old
`Streaming` to a thread.

**M5. Audio boundary hardening.** Downgraded from High by the parent.
`crates/lumen-audio`: (a) `asio/mod.rs:587-599` `close()` ignores the result of
`driver.stop()` and then waits three buffer periods before freeing the
callback state. This is a deliberate handshake and nothing shows a callback
arriving late, but a timed sleep is not proof; an in-flight counter would be.
(b) `asio/mod.rs:453-494` returns early from `open_stream` on a failed
`sample_rate()` with the buffers still created, so the driver can report busy
on the next open. (c) `asio/mod.rs:305-311` maps an `init` failure to
`DeviceNotPresent`, so a held driver reads as a missing one and the user never
sees "busy" (rule 5, a way forward). (d) `AsioStream` has no `Drop`. (e) the
`extern "C"` callbacks have no `catch_unwind` (a panic across the boundary
aborts). (f) `ffi.rs:32-39` leaves buffer-size-change and overload
notifications unhandled behind `allow(dead_code)`. (g) `drift.rs:124-167`
documents a clamp of the resample ratio that the code does not apply, and
discards the `set_resample_ratio` result. Consequences are rare but land on
the audio-device path the owner depends on. Fix as a group, with the
Focusrite as the manual check (MT-AUDIO-*).

**M6. `cpal_host.rs` `Render::fill` skips a block under contention.**
Downgraded from High by the parent (**checked**, `cpal_host.rs:272-276`).
`try_lock` never blocks or allocates, so the hard rule holds in letter, but a
control-thread holder turns a block into silence, an audible dropout. This is
the non-ASIO fallback backend. Fix: a lock-free hand-over, as the engine's
other swaps do.

**M7. WASAPI open can panic.** Downgraded from High by the parent (**checked**,
`wasapi.rs:255`). `panic!("the WASAPI render thread died while opening the
device")` is reachable from a user action, though only if the render thread
panics. The `expect` on the thread spawn at `:243` is the same kind, and 26
production `expect("spawn …")` sites (`files.rs`, `autosave.rs`, `disk.rs`,
`media.rs`, `settings/mod.rs`, `midi/files.rs`, `app/media_pool.rs` and more)
make thread-start failure a panic where `app/handover.rs` and `instance/`
already treat it as a value. Rule 5. Fix: return `AudioError::Driver`, and
one `spawn_named` helper that returns `Result`.

**M8. Hosting control calls ignore timeouts.** `crates/lumen-hosting/src/process/run.rs:135-157`
(**checked**). `Set`, `Text`, `Typed` and `Keys` discard a `HostError::Timeout`
(`let _ =` or `.ok()`), so a host that stops answering control calls but has
not exited is not flagged by them, and each call still waits `QUICK` (300 ms)
on the host's control thread. The audio side has its own guard
(`FAILED_AFTER_FRAMES`) and the editor and poll paths report, so the hole is
narrower than "never flagged" but real. Isolation must flag and recover a
hung plugin. Fix: count consecutive timeouts and fail the host past a limit.

**M9. Unversioned persisted side files, and goldens not pinned.**
The take sidecar (`recording/take_worker.rs:176-193`) and the autosave note
(`autosave.rs:63-70`) are persisted with no version field and no fixture.
Compatibility rests on serde defaults. Fix: a version field and a fixture.
Also, goldens are checked against a model written in code and a missing
golden is regenerated from current code (`tests/format.rs:428-429`), so a
golden and its expectation edited together pass CI: pin each golden's hash.

**M10. Files over about 800 production lines, and mixed concerns.** Counted
before the inline tests (the sweeps' counts, **checked** for `lanes.rs` and
`clips.rs`):

| File | Lines | Mixes |
|---|---|---|
| `lumen-app/src/lanes.rs` | 1,274 | pure clip derivation, the `Lanes` import machine, egui file cards |
| `lumen-engine/src/clips.rs` | 980 | UI handle, player, fades, crossfades, page hooks |
| `lumen-engine/src/midi/player.rs` | 947 | hardware `Out` queue, track state, live play |
| `lumen-app/src/lib.rs` | 970 | `App` (132 fields) and a 355-line `ui` |
| `lumen-app/src/icons.rs` | 991 | icon geometry, widgets (`beam`, `toggle`, `button`), track-type drawings |
| `lumen-app/src/widgets/toggle.rs` | 863 | switch, menu rows, segmented, tabs |
| `lumen-app/src/hosting/mod.rs` | 1,014 | `Parameters` (belongs in `parameters.rs`), a 316-line `hear` match |
| `lumen-plugin-host/src/client.rs` | 855 | child lifecycle, reader/writer threads, log, about 40 request wrappers |
| `lumen-plugin-host/src/editor/win.rs` | 955 | window class, frames, z-order policy, key capture |
| `lumen-perf/src/lib.rs` | 817 | scenario setup, measured functions, dispatch |
| `engine.rs` 886, `mixer/plan.rs` 856, `app/view.rs` 905, `app/palette.rs` 880, `app/projects.rs` 824, `chain/presets.rs`, `chain/view.rs`, `theme.rs`, `transport/bar.rs` | | already in KNOWN_ISSUES (2026-10-13) |

Also `app/commands/run.rs` is one 710-line `run` with 326 arms, and the
`lumen-plugin-host/src/bin/lumen-editor-spike/` binaries (804 and 860 lines)
ship in the product crate and duplicate the production window code. Fix: split
by the seams in the table, one file or folder per group; move or delete the
spike when MT-VST-0 closes.

**M11. Layout.** `src/` has 40 loose modules and `lib.rs` declares about 72;
`app/` has 71 flat files (10 `clip_*`, 5 `auto_*`, 3 `audio_*`). Single
concerns are split across three or four places: consolidate and bounce each
sit in `src/`, `document/` and `app/`; welcome, templates and tap tempo in
`src/` and `app/`. The rule is a folder per concern. Proposed grouping, for the
owner's choice: `engine/` (`lanes`, `routing`, `played`, `lane_bake`), `bake/`
(`bounce`, `consolidate`, `crossfades`, `normalize`), `project_files/`
(`files`, `autosave`, `relink`, `templates`), `system/` (`launch`, `brand`,
`owner`, `trace`, `crash`), `design/` (`theme`, `fonts`, `icons`, `motion`,
`light`), `timeline/` (`timeview`, `timefmt`, `position`, `playhead`,
`ruler`, `panels`, `canvas`), and `app/clip/`, `app/auto/`, `app/audio/`.

**M12. `impl App` outside `app/`.** 24 files outside `app/` and `testing/`
(`browser/` 7, `chain/` 9, `note_editor/` 8; **checked** by count), against the
rule that components take arguments and return what they did. `testing/` also
holds 15, in a public module that ships in the release binary, plus four empty
`impl App {}` blocks (`testing/mod.rs:641-665`). Fix: move when each
component is next changed, as the rule says; delete the empty blocks.

**M13. Stale suppressions.** `note_editor/lane/mod.rs:14-17` (**checked**)
is a module-wide `allow(dead_code, reason = "the lane UI is wired in step by
step (M5 slice 4b)")`; M5 is long closed, and a module-wide allow hides every
real dead item. `audio_editor/strip.rs:586` has `too_many_arguments`
(`strip_more`, 8 parameters): use a parameter struct. `lumen-plugin-host/src/lib.rs:19`
and `lumen-test-plugin/src/lib.rs:11` carry a crate-wide
`allow(clippy::unnecessary_cast)` whose reason applies to a few modules.
`lumen-rt/src/lib.rs:33` and `:91` repeat `missing_const_for_thread_local`
without a reason. Fix: delete or narrow, then build with `-D warnings` and
delete what is reported.

**M14. Design tokens: literal strokes and opacities.** 78 `Stroke::new(<number>, …)`
and 103 `gamma_multiply(<number>)` calls in the UI (`app/rulers.rs:84`,
`arrangement/drop.rs:226`, `note_editor/paint.rs:127`, `app/clip_header.rs:376`,
`piano_roll/draw.rs:142`, among them), plus `Duration` literals that equal
`motion` constants (`app/media_pool.rs:475`, `app/audio_record.rs:404,468`,
`app/copies.rs:188`) and unnamed settle timers
(`app/preferences/mod.rs:171,202-205`, `transport/mod.rs:245`,
`preferences/midi.rs:78`). Two `DIMMED` constants disagree
(`arrangement/clips.rs:38` is 0.42, `theme.rs:410` is 0.45) and `UNDER_LANE`,
named as a token in DESIGN.md, is private to `arrangement/clips.rs:41`. Rule:
tokens only. Needs the owner's eye for any value that should change; the
mechanical part (naming existing values, no visual change) is safe.

**M15. Docs drift.** The largest group by count. `CLAUDE.md` §12 and
`PROGRESS.md` contradict the manual-test records and each other:
- §12 says MT-EDIT-2, MT-REC-5, MT-AUDIO-7, MT-FIRSTRUN-1, MT-SYS-1 and
  MT-WARP-1 wait for the owner; the same file and
  `docs/manual-tests/` record them as passed. It says "ADR-0001 to ADR-0124";
  the highest is ADR-0140 (**checked**). "eleven `@m9.5` ids" is twelve.
  "Every slice now carries its docs" is superseded by ADR-0133, which the same
  file states later. About a dozen branches are described as current that are
  merged to `main`.
- `PROGRESS.md`'s M9.5 row and report still list M1, M2 and M4 as open and
  MT-M9.5-1 as waiting; they were fixed (ADR-0140, commits `8ce9e95c`,
  `b3c12dc5`, `2900d660`) and passed (`a6ba488e`). Its heading says
  "Now (2026-11-03)".
- `ARCHITECTURE.md:144` and `:241` say "format v26" (v34 now, **checked**);
  `app/welcome.rs` is a folder; `saved.rs` does not exist; the `lumen-media`
  row omits `take`, `detect`, `slicer`, `repitch`, `stretch/`, `tempo`,
  `export`; about 29 app modules and a dozen modules in other crates are in no
  table; the thread diagram omits about eleven spawned threads (the disk
  thread, take worker, scan worker, export worker, instance listener,
  settings writer, browser file workers and others).
- `KNOWN_ISSUES.md:17` is fixed (the audio editor draws a warped clip through
  the map, `audio_editor/source.rs:162`) and still listed; lines 176-179 are a
  broken table; many rows say "then S7", "S7 or later" or "waits for the
  owner's ear" after S7 was built and MT-WARP-1 to 3 passed; `:86` says
  "Installer (M9)" where PROGRESS puts it in M10; `:133` says "uncommitted".
- `docs/milestones/M9.5.md` top status still says S2 "waits for the owner's
  choices". `docs/manual-tests/M9.5.md` has MT-CLIP-3, -4 and -5 with no
  result (they wait for the owner, which is right, but the plan text should
  say so).
- Stale file names in comments: `lanes.rs:3`, `app/navigate.rs:4`,
  `commands/mod.rs:4`, `welcome.rs:4-5`, `recording/recorder.rs:16`,
  `piano_roll/notes.rs:79`, `app/preferences/pages.rs:2-3`;
  `lumen-engine/src/clips.rs:6` says "(no warping yet)".
- `lumen-project/src/smf/read.rs:27` (**checked**): the user-visible error
  "which Lumen does not read" names the product in a library crate (hard
  rule 8; the effect is one string, so Medium). Fix: neutral wording.

**M16. Allocation-guard coverage is thin outside `realtime.rs`.**
`lumen-engine/tests/{clips,midi,mixer,record,metronome,meters}.rs` render
without `rt_section`, and `lumen-devices/tests/synth.rs` and `hosted/tests.rs`
have no guard, so `realtime.rs` is the only backstop (real-time sweep). Also
`StreamSide::at` (`lumen-engine/src/stream/mod.rs:244-301`) can call a loader
that takes a lock and reads files; only a doc comment says the audio side
never reaches it. Fix: guard the other render tests, and give the audio side
a type with no loader. This is the M4 point of the M9.5 review, generalised.

### Low

- **`shm/region.rs:426-484`** (**checked**): the write pass calls
  `params.queue(i)` a second time after the validation pass, and the `SAFETY`
  comment relies on the safe public trait returning the same data. No
  implementation misbehaves. Fix: copy the lengths, or check `next + len`
  before each write. Also `write_flagged` has `too_many_arguments` (8 with
  `self`): a parameter struct.
- **Plugin-host details:** plugin stderr lines starting `lumen-host: ` are
  accepted as host log lines (`client.rs:163`, `process.rs:17`; a per-host
  nonce fixes it); the Windows crash filter's `SAFETY` comment covers only the
  `WriteFile` calls, not the reads of RIP, the PE header and the stack
  (`process.rs:400-487`); `shm/sys/windows.rs:58-78` does not check
  `ERROR_ALREADY_EXISTS` where Unix uses `O_EXCL` (names are pid plus counter);
  `scan/run.rs:135` and `catalog/src/scan.rs` use `thread::spawn` that
  panics, where `client.rs` uses `Builder::spawn`; `editor/mac.rs:561`
  `allow(deprecated)` has no `reason`; `instance/mod.rs:649` names a vendor's
  helper in a comment (reword to "the VST3 SDK's plug-provider helper").
- **`lumen-project`:** `migrate.rs:167` `highest_id + 1` is unchecked (a crafted
  file only); `automation.rs:59` `Target::is_level` and `model/mod.rs:291`
  `Project::signature_at` are public and unused; the temp name in `io.rs:202`
  is not unique per save (latent: callers serialise); `edit/note_selection.rs`
  has its tests only in `lumen-app`; `testing` and `fake` modules compile into
  release libraries (`lumen-project/src/lib.rs:21`, `lumen-midi/src/lib.rs:13`:
  a `test-support` feature would fix it).
- **`lumen-midi/src/system.rs:352`:** slots are never reused and the activity
  table has 64, so after 64 distinct input port names in one session further
  ports show no activity light (notes still route).
- **Other audio and media:** `asio`/`wasapi` `expect` on thread spawn and close
  (see M7); `stretch/shim.cpp` `presetDefault` leaks the stretcher when the
  preset throws (reported, line not re-opened); `cpal_host.rs:150-155` and
  `lumen-media/src/lib.rs:79-80` carry a stale and an orphaned comment;
  `VENDORED.md` records the stretch header as 1.3.2 and the upstream tag as
  1.4.0 without explanation and omits the linear library's tag.
- **Engine:** `preview.rs:482` and `:292` drop a block on a full ring (frees on
  the audio thread if the documented invariant ever fails: add `debug_assert!`);
  `hosted/params.rs:159-178` `copy_from` can allocate on a zero-capacity
  placeholder (unreachable today); `clips.rs:786-788` carries the doc comment
  of `seams.rs::resolve`.
- **UI:** egui's own buttons in `arrangement/strip_output.rs:215`,
  `app/history.rs:85` and `app/preferences/mod.rs:29`; the tempo field and the
  recording offset use `DragValue` with no double-click reset
  (`transport/bar.rs:575`, `preferences/audio/latency.rs:19`); two welcome
  texts truncate whole sentences where only the path should
  (`app/welcome/plugins.rs:144`, `mod.rs:277`); `app/auto_recording.rs:397`
  `expect("one point")` on a value that a plugin might make non-finite (not
  traced); `meters.rs:5` has a typo; `color_of` (`arrangement/mod.rs:401`) and
  `CLIP_COLORS` (`theme.rs:252`) both unpack a `u32` colour.
- **Tests:** the `warp_transient` investigations measure and assert nothing, and
  KNOWN_ISSUES asks for a re-run after any change that nothing enforces.
- **Dead `pub` items used only in their own file** (the app-logic sweep's list):
  `routing.rs:52`, `played.rs:37,39,138`, `strays.rs:207`, `relink.rs:20`,
  `bounce.rs:45`, `files.rs:203`, `live.rs:178`.

## Recommended order

1. **M1, M2, M9** (persistence): small, test-first, and priority 1. One
   commit each.
2. **M3, M4, M7, M8** (UI thread and error values): a `spawn_named` helper,
   deletes through `delete_later`, a leftover scan on a worker, hosting
   timeouts.
3. **M5, M6** (audio device path): needs the Focusrite to confirm; do after the
   owner picks a date for the manual run. M16 (guards) can go with it.
4. **M13, M14 (naming only), M15 and the Low items:** mechanical, no
   behaviour change; safe to hand to DeepSeek V4.1 Flash delegates with the
   full suite as the check.
5. **M10, M11, M12** (splits and folders): the largest diff for the least
   behaviour; do them one file or folder per commit after the above so a
   regression is easy to place. M11's grouping is a choice for the owner.
6. **M15's docs pass** last, so it describes the code as it ends up.

## Questions for the owner

1. Go-ahead to fix, and in which of the six groups above?
2. M11: which folder grouping, or leave the layout alone this milestone?
3. M14: name the existing stroke and opacity literals as tokens (no visual
   change), or leave them?
4. M5/M6: fix the audio path now, or wait for a Focusrite session?
5. The editor spike binaries (M10): delete now or when MT-VST-0 closes?

## Status

Nothing fixed yet. When the owner approves, this table records each finding,
its fix and its commit.

| Finding | Fix | Commit |
|---|---|---|
