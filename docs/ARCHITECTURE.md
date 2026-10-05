# Lumen — Architecture (draft, end of M1 prototyping)

This is the shape the M1 prototypes support. Each part links to its decision
record; nothing here is accepted until the M1 decisions are approved.

## Processes and threads

```text
┌──────────────────────────── lumen (main process) ─────────────────────────────┐
│                                                                               │
│  UI thread ── egui/eframe (wgpu) ── document model (immutable snapshots)      │
│     │  commands (undoable)        │ snapshots                                 │
│     ▼                             ▼                                           │
│  engine control (non-RT): graph compiler · disk I/O · autosave · scanning     │
│     │  compiled graphs, commands     ▲ retired graphs (freed here)            │
│     ▼  lock-free queues              │                                        │
│  audio driver thread (RT): ASIO / WASAPI / CoreAudio / PipeWire callback      │
│     │  runs the engine directly, no extra buffering                           │
│     ├── RT worker pool (MMCSS "Pro Audio"): parallel graph nodes              │
│     └── per block: signal plugin hosts ─ wait until deadline ─ else silence   │
│                         │ shared memory + events                              │
└─────────────────────────┼─────────────────────────────────────────────────────┘
                          ▼
   lumen-plugin-host processes (kill-on-close Job object, no crash dialogs)
     RT thread: process plugins on shared buffers · GUI thread: plugin editors
   lumen scan processes (short-lived, timeout): load module → factory → info
```

| Concern | Design | Decision | Evidence |
|---|---|---|---|
| UI | egui for chrome, widgets, text and accessibility; GPU pipelines for dense content | ADR-0002 | P-UI |
| Real-time engine | Allocation-free, block-size-invariant core; graphs built and freed off the audio thread | ADR-0003 | P-ENGINE |
| Audio devices | Lumen's own backend layer; native ASIO over COM; driver probing in a child process | ADR-0006 | P-ASIO |
| Plugins | `vst3` crate bindings; sandboxed host processes; out-of-process scanning | ADR-0005 | P-ISO, P-VST3 |
| Projects | Folder + versioned JSON; migrations over untyped JSON; atomic saves | ADR-0004 | P-FORMAT |
| Time | Integer ticks at 1,411,200 PPQ; one rounding rule to samples | ADR-0009 | P-FORMAT |
| Real-time safety | Allocation guard in tests; RealtimeSanitizer on Linux and macOS CI (planned) | ADR-0010 | all RT prototypes |

## Crates

| Crate | Role | Status |
|---|---|---|
| `crates/lumen-core` | Product identity (name defined once), time base | Production seed |
| `crates/lumen-rt` | Real-time safety tooling (`GuardedAllocator`, `rt_section`) | Production seed |
| `crates/lumen-project` | Document model, `.lumen` format, migrations, atomic save | Production seed |
| `prototypes/engine-core` | Real-time core prototype | Rewrite into `lumen-engine` in M2 |
| `prototypes/asio-probe` | Native ASIO host prototype | Becomes the ASIO backend in M2 (after ADR-0006 licensing) |
| `prototypes/isolation-ipc` | Plugin-host transport prototype | Becomes `lumen-plugin-host` transport in M6 |
| `prototypes/vst3-scan`, `vst3-test-module` | Scanner and misbehaving test module | Scanner seeds M6; test module grows into the test plugin |
| `prototypes/ui-stress` | Dense-view rendering prototype | Techniques move into the M3 arrangement |

## Rules every crate follows
- Audio-thread code never allocates, locks, blocks or panics. Tests prove the
  first property; the others are covered by review and, from M2,
  RealtimeSanitizer.
- Third-party native code (plugins, drivers during probing) runs only where a
  crash cannot take Lumen down.
- Lumen never writes a file it cannot read back; every persisted change bumps
  the format version with a migration and a frozen fixture.
- User-visible strings come from `lumen_core::product`.
