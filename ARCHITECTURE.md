# Nexus — System Architecture

## Overview
Nexus is a Windows desktop assistant and phone companion built with **Rust**, **Tauri v2**, and a **Vite 8 + strict TypeScript** frontend (`allowJs: false`, `strict: true`).

## High-Level Module Map

```
nexus/
├── mirror-engine/                 # Rust ADB client, wireless pair/connect, server protocol & stream parser
│   └── src/
│       ├── adb/                   # AdbClient (PATH/bundled adb), WirelessManager (mDNS QR + :5555)
│       ├── protocol/              # ServerParams, MirrorServerManager, VideoStreamReader, ControlMessage
│       └── engine.rs              # ActiveSession (H.264 packet_rx + control_tx)
├── src-tauri/src/
│   ├── main.rs                    # App lifecycle, tray menu, WebView2 low-memory trim, Tauri command registration
│   ├── settings.rs                # Persisted AuraSettings, 4 top-level modules + setup_completed gate
│   ├── engine.rs                  # Voice loop coordinator & idle/sleep teardown
│   ├── voice.rs                   # Audio capture + Chromium Speech v2 client
│   ├── wake_word.rs               # Local offline wake-word matcher
│   ├── executor/                  # Local command parser, timed power actions, window & media controls
│   ├── ai/                        # Optional PC-only Voice AI, DPAPI multi-key vault, dictation, TTS & PowerShell guard
│   ├── modules/
│   │   ├── circle_search.rs       # Dynamic BitBlt capture -> temporary WebView2 overlay -> K32EmptyWorkingSet cleanup
│   │   ├── cs/                    # Capture, OCR (Windows.Media.Ocr), hotkey, pill trigger, translate, music ID
│   │   ├── mirror.rs              # Native Win32 HWND + Direct3D 11 VideoProcessor SwapChain mirror window
│   │   ├── mirror_decoder.rs      # Windows Media Foundation H.264 / AAC / Opus decoders
│   │   ├── mirror_audio.rs        # Low-latency cpal WASAPI audio player with sample-rate resampling
│   │   └── Screen Mirror module   # ADB device list, QR pairing, permanent wireless & live settings sync
│   ├── companion.rs               # Phone Link HTTP router (:41780), on-demand QR pairing, bearer auth, wakeup
│   ├── trackpad/                  # 120Hz UDP (:41781) trackpad & Win32 pointer/key injection
│   ├── gamepad/                   # UDP binary receiver, profiles, Win32 ScanCode engine (gated on companion)
│   ├── remote_terminal.rs         # Opt-in Remote PowerShell endpoint (off by default)
│   ├── clipboard_sync.rs          # Phone-to-PC text/link/image clipboard sync
│   └── island_bridge.rs           # Named pipe IPC (\\.\pipe\inex_island_ipc) to Inex Island
└── ui/
    ├── main.ts                    # Settings shell entrypoint, 4-tab sidebar routing, first-run #setup-modal
    ├── types/                     # Strict TypeScript domain models & Tauri globals
    ├── js/core/                   # State, DOM helpers, modals, hotkeys, updater, tray bridge
    └── js/features/               # Voice, Voice AI, Circle to Search, Phone Link, Gamepad & Screen Mirror
```

## Core Design Principles

1. **Zero Idle Cost for Disabled Modules:**
   - Top-level modules are consolidated into four independently toggleable pillars:
     1. **Voice** (`voice`) — includes the `Voice -> AI & Keys` subsection (`ai_assistant` / `dictation`).
     2. **Circle to Search** (`search`) — destroys its overlay window on close and empties process working sets; idles behind a native Win32 trigger/hotkey when inactive.
     3. **Phone Link** (`companion`) — owns the **Gamepad** (`gamepad`) subsection (`#page-companion-gamepad` with `Overview | Profile HUD | Tuning`). Turning Phone Link off or unpairing immediately suspends the Gamepad receiver and UDP listener.
     4. **Screen Mirror** (`mirror`) — idle until a user connects/starts mirroring or QR pairing.
   - On first launch (`setup_completed == false`), `#setup-modal` presents the 4-module chooser so unchecked modules never allocate audio streams, pollers, or background threads.

2. **WebView2 Low-Memory Tray Residency:**
   - Minimizing or closing Settings (`close_settings`, `WindowEvent::CloseRequested`, or `--silent` boot) hides the window to tray, sets `ICoreWebView2Settings` / `ICoreWebView2_19::SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW)` via `webview2-com`, and calls `SetProcessWorkingSetSize(-1, -1)` after a short settle delay. Restoring the window switches WebView2 memory target back to `NORMAL`.

3. **Frontend Verification & Golden UI:**
   - `tsc --noEmit` enforces zero implicit `any` and unused code checks.
   - `vitest` covers state normalization, module gating, Screen Mirror preference clamping, and hotkey registration helpers.
   - `scripts/golden-ui-harness.mjs` uses Playwright + `pixelmatch` against mocked Tauri IPC payloads to guard layout regressions across all primary views and `#setup-modal`.
