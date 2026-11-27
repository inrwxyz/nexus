import type { ThemeMode } from '../../types/models.js';
import { $$ } from './dom.js';
import { prefs, savePrefs } from './state.js';
import { showToast } from './toast.js';

const THEME_LABELS: Readonly<Record<ThemeMode, string>> = Object.freeze({
  dark: 'Dark',
  light: 'Light',
  system: 'Windows System Default',
});

export function applyTheme(): void {
  document.documentElement.dataset.theme = prefs.theme || 'dark';
  $$<HTMLButtonElement>('.theme-opt').forEach(btn => {
    btn.setAttribute('aria-checked', String(btn.dataset.themeOpt === prefs.theme));
  });
}

export function initTheme(): void {
  applyTheme();
  $$<HTMLButtonElement>('.theme-opt').forEach(btn => {
    btn.addEventListener('click', () => {
      const next = (btn.dataset.themeOpt as ThemeMode | undefined) || 'dark';
      prefs.theme = next;
      savePrefs();
      applyTheme();
      showToast(THEME_LABELS[next] || 'Theme updated');
    });
  });
}
