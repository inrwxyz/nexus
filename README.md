# Nexus

<div align="center">
  <img src="assets/logo-512-clean.png" width="96" height="96" alt="Nexus Logo" />
  <p>Lightweight Windows desktop assistant and phone companion built with Rust, Tauri v2, and Kotlin.</p>
</div>

---

## Overview

Nexus is a low-resource Windows desktop companion offering ambient voice automation, an offline Circle-to-Search overlay, screen mirroring, phone clipboard sync, and a low-latency wireless trackpad and game controller.

## Features

- **Voice Automation**:
  - Fast speech-to-text via Google's **unofficial Chromium Speech Recognition v2** endpoint + offline wake-word detection (`Hey Google`, `Hey Inex`, `Hey PC`, `Alexa`, `Jarvis`).
  - Media controls for Spotify and YouTube via Windows SMTC (play, pause, next, previous).
  - System volume, per-app mixer, screen brightness, screenshot capture, window controls, and app launching.
  - Timed power actions (`Shutdown in 10 minutes`, `Restart in 5 minutes`, `Cancel Shutdown`; default 5s grace timer).
  - **Optional Voice AI, Multi-Key Vault & Dictation (`Voice -> AI & Keys`)** *(PC-only, off by default)*:
    - Fast local voice commands and STT always run first; AI activates only as fallback for casual chat, failure explanations, screen OCR/open-window context, and smart dictation (`"type ..."` / `"smart type ..."`).
    - Windows DPAPI-encrypted Multi-API-Key Vault (`Gemini`, `Groq`, `OpenRouter`, `OpenAI`, `Claude`) with immediate key masking and automatic `429`/`503` failover.
    - Natural Voice Talkback (Google natural speech or Windows OneCore male voices `George`/`Ravi`) synced with Inex Island's voice bars.
    - Optional on-demand Local LLM setup (`Ollama` + downloaded model under a separate switch; zero bundled weights) and strict opt-in PowerShell diagnostics/proposals (destructive commands blocked).
  - **Speech Recognition & Privacy Notice:** Speech-to-text uses Google's **unofficial/experimental Chromium v2 speech endpoint** (with **no official guarantee or SLA**), and Voice Talkback uses Google TTS when set to Natural. When the Voice module is enabled, audio after wake-word detection is sent to Google's servers for transcription. If you prefer not to transmit voice audio externally, leave Voice disabled or mute the microphone at any time.
- **Phone Link (Nexus Companion)**:
  - Zero-latency 120Hz UDP trackpad (starts **Off** by default on launch; lock/unlock mode included) and low-latency PC-to-phone audio streaming.
  - Background Phone Clipboard Sync (text, links, and screenshots).
  - Remote power controls (Wake-on-LAN, Sleep, Lock, Restart, and confirmation-gated Shut Down) + QR pairing with instant unpair sync.
- **Game Controller (`Profile HUD`)**:
  - 3 built-in controller profiles (`Action / Shooter`, `Racing Wheel Pro` with Gyro tilt steering & pedals, and `Xbox Pro Gamepad`) plus custom layouts edited on phone and synced with PC.
  - Foreground game window auto-detection and automatic suspension whenever Phone Link is unpaired or turned off.
  - Full black OLED layout, open confirmation dialog, and lock mode.
- **Opt-In Remote Terminal (Security Policy)**:
  - **Disabled by default.** Enabling **Remote Terminal Access** on PC requires reading and explicitly checking the security policy dialog.
  - Grants the paired phone direct Windows PowerShell execution in the Command tab (only shown when enabled on PC; pressing Back or Exit closes the session immediately).
  - **User Responsibility Policy:** This tool is provided strictly as-is with **no warranty or guarantee**. Enabling or running commands via Remote Terminal can modify or delete files or system state; all usage and consequences are solely the user's responsibility.
- **Circle to Search**:
  - Offline screen region OCR, in-place Google Translate, Visual Lens search, and WASAPI music recognition.
- **Inex Island Integration & Screen Mirror**:
  - Named pipe IPC (`\\.\pipe\inex_island_ipc`) for live status & timers, plus native **Nexus Screen Mirror** (wired USB, QR pairing, and persistent `:5555` wireless ADB streaming decoded via Windows Media Foundation + Direct3D 11 native window with `cpal` WASAPI low-latency audio and live screen-off/audio controls).

## Project Structure

```
nexus/
├── mirror-engine/           # Native Rust ADB client, wireless manager & device mirror stream
├── src-tauri/src/           # Rust backend & Tauri v2 modules
│   ├── ai/                  # Standalone Voice AI, DPAPI key vault, dictation, TTS & guard
│   ├── executor/            # Command parser, timers, window & media actions
│   ├── gamepad/             # UDP binary receiver, profiles, Win32 ScanCode engine (under Phone Link)
│   ├── trackpad/            # UDP trackpad & mouse/keyboard injection
│   ├── pill/                # Ambient floating status pill & Win32 renderer
│   ├── modules/             # Circle to Search (cs/) & native D3D11/MF Screen Mirror (mirror*)
│   ├── clipboard_sync.rs    # Phone-to-PC text/link/image clipboard sync
│   ├── remote_terminal.rs   # Opt-in gated PowerShell companion endpoint
│   └── companion.rs         # Pairing, auth, audio & companion HTTP router
├── NexusCompanion/app/      # Kotlin + Compose Android companion app
├── styles-parts/            # Base theme & layout CSS modules
└── ui/                      # Vite 8 + Strict TypeScript desktop settings & overlay UI (ui/js/)
```

## Build

### PC (Windows)
```bash
npm run build
```

### Android Companion
```bash
cd NexusCompanion
./gradlew assembleRelease
```

## License

[GPL-3.0](LICENSE)
