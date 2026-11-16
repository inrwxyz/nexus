# Nexus Phone Companion — Android Specification & Integration Guide

This document describes how to connect an Android device (via an app, shortcut, or Google Assistant) to the **Nexus** PC client (Rust + Tauri) running on the local Wi-Fi network.

---

## 1. Network Overview & Ports

* **PC HTTP API Port:** `41780`
  * Used for: Device Pairing, Status, and "Hey Google" Voice Command Relay.
  * Endpoint Base URL: `http://<PC_LOCAL_IP>:41780`
* **PC UDP Trackpad Port:** `41781`
  * Used for: 120Hz ultra-low latency mouse movement, clicks, momentum scrolling, and multi-touch gestures.
  * Endpoint: `udp://<PC_LOCAL_IP>:41781`

---

## 2. One-Time Pairing Flow

1. On the PC, open Nexus Settings $\rightarrow$ **Phone Link** tab.
2. The PC displays a **6-Digit Pairing PIN** (e.g. `839214`).
3. From the Android App, send an HTTP POST request:

```http
POST http://<PC_LOCAL_IP>:41780/api/companion/pair
Content-Type: application/json

{
  "code": "839214",
  "device": "Pixel 8 Pro"
}
```

### PC Response (Success):
```json
{
  "status": "ok",
  "token": "d7a4f910-8b1e-4c7b-a259-0f2c4180d5e1",
  "device": "Pixel 8 Pro",
  "udp_port": 41781
}
```

* **Action:** Save `token` and `PC_LOCAL_IP` in `SharedPreferences` or encrypted storage on Android. All subsequent requests require this `token`.

---

## 3. "Hey Google" Voice Command Relay

When you are away from the PC (e.g. lying in bed), you trigger Google Assistant (or an Android App Action / Shortcut):
* Voice prompt: *"Hey Google, run PC [command]"* or shortcut with input.
* Android extracts the raw command string (e.g., `"close chrome"`, `"volume 30"`, `"turn off pc"`, `"play lofi beats"`).
* Android sends HTTP POST to Nexus:

```http
POST http://<PC_LOCAL_IP>:41780/api/companion/command
Content-Type: application/json

{
  "token": "d7a4f910-8b1e-4c7b-a259-0f2c4180d5e1",
  "command": "close chrome"
}
```

### PC Response:
```json
{
  "status": "ok",
  "command": "close chrome",
  "success": true
}
```
* Nexus parses the open-ended text through its built-in executor, dispatches the system/media/window action, and displays visual feedback on the Inex Dynamic Island / Corner Pill.

---

## 4. Mac-Style 120Hz Wireless Trackpad (UDP: `41781`)

Stream touch coordinates directly to `udp://<PC_LOCAL_IP>:41781`.

### Option A: JSON UDP Packets (Simplest Implementation)
You can directly send UTF-8 JSON datagrams:

#### 1. Cursor Movement (1 Finger Drag):
```json
{"token":"<AUTH_TOKEN>","type":"move","dx":2.4,"dy":-1.1}
```
*(Nexus uses a sub-pixel floating accumulator so slow, tiny finger glides are silky smooth without quantization jitter).*

#### 2. Clicks & Taps:
* **1-Finger Tap (Left Click):**
  ```json
  {"token":"<AUTH_TOKEN>","type":"click","btn":"left"}
  ```
* **2-Finger Tap (Right Click):**
  ```json
  {"token":"<AUTH_TOKEN>","type":"click","btn":"right"}
  ```
* **Finger Down / Up (Drag & Drop):**
  ```json
  {"token":"<AUTH_TOKEN>","type":"down","btn":"left"}
  ```
  ```json
  {"token":"<AUTH_TOKEN>","type":"up","btn":"left"}
  ```

#### 3. Two-Finger Scrolling & Momentum:
* **Active Two-Finger Drag (Scroll):**
  ```json
  {"token":"<AUTH_TOKEN>","type":"scroll","dx":0.0,"dy":14.0}
  ```
* **Two-Finger Release (Trigger Momentum/Inertia Decay):**
  ```json
  {"token":"<AUTH_TOKEN>","type":"scroll_end"}
  ```
  *(Nexus automatically decays scroll velocity at 60–120 FPS until it halts smoothly).*

#### 4. Three-Finger Mac Gestures:
* **Swipe Up $\rightarrow$ Windows Task View (`Win + Tab`):**
  ```json
  {"token":"<AUTH_TOKEN>","type":"gesture","name":"swipe_up"}
  ```
* **Swipe Down $\rightarrow$ Show Desktop (`Win + D`):**
  ```json
  {"token":"<AUTH_TOKEN>","type":"gesture","name":"swipe_down"}
  ```
* **Swipe Left $\rightarrow$ Virtual Desktop Left (`Ctrl + Win + Left`):**
  ```json
  {"token":"<AUTH_TOKEN>","type":"gesture","name":"swipe_left"}
  ```
* **Swipe Right $\rightarrow$ Virtual Desktop Right (`Ctrl + Win + Right`):**
  ```json
  {"token":"<AUTH_TOKEN>","type":"gesture","name":"swipe_right"}
  ```

---

### Option B: Binary UDP Packets (16 Bytes, Extreme Performance)
For 120Hz zero-GC performance, Android can send a raw 16-byte `ByteBuffer` (Little Endian):

| Offset | Type | Field | Description |
|---|---|---|---|
| 0..3 | `u32` (LE) | `token_hash` | `CRC32(token.toByteArray())` |
| 4 | `u8` | `event_type` | `1`: Move, `2`: Down, `3`: Up, `4`: Click, `5`: Scroll, `6`: Gesture |
| 5 | `u8` | `param` | For click/down/up: `0`=left, `1`=right, `2`=middle. For gesture: `1`=swipe_up, `2`=swipe_down, `3`=swipe_left, `4`=swipe_right |
| 6..7 | `u16` (LE) | `flags` | Bit 0 (`0x01`): Trigger scroll inertia |
| 8..11 | `f32` (LE) | `dx` | Horizontal delta |
| 12..15 | `f32` (LE) | `dy` | Vertical delta |

---

## 5. Other APIs

* **Check Status:** `GET http://<PC_LOCAL_IP>:41780/api/companion/status`
* **Unpair Device:** `POST http://<PC_LOCAL_IP>:41780/api/companion/unpair`
* **Generate New PIN:** `POST http://<PC_LOCAL_IP>:41780/api/companion/generate_code`
