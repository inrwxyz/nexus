import type { AuraSettingsPayload, ModuleState, TopModuleKey } from '../../types/models.js';
import { $ } from './dom.js';
import { emit } from './bus.js';
import { invoke } from './tauri.js';
import { showToast } from './toast.js';
import {
  ALL_MOD_KEYS,
  TOP_MOD_KEYS,
  aiState,
  compState,
  csState,
  gamepadState,
  modState,
  setupState,
} from './state.js';
import { applyModules, refreshDropdowns, showModule } from './navigation.js';

export function firstEnabledModule(modules: Partial<Record<TopModuleKey, ModuleState>>): TopModuleKey {
  return TOP_MOD_KEYS.find(k => Boolean(modules[k]?.enabled)) || 'voice';
}

function applyLoadedSettings(data: Partial<AuraSettingsPayload> | null | undefined): void {
  if (!data) return;

  setupState.completed = data.setup_completed !== false;

  const selWake = $<HTMLSelectElement>('#sel-wakeword');
  const tCorner = $<HTMLInputElement>('#toggle-corner');
  const tIsland = $<HTMLInputElement>('#toggle-island');
  const tStartup = $<HTMLInputElement>('#toggle-startup');
  const tUpdater = $<HTMLInputElement>('#toggle-updater');

  if (data.wake_word && selWake) selWake.value = data.wake_word;
  if (typeof data.corner_pill === 'boolean' && tCorner) tCorner.checked = data.corner_pill;
  if (typeof data.show_on_island === 'boolean' && tIsland) tIsland.checked = data.show_on_island;
  if (typeof data.launch_at_login === 'boolean' && tStartup) tStartup.checked = data.launch_at_login;
  if (typeof data.auto_updater === 'boolean' && tUpdater) tUpdater.checked = data.auto_updater;

  if (data.modules && typeof data.modules === 'object') {
    for (const key of ALL_MOD_KEYS) {
      const entry = data.modules[key];
      if (entry && typeof entry.enabled === 'boolean') {
        modState[key].enabled = entry.enabled;
      }
    }
  }

  if (data.circle_search && typeof data.circle_search === 'object') {
    Object.assign(csState, data.circle_search);
  }

  if (data.companion && typeof data.companion === 'object') {
    Object.assign(compState, data.companion);
    compState.paired = Boolean(data.companion.auth_token || data.companion.paired_device);
  }

  if (data.gamepad && typeof data.gamepad === 'object') {
    Object.assign(gamepadState, data.gamepad);
  }

  if (data.ai && typeof data.ai === 'object') {
    Object.assign(aiState, data.ai);
  }
}

export function syncAllUI(): void {
  refreshDropdowns();
  applyModules();
  emit('settings:sync');
}

export async function loadSettings(): Promise<void> {
  try {
    let data = await invoke<AuraSettingsPayload>('get_settings');
    if (!data) {
      const res = await fetch('/api/settings').catch(() => null);
      if (res?.ok) data = (await res.json()) as AuraSettingsPayload;
    }
    if (data) {
      applyLoadedSettings(data);
      const firstMod = firstEnabledModule(data.modules || {});
      if (firstMod && !modState.voice.enabled) {
        showModule(firstMod);
      }
    }
  } catch (err) {
    console.warn('loadSettings failed (running standalone or mock):', err);
  }
  syncAllUI();
}

export async function saveSettings(toastMsg?: string): Promise<void> {
  const payload: AuraSettingsPayload = {
    setup_completed: setupState.completed,
    wake_word: $<HTMLSelectElement>('#sel-wakeword')?.value || 'hey google',
    corner_pill: Boolean($<HTMLInputElement>('#toggle-corner')?.checked),
    show_on_island: Boolean($<HTMLInputElement>('#toggle-island')?.checked),
    launch_at_login: Boolean($<HTMLInputElement>('#toggle-startup')?.checked),
    auto_updater: Boolean($<HTMLInputElement>('#toggle-updater')?.checked),
    modules: structuredClone(modState),
    circle_search: { ...csState },
    companion: {
      voice_relay: compState.voice_relay,
      touchpad: compState.touchpad,
      inertia_scrolling: compState.inertia_scrolling,
      remote_terminal: Boolean(compState.remote_terminal),
      sensitivity: compState.sensitivity,
      show_on_island: compState.show_on_island,
      paired_device: compState.paired_device || null,
      // Backend owns companion pairing secrets; never push stale/null tokens from UI.
      auth_token: null,
      pairing_code: null,
    },
    gamepad: {
      default_mode: gamepadState.default_mode || 'action',
      aim_sensitivity: Number(gamepadState.aim_sensitivity) || 1.2,
      gyro_sensitivity: Number(gamepadState.gyro_sensitivity) || 1.3,
      gyro_deadzone: Number(gamepadState.gyro_deadzone) || 0.08,
      auto_sprint: gamepadState.auto_sprint !== false,
      haptics: gamepadState.haptics !== false,
      auto_detect: gamepadState.auto_detect !== false,
    },
    ai: {
      enabled: Boolean(aiState.enabled),
      voice_reply: aiState.voice_reply !== false,
      voice_style: String(aiState.voice_style || 'google_natural'),
      dictation_enabled: aiState.dictation_enabled !== false,
      screen_context: aiState.screen_context !== false,
      local_llm_enabled: Boolean(aiState.local_llm_enabled),
      local_model: String(aiState.local_model || 'llama3.2:3b'),
      routing_mode: String(aiState.routing_mode || 'hybrid'),
      allow_ai_terminal: Boolean(aiState.allow_ai_terminal),
    },
  };

  try {
    const res = await invoke<boolean>('save_settings', { newSettings: payload });
    if (res === null) {
      await fetch('/api/settings', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
      }).catch(() => {});
    }
    showToast(toastMsg || 'Settings saved');
    emit('settings:saved', { modules: payload.modules });
  } catch (err) {
    console.error('saveSettings failed:', err);
    showToast('Failed to save settings');
  }
}
