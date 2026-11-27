/**
 * Nexus Settings UI entry point (Strict TypeScript + Vite 8).
 */

import { initTheme } from './js/core/theme.js';
import { initWindowControls } from './js/core/window.js';
import { buildModuleSysRows, initDropdowns, initSidebarNav, showModule } from './js/core/navigation.js';
import { loadSettings, saveSettings } from './js/core/settings-store.js';
import { initVoiceFeature } from './js/features/voice/index.js';
import { initCircleSearchFeature } from './js/features/circle-search/index.js';
import { initCompanionFeature } from './js/features/companion/index.js';
import { initScrcpyFeature } from './js/features/scrcpy/index.js';
import { initGamepadFeature } from './js/features/gamepad/index.js';
import { initSettingsAppFeature } from './js/features/settings-app/index.js';

initTheme();
initWindowControls();
initCircleSearchFeature();
buildModuleSysRows(msg => {
  void saveSettings(msg);
});
initDropdowns();
initSidebarNav();

initVoiceFeature();
initCompanionFeature();
initScrcpyFeature();
initGamepadFeature();
initSettingsAppFeature();

showModule('voice', 'home');
void loadSettings();
