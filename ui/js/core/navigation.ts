import type { AllModuleKey, NavModuleKey, TopModuleKey } from '../../types/models.js';
import { $, $$, setText } from './dom.js';
import { emit } from './bus.js';
import { MODULES, TOP_MOD_KEYS, modState, navState, prefs, savePrefs } from './state.js';

const CHECK_SVG =
  '<svg class="dd-check" viewBox="0 0 24 24" width="14" height="14" stroke="currentColor" stroke-width="2.2" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>';

const POWER_SVG =
  '<svg viewBox="0 0 24 24" width="17" height="17" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="M18.36 6.64a9 9 0 1 1-12.73 0"/><line x1="12" y1="2" x2="12" y2="12"/></svg>';

const SETTINGS_TABS: ReadonlyArray<readonly [string, string]> = [
  ['general', 'General'],
  ['theme', 'Theme'],
  ['about', 'About'],
  ['reset', 'Reset'],
];

export const cleanOptionLabel = (opt?: HTMLOptionElement | null): string =>
  (opt?.textContent || '').replace(/\s*\(Default\)\s*/i, '').trim();

export function buildModuleSysRows(onPersist?: (msg: string) => void): void {
  for (const key of TOP_MOD_KEYS) {
    const host = $(`#sys-${key}`);
    if (!host) continue;
    const name = MODULES[key].name;
    host.innerHTML = `
      <div class="row mod-power-row">
        <div class="row-left">
          <div class="ico">${POWER_SVG}</div>
          <div class="row-text">
            <div class="row-label">${name} on / off</div>
            <div class="row-sub">Turn ${name} on or off</div>
          </div>
        </div>
        <div class="row-ctrl">
          <label class="switch">
            <input type="checkbox" data-mod-enabled="${key}" />
            <span class="slider"></span>
          </label>
        </div>
      </div>
    `;
    const input = $<HTMLInputElement>('[data-mod-enabled]', host);
    input?.addEventListener('change', () => setModuleEnabled(key, input.checked, onPersist));
  }
}

export function setModuleEnabled(
  key: AllModuleKey,
  enabled: boolean,
  onPersist?: (msg: string) => void
): void {
  if (!modState[key]) return;
  modState[key].enabled = Boolean(enabled);
  applyModules();
  emit('module:toggled', { module: key, enabled: modState[key].enabled });
  if (onPersist) {
    const label = key === 'gamepad' ? 'Game Controller' : MODULES[key].name;
    onPersist(`${label} ${modState[key].enabled ? 'on' : 'off'}`);
  }
}

export function applyModules(): void {
  for (const key of TOP_MOD_KEYS) {
    $(`.mod[data-module="${key}"]`)?.classList.toggle('off', !modState[key].enabled);
    const input = $<HTMLInputElement>(`[data-mod-enabled="${key}"]`);
    if (input) input.checked = modState[key].enabled;
  }
  const gpToggle = $<HTMLInputElement>('#toggle-gp-enabled');
  if (gpToggle) gpToggle.checked = Boolean(modState.gamepad?.enabled);

  const isMod = (TOP_MOD_KEYS as readonly string[]).includes(navState.curModule);
  $('#page')?.classList.toggle(
    'module-off',
    isMod && !modState[navState.curModule as TopModuleKey].enabled
  );
}

export function showTab(id: string): void {
  const prevTab = navState.curTab;
  navState.curTab = id;
  prefs.lastModule = navState.curModule;
  prefs.lastTab = id;
  savePrefs();

  $$<HTMLButtonElement>('#subnav .tab').forEach(btn =>
    btn.classList.toggle('active', btn.dataset.tab === id)
  );
  const targetId = `page-${navState.curModule}-${id}`;
  $$('.page-section').forEach(sec => sec.classList.toggle('active', sec.id === targetId));
  const page = $('#page');
  if (page) page.scrollTop = 0;

  emit('tab:changed', { module: navState.curModule, tab: id, prevTab });
}

export function showModule(key: NavModuleKey | 'gamepad', tab?: string): void {
  const resolvedKey: NavModuleKey = key === 'gamepad' ? 'companion' : key;
  const resolvedTab = key === 'gamepad' ? 'gamepad' : tab;
  const isSettings = resolvedKey === 'settings';
  const mod = isSettings
    ? { name: 'Settings', tabs: SETTINGS_TABS }
    : MODULES[resolvedKey as TopModuleKey];
  if (!mod) return;

  const prevModule = navState.curModule;
  navState.curModule = resolvedKey;
  $$<HTMLButtonElement>('.mod').forEach(btn =>
    btn.classList.toggle('active', btn.dataset.module === resolvedKey)
  );
  setText('modname', mod.name);

  const subnav = $('#subnav');
  if (subnav) {
    subnav.innerHTML = '';
    for (const [id, label] of mod.tabs) {
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'tab';
      btn.dataset.tab = id;
      btn.textContent = label;
      btn.addEventListener('click', () => showTab(id));
      subnav.appendChild(btn);
    }
  }

  const nextTab = mod.tabs.some(([id]) => id === resolvedTab) ? resolvedTab! : mod.tabs[0][0];
  showTab(nextTab);
  applyModules();
  emit('module:changed', { module: resolvedKey, tab: nextTab, prevModule });
}

export function closeAllDropdowns(): void {
  $$('.dd.open').forEach(dd => dd.classList.remove('open'));
}

function buildDropdownMenu(dd: HTMLElement, selectEl: HTMLSelectElement): void {
  const menu = $('.dd-menu', dd);
  if (!menu) return;
  menu.textContent = '';

  [...selectEl.options].forEach(opt => {
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'dd-item';
    btn.dataset.v = opt.value;
    btn.setAttribute('role', 'option');

    const label = document.createElement('span');
    label.className = 'dd-label';
    label.textContent = cleanOptionLabel(opt);
    btn.appendChild(label);

    if (/default/i.test(opt.textContent || '')) {
      const badge = document.createElement('em');
      badge.textContent = 'DEFAULT';
      btn.appendChild(badge);
    }
    btn.insertAdjacentHTML('beforeend', CHECK_SVG);

    btn.addEventListener('click', () => {
      selectEl.value = opt.value;
      selectEl.dispatchEvent(new Event('change'));
      dd.classList.remove('open');
    });
    menu.appendChild(btn);
  });
}

let ddGlobalBound = false;

export function initDropdowns(): void {
  $$<HTMLElement>('[data-dd]').forEach(dd => {
    const selId = dd.dataset.select || 'sel-wakeword';
    const selectEl = $<HTMLSelectElement>(`#${selId}`);
    const trigger = $<HTMLButtonElement>('.field', dd);
    if (!selectEl || !trigger) return;

    buildDropdownMenu(dd, selectEl);
    if (dd.dataset.bound === '1') return;
    dd.dataset.bound = '1';

    trigger.addEventListener('click', e => {
      e.stopPropagation();
      const wasOpen = dd.classList.contains('open');
      closeAllDropdowns();
      if (!wasOpen) dd.classList.add('open');
    });

    dd.addEventListener('keydown', e => {
      const items = $$<HTMLButtonElement>('.dd-item', dd);
      const idx = document.activeElement instanceof HTMLButtonElement ? items.indexOf(document.activeElement) : -1;
      if (e.key === 'Escape') {
        dd.classList.remove('open');
        trigger.focus();
      } else if (e.key === 'ArrowDown' && items.length) {
        e.preventDefault();
        (items[idx + 1] || items[0]).focus();
      } else if (e.key === 'ArrowUp' && items.length) {
        e.preventDefault();
        (items[idx - 1] || items[items.length - 1]).focus();
      }
    });
  });

  if (!ddGlobalBound) {
    ddGlobalBound = true;
    document.addEventListener('click', e => {
      if (!(e.target instanceof Element) || !e.target.closest('.dd')) {
        closeAllDropdowns();
      }
    });
  }
}

export function refreshDropdowns(): void {
  $$<HTMLElement>('[data-dd]').forEach(dd => {
    const sel = $<HTMLSelectElement>(`#${dd.dataset.select || 'sel-wakeword'}`);
    const valEl = $(`#${dd.dataset.value || 'dd-voice-value'}`);
    if (!sel) return;

    const opt = sel.selectedOptions[0] || sel.options[0];
    const label = cleanOptionLabel(opt);
    if (valEl && valEl.textContent !== label) valEl.textContent = label;
    $$<HTMLElement>('.dd-item', dd).forEach(item =>
      item.classList.toggle('sel', item.dataset.v === sel.value)
    );

    if (sel.id === 'sel-wakeword') setText('wake-word', label);
  });
}

export function initSidebarNav(): void {
  $$<HTMLButtonElement>('.mod[data-module]').forEach(btn => {
    btn.addEventListener('click', () => {
      const mod = btn.dataset.module as NavModuleKey | undefined;
      if (mod) showModule(mod);
    });
  });
}
