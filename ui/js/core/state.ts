import type {
  AiConfig,
  AllModuleKey,
  CircleSearchConfig,
  CompanionConfig,
  GamepadConfig,
  ModuleMeta,
  ModuleState,
  NavModuleKey,
  ThemeMode,
  TopModuleKey,
  UiPrefs,
} from '../../types/models.js';

export const STORAGE_KEY = 'nexus-ui';

/** Canonical 4 top-level sidebar feature modules and their sub-tabs. */
export const MODULES: Record<TopModuleKey, ModuleMeta> = {
  voice: {
    name: 'Voice Command',
    sub: 'Voice',
    defaultOn: true,
    tabs: [
      ['home', 'Overview'],
      ['voice', 'Voice'],
      ['commands', 'Commands'],
      ['ai', 'AI & Keys'],
      ['system', 'System'],
    ],
  },
  search: {
    name: 'Circle to Search',
    sub: 'Search',
    defaultOn: true,
    tabs: [
      ['overview', 'Overview'],
      ['config', 'Config'],
      ['system', 'System'],
    ],
  },
  companion: {
    name: 'Phone Link',
    sub: 'Mobile & Controller',
    defaultOn: true,
    tabs: [
      ['pairing', 'Pairing'],
      ['features', 'Features'],
      ['gamepad', 'Gamepad'],
      ['system', 'System'],
    ],
  },
  scrcpy: {
    name: 'Screen Mirror',
    sub: 'USB / Wi-Fi',
    defaultOn: false,
    tabs: [
      ['connect', 'Connect'],
      ['config', 'Config'],
      ['system', 'System'],
    ],
  },
};

export const TOP_MOD_KEYS = Object.keys(MODULES) as TopModuleKey[];
export const ALL_MOD_KEYS: AllModuleKey[] = [...TOP_MOD_KEYS, 'gamepad'];

export const DEFAULT_PREFS: Readonly<UiPrefs> = Object.freeze({
  theme: 'dark',
  lastModule: 'voice',
  lastTab: 'home',
});

function loadPrefsRaw(): Partial<UiPrefs> & { lastModule?: string } {
  try {
    return (JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}') as Partial<UiPrefs>) || {};
  } catch {
    return {};
  }
}

const rawPrefs = loadPrefsRaw();
const rawLastMod: string | undefined = rawPrefs.lastModule;
const migratedLastModule: NavModuleKey =
  rawLastMod === 'gamepad'
    ? 'companion'
    : rawLastMod === 'settings' || (rawLastMod && rawLastMod in MODULES)
      ? (rawLastMod as NavModuleKey)
      : 'voice';
const migratedLastTab = rawLastMod === 'gamepad' ? 'gamepad' : rawPrefs.lastTab || 'home';

export const prefs: UiPrefs = {
  theme: (rawPrefs.theme as ThemeMode) || DEFAULT_PREFS.theme,
  lastModule: migratedLastModule,
  lastTab: migratedLastTab,
};

export function savePrefs(): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    /* ignore storage quota failures */
  }
}

export const setupState = {
  completed: true,
};

/** Per-module enable state matching Rust `AuraSettings.modules`. */
export const modState: Record<AllModuleKey, ModuleState> = {
  voice: { enabled: MODULES.voice.defaultOn },
  search: { enabled: MODULES.search.defaultOn },
  companion: { enabled: MODULES.companion.defaultOn },
  scrcpy: { enabled: MODULES.scrcpy.defaultOn },
  gamepad: { enabled: true },
};

export const csState: CircleSearchConfig = {
  on_screen_trigger: false,
  hotkey: 'Ctrl+Shift+S',
  default_action: 'ask',
  target_lang: 'bn',
  translate_mode: 'in_place',
};

export const compState: CompanionConfig = {
  paired: false,
  paired_device_name: '',
  paired_device: null,
  auth_token: null,
  pairing_code: null,
  pairing_url: '',
  voice_relay: true,
  show_on_island: true,
  touchpad: true,
  inertia_scrolling: true,
  remote_terminal: false,
  sensitivity: 1.2,
};

export const gamepadState: GamepadConfig = {
  default_mode: 'action',
  aim_sensitivity: 1.2,
  gyro_sensitivity: 1.3,
  gyro_deadzone: 0.08,
  auto_sprint: true,
  haptics: true,
  auto_detect: true,
};

export const aiState: AiConfig = {
  enabled: false,
  voice_reply: true,
  voice_style: 'google_natural',
  dictation_enabled: true,
  screen_context: true,
  local_llm_enabled: false,
  local_model: 'llama3.2:3b',
  routing_mode: 'hybrid',
  allow_ai_terminal: false,
};

export const navState: { curModule: NavModuleKey; curTab: string } = {
  curModule: prefs.lastModule,
  curTab: prefs.lastTab || 'home',
};

export function applyModuleSelection(selected: ReadonlyArray<TopModuleKey>): void {
  const chosen = new Set<TopModuleKey>(selected);
  for (const key of TOP_MOD_KEYS) {
    modState[key].enabled = chosen.has(key);
  }
  modState.gamepad.enabled = chosen.has('companion');
  setupState.completed = true;
}

export function resetUiStateToDefaults(): void {
  for (const key of TOP_MOD_KEYS) {
    modState[key].enabled = MODULES[key].defaultOn;
  }
  modState.gamepad.enabled = true;
  setupState.completed = true;

  csState.on_screen_trigger = false;
  csState.hotkey = 'Ctrl+Shift+S';
  csState.default_action = 'ask';
  csState.target_lang = 'bn';
  csState.translate_mode = 'in_place';

  compState.voice_relay = true;
  compState.show_on_island = true;
  compState.touchpad = true;
  compState.inertia_scrolling = true;
  compState.remote_terminal = false;
  compState.sensitivity = 1.2;

  gamepadState.default_mode = 'action';
  gamepadState.aim_sensitivity = 1.2;
  gamepadState.gyro_sensitivity = 1.3;
  gamepadState.gyro_deadzone = 0.08;
  gamepadState.auto_sprint = true;
  gamepadState.haptics = true;
  gamepadState.auto_detect = true;

  aiState.enabled = false;
  aiState.voice_reply = true;
  aiState.voice_style = 'google_natural';
  aiState.dictation_enabled = true;
  aiState.screen_context = true;
  aiState.local_llm_enabled = false;
  aiState.local_model = 'llama3.2:3b';
  aiState.routing_mode = 'hybrid';
  aiState.allow_ai_terminal = false;

  prefs.theme = 'dark';
  savePrefs();
}
