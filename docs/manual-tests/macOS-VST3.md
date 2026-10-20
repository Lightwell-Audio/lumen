# MT-VST-MAC — Native VST3 workflow

Run from the repository root on macOS. Use a disposable project and isolated
settings; retain the owner's normal settings and recovery files.

```sh
cargo build -p lumen-app -p lumen-plugin-host -p lumen-test-plugin
mkdir -p /tmp/lumen-vst3-manual
cargo run -p lumen-app -- --audio test:fake \
  --settings /tmp/lumen-vst3-manual/settings.json \
  --autosave-dir /tmp/lumen-vst3-manual/autosave \
  --crash-dir /tmp/lumen-vst3-manual/crash
```

The fake device verifies application behavior without changing the owner's
audio device. Audible playback and latency require a separate run using an
explicitly selected real device and the same disposable settings.

1. Open Devices in the browser. Check plugins in both system and user VST3
   folders, a custom folder, search, favorites and rescan. An incompatible
   architecture or failed scan must show a recoverable reason.
2. Add a native instrument to a MIDI track, draw a short part and play.
   Check meter activity; in the real-device run listen for clean sound.
   Add an effect, bypass and remove it, then undo/redo each edit.
3. Open the instrument's native editor. Type in a text field: editing must
   reach the plugin and must not trigger arrangement commands. Enable the
   computer keyboard as MIDI input and check note press/release, including
   changing focus or hiding/closing the editor while a key is held.
4. Resize a resizable editor using its border and its own controls. Check
   content stays aligned. A fixed-size plugin must keep its supported size.
   Move between displays where available; check Retina rendering and focus.
   Close, reopen, select another track and return; check bounds and visibility.
5. Change a native control, check generic parameter controls, then undo/redo.
   Load and save a preset. Save the project, close and reopen it; check sound,
   parameter values, chain order and remembered editor placement.
6. Route an auxiliary instrument output to another track and key an effect
   from a second track. Check independent meters, audio and delay compensation.
7. Quit while editors are open. Check host processes exit. Repeat with a
   deliberately hung test host and a killed disposable parent; no host may
   remain. Check reload and safe mode retain saved plugin state.
8. Repeat the relevant keyboard, layout and native window checks in both
   themes at 100%, 150% and 200% application scale. Check at 64 and 128 audio
   frames on a real device; record rate, hardware, plugin count and dropouts.

## Recorded runs

Pending. Automated probe, unit and end-to-end results are recorded separately
in the slice plan; they do not establish audible hardware playback, native
text entry, every plugin's resize behavior or multiple-display behavior.
