export type TopModuleKey = 'voice' | 'search' | 'companion' | 'scrcpy';
export type AllModuleKey = TopModuleKey | 'gamepad';
export type NavModuleKey = TopModuleKey | 'settings';
export type ThemeMode = 'dark' | 'light' | 'system';

export interface ModuleState {
  enabled: boolean;
}

export interface ModuleMeta {
  name: string;
  sub: string;
  defaultOn: boolean;
  tabs: ReadonlyArray<readonly [string, string]>;
}

export interface CircleSearchConfig {
  on_screen_trigger: boolean;
  hotkey: string;
  default_action: 'ask' | 'ocr' | 'translate' | 'lens';
  target_lang: string;
  translate_mode: 'in_place' | 'google_web';
}

export interface CompanionConfig {
  paired?: boolean;
  paired_device_name?: string;
  paired_device: string | null;
  auth_token: string | null;
  pairing_code: string | null;
  pairing_url?: string;
  voice_relay: boolean;
  show_on_island: boolean;
  touchpad: boolean;
  inertia_scrolling: boolean;
  remote_terminal: boolean;
  sensitivity: number;
}

export interface GamepadConfig {
  default_mode: string;
  aim_sensitivity: number;
  gyro_sensitivity: number;
  gyro_deadzone: number;
  auto_sprint: boolean;
  haptics: boolean;
  auto_detect: boolean;
}

export interface AiConfig {
  enabled: boolean;
  voice_reply: boolean;
  voice_style: string;
  dictation_enabled: boolean;
  screen_context: boolean;
  local_llm_enabled: boolean;
  local_model: string;
  routing_mode: string;
  allow_ai_terminal: boolean;
}

export interface AuraSettingsPayload {
  setup_completed?: boolean;
  wake_word: string;
  corner_pill: boolean;
  show_on_island: boolean;
  launch_at_login: boolean;
  auto_updater: boolean;
  modules: Record<AllModuleKey, ModuleState>;
  circle_search: CircleSearchConfig;
  companion: CompanionConfig;
  gamepad: GamepadConfig;
  ai: AiConfig;
}

export interface UiPrefs {
  theme: ThemeMode;
  lastModule: NavModuleKey;
  lastTab: string;
}

export interface GamepadProfile {
  id: string;
  name: string;
  category?: string;
  description?: string;
  gyro?: boolean;
  items?: unknown[];
}

export interface GamepadStore {
  active_profile_id: string;
  detected_game?: string | null;
  profiles: GamepadProfile[];
}

export interface GamepadStatus {
  enabled: boolean;
  connected: boolean;
  held_keys_count: number;
  total_packets: number;
  vigem_installed: boolean;
}

export interface ScrcpyPrefs {
  res: '720' | '1080' | 'orig';
  bitrate: '4' | '8' | '16';
  fps: '60' | '120';
  screenOff: boolean;
  audioEnabled: boolean;
  audioBuf: '40' | '120' | '250';
  audioCodec: 'auto' | 'opus' | 'aac';
  audioDup: boolean;
}

export interface AdbDevice {
  serial: string;
  state: string;
  model?: string;
  is_wireless?: boolean;
}

export interface SavedWirelessDevice {
  ip: string;
  port: number;
  model?: string;
  auto_reconnect?: boolean;
}

export interface AiKeyInfo {
  id: string;
  label: string;
  provider: string;
  model: string;
  masked: string;
  enabled: boolean;
  cooldown_secs: number;
}

export interface AiProposal {
  id: string;
  reason: string;
  command: string;
}

export interface AiLocalStatus {
  running: boolean;
  ollama_installed: boolean;
  models?: string[];
}

export interface CircleOverlayInitInfo {
  width: number;
  height: number;
  default_action: string;
  target_lang: string;
  translate_mode: string;
}

export interface PhysicalBox {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface CssBox {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface TranslateLine {
  x: number;
  y: number;
  width: number;
  height: number;
  source_text: string;
  translated_text: string;
  bg_color?: string;
  fg_color?: string;
}

export interface NexusEventMap {
  'module:toggled': { module: AllModuleKey; enabled: boolean };
  'module:changed': { module: NavModuleKey; tab: string; prevModule: NavModuleKey };
  'tab:changed': { module: NavModuleKey; tab: string; prevTab: string };
  'settings:sync': undefined;
  'settings:saved': { modules: Record<AllModuleKey, ModuleState> };
  'companion:pairing-changed': { paired: boolean };
}
