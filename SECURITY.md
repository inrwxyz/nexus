# Security Policy & Architecture Controls

## Supported Versions
Only the latest release branch (`v2.x`) receives security updates and fixes.

## Key Security Controls

### 1. API Key Storage (`src-tauri/src/ai/vault.rs`)
- **DPAPI at Rest:** Third-party AI keys (`gemini`, `groq`, `openrouter`, `openai`, `claude`) are encrypted immediately with Windows Data Protection API (`CryptProtectData`) before being written to `%LOCALAPPDATA%\Nexus\ai-keys.vault`.
- **Masked UI Responses:** Plaintext keys are never returned to the WebView frontend; UI commands receive only `MaskedKeyInfo` (`••••xxxx`).

### 2. AI PowerShell Guardrails (`src-tauri/src/ai/terminal_guard.rs`)
- Disabled by default (`allow_ai_terminal = false`).
- Maintains a strict blocklist (`DESTRUCTIVE_PATTERNS`) against destructive file/disk/registry commands and obfuscated `Invoke-Expression` (`iex`) patterns.
- All mutating commands are queued as `PendingTerminalProposal` items and never execute without an explicit user click in the desktop UI.

### 3. Phone Link & Remote Terminal (`src-tauri/src/companion.rs`, `remote_terminal.rs`)
- **Bearer Auth:** HTTP (`0.0.0.0:41780`) companion endpoints require a per-pair cryptographic token (`companion.auth_token`). Unpairing (`revoke`) clears the token, notifies the phone over UDP (`{"type":"unauthorized"}`), rotates the pairing key, and suspends the Gamepad receiver.
- **Origin Allowlist (`src-tauri/src/server.rs`):** Browser `Origin` headers are checked against localhost/Tauri origins (`http://127.0.0.1:41780`, `http://localhost:41780`, `tauri://`, `https://tauri.localhost`).
- **Opt-In Remote Terminal:** Disabled by default (`companion.remote_terminal = false`). Enabling requires accepting an explicit desktop confirmation dialog, and command execution enforces `CMD_TIMEOUT` (15s) and output truncation (`MAX_OUTPUT_BYTES = 16 KiB`).

### 4. Voice Privacy
- Wake-word detection (`src-tauri/src/wake_word.rs`) runs locally offline.
- When `voice` is enabled, post-wake audio is sent to Google's unofficial Chromium Speech Recognition v2 endpoint (and Google TTS if set to Natural). Disabling the `voice` module or muting prevents audio capture and upload.

## Reporting a Vulnerability
Please report security issues privately through GitHub Security Advisories rather than opening public issues with exploit payloads.
