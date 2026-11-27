import type { CircleSearchConfig } from '../../../types/models.js';
import { $ } from '../../core/dom.js';
import { on } from '../../core/bus.js';
import { invoke } from '../../core/tauri.js';
import { csState } from '../../core/state.js';
import { showToast } from '../../core/toast.js';
import { cleanOptionLabel, initDropdowns } from '../../core/navigation.js';
import { saveSettings, syncAllUI } from '../../core/settings-store.js';
import { buildCircleSearchPages } from './pages.js';

interface CircleSearchStatus {
  enabled: boolean;
  on_screen_trigger?: boolean;
  hotkey?: string;
  hotkey_error?: string | null;
}

let recordingHotkey = false;

const SPECIAL_CODES: Readonly<Record<string, string>> = Object.freeze({
  Space: 'Space',
  PrintScreen: 'PrintScreen',
  Backquote: '`',
  Minus: '-',
  Equal: '=',
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  Semicolon: ';',
  Quote: "'",
  Comma: ',',
  Period: '.',
  Slash: '/',
});

const NAV_KEYS = new Set([
  'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight',
  'Insert', 'Delete', 'Home', 'End', 'PageUp', 'PageDown', 'Tab', 'Enter', 'Pause',
]);

export function formatKeyEventCombo(
  e: Pick<KeyboardEvent, 'key' | 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>
): { combo?: string; error?: string } | null {
  const k = e.key;
  if (['Control', 'Shift', 'Alt', 'Meta'].includes(k)) return null;

  const parts: string[] = [];
  if (e.ctrlKey) parts.push('Ctrl');
  if (e.altKey) parts.push('Alt');
  if (e.shiftKey) parts.push('Shift');
  if (e.metaKey) parts.push('Win');

  let main = '';
  const code = e.code || '';
  if (/^Key[A-Z]$/.test(code)) main = code.slice(3);
  else if (/^Digit[0-9]$/.test(code)) main = code.slice(5);
  else if (/^F([1-9]|1\d|2[0-4])$/.test(code)) main = code;
  else if (SPECIAL_CODES[code]) main = SPECIAL_CODES[code];
  else if (k === ' ') main = 'Space';
  else if (NAV_KEYS.has(k)) main = k.replace('Arrow', '');
  else return { error: 'Unsupported key — try Ctrl/Alt/Shift + letter, number, Space, or F1-F12' };

  const isFn = /^F\d+$/.test(main) || main === 'PrintScreen';
  if (parts.length === 0 && !isFn) {
    return { error: 'Add a modifier (Ctrl, Alt, or Shift) or use F1-F12' };
  }
  parts.push(main);
  return { combo: parts.join('+') };
}

function syncCircleSearchUI(): void {
  const pill = $<HTMLInputElement>('#toggle-cs-pill');
  const selAct = $<HTMLSelectElement>('#sel-cs-action');
  const selLang = $<HTMLSelectElement>('#sel-cs-lang');
  const selTrMode = $<HTMLSelectElement>('#sel-cs-trmode');
  const badge = $('#cs-hotkey-badge');

  if (pill) pill.checked = Boolean(csState.on_screen_trigger);
  if (selAct) selAct.value = csState.default_action || 'ask';
  if (selLang) selLang.value = csState.target_lang || 'bn';
  if (selTrMode) selTrMode.value = csState.translate_mode || 'in_place';
  if (badge) badge.innerHTML = `<i></i>${csState.hotkey || 'Ctrl+Shift+S'}`;
}

export async function refreshCircleSearchStatus(): Promise<void> {
  try {
    const st = await invoke<CircleSearchStatus>('get_circle_search_status');
    if (!st) return;

    const badge = $('#cs-status-badge');
    const sub = $('#cs-hotkey-sub');
    if (badge) {
      if (!st.enabled) badge.innerHTML = '<i></i>Off (disabled in System)';
      else if (st.hotkey_error) {
        badge.innerHTML = `<i></i>Hotkey issue · Trigger Pill: ${st.on_screen_trigger ? 'On' : 'Off'}`;
      } else {
        badge.innerHTML = `<i></i>Ready · ${st.hotkey || csState.hotkey}`;
      }
    }
    if (sub) {
      sub.textContent = st.hotkey_error
        ? `Registration failed: ${st.hotkey_error}`
        : 'Press shortcut anywhere to freeze and circle';
    }
  } catch {
    /* ignore outside Tauri */
  }
}

type CsStringField = 'default_action' | 'target_lang' | 'translate_mode';

function bindDropdownControl(
  selectEl: HTMLSelectElement | null,
  fieldName: CsStringField,
  labelPrefix: string
): void {
  selectEl?.addEventListener('change', () => {
    (csState as Record<CsStringField, string>)[fieldName] = selectEl.value as CircleSearchConfig[CsStringField];
    syncAllUI();
    const opt = selectEl.selectedOptions?.[0];
    const label = opt ? cleanOptionLabel(opt) : selectEl.value;
    void saveSettings(`${labelPrefix}: ${label}`);
  });
}

export function initCircleSearchFeature(): void {
  buildCircleSearchPages();
  initDropdowns();

  const pill = $<HTMLInputElement>('#toggle-cs-pill');
  const btnKey = $<HTMLButtonElement>('#btn-cs-hotkey');
  const btnLaunch = $<HTMLButtonElement>('#btn-cs-launch');

  pill?.addEventListener('change', () => {
    csState.on_screen_trigger = pill.checked;
    void saveSettings(`On-Screen Trigger Pill ${pill.checked ? 'on' : 'off'}`);
    void refreshCircleSearchStatus();
  });

  bindDropdownControl($<HTMLSelectElement>('#sel-cs-action'), 'default_action', 'Default action');
  bindDropdownControl($<HTMLSelectElement>('#sel-cs-lang'), 'target_lang', 'Target language');
  bindDropdownControl($<HTMLSelectElement>('#sel-cs-trmode'), 'translate_mode', 'Translation mode');

  btnLaunch?.addEventListener('click', async () => {
    try {
      await invoke('trigger_circle_search');
    } catch (err) {
      showToast(String(err));
    }
  });

  if (btnKey) {
    const stopRec = (): void => {
      recordingHotkey = false;
      btnKey.textContent = 'Record Key';
    };

    btnKey.addEventListener('click', () => {
      recordingHotkey = !recordingHotkey;
      btnKey.textContent = recordingHotkey ? 'Press keys...' : 'Record Key';
    });

    window.addEventListener(
      'keydown',
      e => {
        if (!recordingHotkey) return;
        e.preventDefault();
        e.stopPropagation();

        if (e.key === 'Escape') {
          stopRec();
          return;
        }
        const parsed = formatKeyEventCombo(e);
        if (!parsed) return;
        stopRec();
        if (parsed.error || !parsed.combo) {
          showToast(parsed.error || 'Unsupported key');
          return;
        }
        csState.hotkey = parsed.combo;
        syncCircleSearchUI();
        void saveSettings(`Hotkey set to ${csState.hotkey}`).then(() => refreshCircleSearchStatus());
      },
      true
    );
  }

  on('settings:sync', () => {
    syncCircleSearchUI();
    void refreshCircleSearchStatus();
  });

  syncCircleSearchUI();
}
