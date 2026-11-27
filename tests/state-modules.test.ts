import { describe, expect, it } from 'vitest';
import {
  MODULES,
  TOP_MOD_KEYS,
  applyModuleSelection,
  modState,
  resetUiStateToDefaults,
  setupState,
} from '../ui/js/core/state.js';
import { firstEnabledModule } from '../ui/js/core/settings-store.js';

describe('nexus 4-module state & phone-link gamepad structure', () => {
  it('exposes exactly 4 top-level sidebar modules with Gamepad inside Phone Link', () => {
    expect(TOP_MOD_KEYS).toEqual(['voice', 'search', 'companion', 'scrcpy']);
    expect(MODULES.companion.tabs.map(([id]) => id)).toEqual([
      'pairing',
      'features',
      'gamepad',
      'system',
    ]);
    expect(MODULES.scrcpy.name).toBe('Screen Mirror');
  });

  it('enables only chosen modules on first-run selection and disables unselected ones', () => {
    resetUiStateToDefaults();
    applyModuleSelection(['search']);

    expect(setupState.completed).toBe(true);
    expect(modState.search.enabled).toBe(true);
    expect(modState.voice.enabled).toBe(false);
    expect(modState.companion.enabled).toBe(false);
    expect(modState.gamepad.enabled).toBe(false);
    expect(modState.scrcpy.enabled).toBe(false);
    expect(firstEnabledModule(modState)).toBe('search');
  });

  it('enables gamepad automatically when companion is selected in setup', () => {
    applyModuleSelection(['companion', 'scrcpy']);
    expect(modState.companion.enabled).toBe(true);
    expect(modState.gamepad.enabled).toBe(true);
    expect(modState.scrcpy.enabled).toBe(true);
    expect(modState.voice.enabled).toBe(false);
    expect(modState.search.enabled).toBe(false);
  });
});
