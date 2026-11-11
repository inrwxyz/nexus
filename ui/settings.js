// ─── Nexus — UI controller ────────────────────────────────────────────────────
// Static, lightweight. Left sidebar = modules, top pill bar = that module's sub-sections.
// Backend calls (get_settings / save_settings / toggle_mic_mute / check_update / apply_update /
// close_settings / open_browser, voice-event, mic-mute-changed) are unchanged.

const $  = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const T = window.__TAURI__;

// ── Theme (dark | light | system) ──────────────────────────────────────────
const prefs = { theme: 'dark' };
try { Object.assign(prefs, JSON.parse(localStorage.getItem('nexus-ui') || '{}')); } catch (e) {}
if (!['dark', 'light', 'system'].includes(prefs.theme)) prefs.theme = 'dark';
const savePrefs = () => { try { localStorage.setItem('nexus-ui', JSON.stringify(prefs)); } catch (e) {} };
function applyTheme() {
  document.documentElement.dataset.theme = prefs.theme;
  $$('.theme-opt').forEach(b => b.setAttribute('aria-checked', String(b.dataset.themeOpt === prefs.theme)));
}
const THEME_NAMES = { dark: 'Dark', light: 'Light', system: 'Windows System Default' };
$$('.theme-opt').forEach(b => b.addEventListener('click', () => {
  prefs.theme = b.dataset.themeOpt; savePrefs(); applyTheme(); showSaveToast(THEME_NAMES[prefs.theme]);
}));

// ── Elements ───────────────────────────────────────────────────────────────
const selWakeWord   = $('#sel-wakeword');
const toggleCorner  = $('#toggle-corner');
const toggleStartup = $('#toggle-startup');
const toggleUpdater = $('#toggle-updater');
const toast         = $('#save-toast');

// ── Plain text setter (no animation) ───────────────────────────────────────
function setText(el, text) { if (el && el.textContent !== text) el.textContent = text; }

// ── Toast ──────────────────────────────────────────────────────────────────
let toastTimer = null;
function showSaveToast(msg = 'Settings saved') {
  $('#save-toast-msg').textContent = msg;
  toast.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toast.classList.remove('show'), 1600);
}

// ── Modules → sub-sections ─────────────────────────────────────────────────
const SOON = (title, blurb, items) => ({ title, blurb, items });
const MODULES = {
  voice:    { name: 'Voice', tabs: [['home', 'Home'], ['voice', 'Voice'], ['system', 'System'], ['commands', 'Commands']] },
  search:   { name: 'Circle to Search', tabs: [['overview', 'Overview'], ['config', 'Configuration'], ['system', 'System']],
              info: SOON('Circle to Search', 'Draw around anything on your screen and search it instantly.',
                [['Screen selection', 'Circle or drag over any part of the screen'], ['Offline OCR', 'Extract text directly using Windows OCR'], ['Google Lens', 'Visual search any region in one click']]) },
  settings: { name: 'Settings', tabs: [['general', 'General'], ['theme', 'Theme'], ['about', 'About'], ['reset', 'Reset']] },
};

// Generated pages for modules (Circle to Search is wired; remaining modules show planned placeholders)
const rowHTML = ([l, s]) => `<div class="row"><div class="row-left"><div class="ico"><svg viewBox="0 0 24 24" width="16" height="16" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14M5 12h14"/></svg></div><div class="row-text"><div class="row-label">${l}</div><div class="row-sub">${s}</div></div></div></div>`;
const ICO_ARROW = '<svg viewBox="0 0 24 24" width="16" height="16" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M13 6l6 6-6 6"/></svg>';
const ICO_PILL  = '<svg viewBox="0 0 24 24" width="16" height="16" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="3"/><rect x="8" y="15" width="8" height="2" rx="1"/></svg>';
const ICO_KEY   = '<svg viewBox="0 0 24 24" width="16" height="16" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10"/></svg>';
const ICO_ACT   = '<svg viewBox="0 0 24 24" width="16" height="16" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.35-4.35"/></svg>';
const ICO_LANG  = '<svg viewBox="0 0 24 24" width="16" height="16" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="m5 8 6 6"/><path d="m4 14 6-6 2-3"/><path d="M2 5h12"/><path d="M7 2h1"/><path d="m22 22-5-10-5 10"/><path d="M14 18h6"/></svg>';

function buildPlaceholderPages() {
  const host = $('#page');
  for (const [key, m] of Object.entries(MODULES)) {
    if (!m.info) continue;
    const mk = (tab, kick, h1, p, body) => {
      const s = document.createElement('section');
      s.id = `page-${key}-${tab}`; s.className = 'page-section split';
      s.innerHTML = `<aside class="side"><div class="kicker">${kick}</div><h1>${h1}</h1><p>${p}</p></aside><div class="main">${body}</div>`;
      host.appendChild(s);
    };
    if (key === 'search') {
      mk('overview', '01', m.info.title, m.info.blurb,
        `<button class="launch-big" id="btn-cs-launch" type="button"><div class="launch-ico">${ICO_ACT}</div><div class="launch-text"><div class="launch-title">Launch Circle to Search</div><div class="launch-sub">Freeze the active monitor, then draw around anything to copy text or search it with Google Lens.</div></div><div class="launch-arrow">${ICO_ARROW}</div></button>` +
        `<div class="launch-foot"><span class="badge" id="cs-status-badge"><i></i>Ready</span></div>`);
      mk('config', '02', 'Configu&shy;ration', `Options for ${m.name}.`,
        `<div class="sheet">` +
        `<div class="row"><div class="row-left"><div class="ico">${ICO_PILL}</div><div class="row-text"><div class="row-label">On-Screen Trigger Pill</div><div class="row-sub">Show a tiny lower-right icon handle to launch Circle to Search</div></div></div><div class="row-ctrl"><label class="switch"><input type="checkbox" id="toggle-cs-pill" /><span class="slider"></span></label></div></div>` +
        `<div class="row"><div class="row-left"><div class="ico">${ICO_KEY}</div><div class="row-text"><div class="row-label">Global Hotkey</div><div class="row-sub" id="cs-hotkey-sub">Shortcut to freeze screen and draw anywhere</div></div></div><div class="row-ctrl row-ctrl-inline"><span class="badge" id="cs-hotkey-badge"><i></i>Ctrl+Shift+S</span><button id="btn-cs-hotkey" class="btn" type="button">Record Key</button></div></div>` +
        `<div class="row"><div class="row-left"><div class="ico">${ICO_ACT}</div><div class="row-text"><div class="row-label">Default Action</div><div class="row-sub">What runs right after circling a region</div></div></div><div class="row-ctrl"><div class="dd" data-dd data-select="sel-cs-action" data-value="dd-cs-action-value"><button class="dd-trigger field" type="button" aria-haspopup="listbox"><span class="dd-value" id="dd-cs-action-value">Show Options</span><svg viewBox="0 0 24 24" width="13" height="13" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg></button><div class="dd-menu" role="listbox"></div></div><select id="sel-cs-action" class="sr-only" tabindex="-1" aria-hidden="true"><option value="ask">Show Options  (Default)</option><option value="ocr">Extract Text (OCR)</option><option value="translate">Translate Selection</option><option value="lens">Google Lens</option></select></div></div>` +
        `<div class="row"><div class="row-left"><div class="ico">${ICO_LANG}</div><div class="row-text"><div class="row-label">Default Target Language</div><div class="row-sub">Used automatically by both In-Place and Google Translate Web</div></div></div><div class="row-ctrl"><div class="dd" data-dd data-select="sel-cs-lang" data-value="dd-cs-lang-value"><button class="dd-trigger field" type="button" aria-haspopup="listbox"><span class="dd-value" id="dd-cs-lang-value">Bengali</span><svg viewBox="0 0 24 24" width="13" height="13" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg></button><div class="dd-menu" role="listbox"></div></div><select id="sel-cs-lang" class="sr-only" tabindex="-1" aria-hidden="true"><option value="bn">Bengali  (Default)</option><option value="en">English</option><option value="hi">Hindi</option><option value="es">Spanish</option><option value="ja">Japanese</option><option value="fr">French</option><option value="de">German</option><option value="ar">Arabic</option><option value="ko">Korean</option><option value="zh-CN">Chinese</option><option value="pt">Portuguese</option><option value="ru">Russian</option></select></div></div>` +
        `<div class="row"><div class="row-left"><div class="ico">${ICO_ACT}</div><div class="row-text"><div class="row-label">Translation Mode</div><div class="row-sub">In-Place overlay (fast) or Google Translate Web for all languages</div></div></div><div class="row-ctrl"><div class="dd" data-dd data-select="sel-cs-trmode" data-value="dd-cs-trmode-value"><button class="dd-trigger field" type="button" aria-haspopup="listbox"><span class="dd-value" id="dd-cs-trmode-value">In-Place Screen Overlay</span><svg viewBox="0 0 24 24" width="13" height="13" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg></button><div class="dd-menu" role="listbox"></div></div><select id="sel-cs-trmode" class="sr-only" tabindex="-1" aria-hidden="true"><option value="in_place">In-Place Screen Overlay  (Default)</option><option value="google_web">Google Translate Web  (All Languages)</option></select></div></div>` +
        `</div>`);
    } else {
      mk('overview', '01', m.info.title, m.info.blurb,
        `<div class="sheet"><div class="sheet-label">Planned features</div>${m.info.items.map(rowHTML).join('')}</div><div><span class="badge"><i></i>Not available yet</span></div>`);
      mk('presets', '02', 'Presets', `Saved setups for ${m.name}.`,
        `<div class="sheet"><div class="empty"><b>No presets yet</b>Presets you save for ${m.name} will show up here.</div></div>`);
      mk('config', '03', 'Configu&shy;ration', `Options for ${m.name}.`,
        `<div class="sheet"><div class="empty"><b>Nothing to configure yet</b>Settings for ${m.name} will appear here.</div></div>`);
    }
    const sysKick = String(m.tabs.length).padStart(2, '0');
    mk('system', sysKick, 'Sys<em>tem</em>', `Turn ${m.name} on or off and choose whether it starts with Windows.`,
      `<div class="sheet" id="sys-${key}"></div>`);
  }
}

// ── Per-module on/off + launch-at-login (lives in each module's System tab) ──
const MOD_KEYS = ['voice', 'search'];
const MOD_BLURB = {
  voice: 'Wake word, voice commands and the corner pill',
  search: 'Circle anything on screen and search it',
};
const modState = {};
const resetModState = () => MOD_KEYS.forEach(k => { modState[k] = { enabled: true, launch_at_login: false }; });
resetModState();

const ICO_POWER = '<svg viewBox="0 0 24 24" width="16" height="16" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2v10"/><path d="M18.4 6.6a9 9 0 1 1-12.8 0"/></svg>';
const ICO_LOGIN = '<svg viewBox="0 0 24 24" width="16" height="16" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4"/><path d="m10 17 5-5-5-5"/><path d="M15 12H3"/></svg>';
const sysRow = (ico, label, sub, attr, k, extra = '', subAttr = '') => `<div class="row${extra}"><div class="row-left"><div class="ico">${ico}</div><div class="row-text"><div class="row-label">${label}</div><div class="row-sub"${subAttr}>${sub}</div></div></div><div class="row-ctrl"><label class="switch"><input type="checkbox" ${attr}="${k}" /><span class="slider"></span></label></div></div>`;
function buildModuleList() {
  MOD_KEYS.forEach(k => {
    const host = $(`#sys-${k}`); if (!host) return;
    const n = MODULES[k].name;
    host.innerHTML =
      sysRow(ICO_POWER, `${n} on / off`, `Turn ${n} on or off`, 'data-mod-enabled', k, ' mod-power-row') +
      sysRow(ICO_LOGIN, 'Launch at login', `Start ${n} automatically when Windows boots`, 'data-mod-login', k, ' mod-login-row', ` data-mod-login-sub="${k}"`);
    $$('[data-mod-enabled]', host).forEach(i => i.addEventListener('change', () => setModule(i.dataset.modEnabled, { enabled: i.checked })));
    $$('[data-mod-login]', host).forEach(i => i.addEventListener('change', () => setModule(i.dataset.modLogin, { launch_at_login: i.checked })));
    const loginRow = $('.mod-login-row', host);
    if (loginRow) loginRow.addEventListener('click', () => {
      if (!toggleStartup.checked) showSaveToast('Turn on Launch at Login in Settings → General first');
      else if (!modState[k].enabled) showSaveToast(`Turn on ${n} above first`);
    });
  });
}
function setModule(key, patch) {
  if (!modState[key]) return;
  if ('launch_at_login' in patch && (!toggleStartup.checked || !modState[key].enabled)) {
    applyModules();
    showSaveToast(!toggleStartup.checked ? 'Turn on Launch at Login in Settings → General first' : `Turn on ${MODULES[key].name} above first`);
    return;
  }
  Object.assign(modState[key], patch);
  applyModules();
  const name = MODULES[key].name;
  saveSettings('launch_at_login' in patch ? `${name}: start with Windows ${patch.launch_at_login ? 'on' : 'off'}` : `${name} ${patch.enabled ? 'on' : 'off'}`);
}
function applyModules() {
  const masterLogin = !!toggleStartup.checked;
  if (!masterLogin) {
    MOD_KEYS.forEach(k => { modState[k].launch_at_login = false; });
  }
  MOD_KEYS.forEach(k => {
    const b = $(`.mod[data-module="${k}"]`); if (b) b.classList.toggle('off', !modState[k].enabled);
    const en = $(`[data-mod-enabled="${k}"]`), lg = $(`[data-mod-login="${k}"]`), sub = $(`[data-mod-login-sub="${k}"]`);
    if (en) en.checked = modState[k].enabled;
    if (lg) {
      lg.checked = masterLogin && modState[k].enabled && modState[k].launch_at_login;
      lg.disabled = !masterLogin || !modState[k].enabled;
    }
    if (sub) {
      const n = MODULES[k].name;
      sub.textContent = !masterLogin
        ? 'Requires Launch at Login in Settings → General'
        : !modState[k].enabled
          ? `Turn on ${n} above first`
          : `Start ${n} automatically when Windows boots`;
    }
  });
  const isMod = MOD_KEYS.includes(curModule);
  $('#page').classList.toggle('module-off', isMod && !modState[curModule].enabled);
}

const subnav = $('#subnav');
let curModule = 'voice', curTab = 'home';

function showModule(key, tab) {
  const m = MODULES[key]; if (!m) return;
  curModule = key;
  $$('.mod').forEach(b => b.classList.toggle('active', b.dataset.module === key));
  subnav.textContent = '';
  m.tabs.forEach(([id, label]) => {
    const b = document.createElement('button');
    b.className = 'tab'; b.type = 'button'; b.dataset.tab = id; b.textContent = label;
    b.addEventListener('click', () => showTab(id));
    subnav.appendChild(b);
  });
  showTab(tab || m.tabs[0][0]);
  applyModules();
}
function showTab(id) {
  const to = $(`#page-${curModule}-${id}`); if (!to) return;
  curTab = id;
  $$('.tab', subnav).forEach(t => t.classList.toggle('active', t.dataset.tab === id));
  $$('.page-section.active').forEach(p => p.classList.remove('active'));
  to.classList.add('active');
  $('#page').scrollTop = 0;
  closeAllDropdowns();
}
$$('.mod').forEach(b => b.addEventListener('click', () => showModule(b.dataset.module)));

let csRecordingHotkey = false;
document.addEventListener('keydown', e => {
  if (csRecordingHotkey) return;
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  const tabs = MODULES[curModule].tabs, i = '123456789'.indexOf(e.key);
  if (i > -1 && tabs[i]) return showTab(tabs[i][0]);
  if (curModule === 'voice' && curTab === 'home' && !document.activeElement.closest('.dd')) {
    if (e.key === 'ArrowLeft')  cycleWake(-1);
    if (e.key === 'ArrowRight') cycleWake(1);
  }
});

// ── Dropdown ───────────────────────────────────────────────────────────────
const wakeLabel = o => o.textContent.replace(/\(.*?\)/, '').trim();
const CHECK = '<svg class="dd-check" viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>';
function buildMenu(dd, selectEl) {
  const menu = $('.dd-menu', dd);
  menu.textContent = '';
  [...selectEl.options].forEach(o => {
    const b = document.createElement('button');
    b.type = 'button'; b.className = 'dd-item'; b.dataset.v = o.value; b.setAttribute('role', 'option');
    const lab = document.createElement('span'); lab.className = 'dd-label'; lab.textContent = wakeLabel(o); b.appendChild(lab);
    if (/default/i.test(o.textContent)) { const em = document.createElement('em'); em.textContent = 'DEFAULT'; b.appendChild(em); }
    b.insertAdjacentHTML('beforeend', CHECK);
    b.addEventListener('click', () => {
      selectEl.value = o.value;
      selectEl.dispatchEvent(new Event('change'));
      closeAllDropdowns();
    });
    menu.appendChild(b);
  });
}
function closeAllDropdowns() { $$('.dd.open').forEach(d => d.classList.remove('open')); }
function initDropdowns() {
  $$('[data-dd]').forEach(dd => {
    if (dd.dataset.ddReady) return;
    dd.dataset.ddReady = '1';
    const selId = dd.dataset.select || 'sel-wakeword';
    const selectEl = $('#' + selId);
    if (!selectEl) return;
    const trigger = $('.dd-trigger', dd);
    buildMenu(dd, selectEl);
    trigger.addEventListener('click', e => { e.stopPropagation(); const was = dd.classList.contains('open'); closeAllDropdowns(); if (!was) dd.classList.add('open'); });
    dd.addEventListener('keydown', e => {
      const items = $$('.dd-item', dd), i = items.indexOf(document.activeElement);
      if (e.key === 'Escape') { dd.classList.remove('open'); trigger.focus(); }
      else if (e.key === 'ArrowDown') { e.preventDefault(); (items[i + 1] || items[0]).focus(); }
      else if (e.key === 'ArrowUp')   { e.preventDefault(); (items[i - 1] || items[items.length - 1]).focus(); }
    });
  });
}
document.addEventListener('click', e => { if (!e.target.closest('.dd')) closeAllDropdowns(); });

// ── Wake word ──────────────────────────────────────────────────────────────
function setWake(value) { selWakeWord.value = value; selWakeWord.dispatchEvent(new Event('change')); }
function cycleWake(dir) {
  const n = selWakeWord.options.length;
  setWake(selWakeWord.options[(selWakeWord.selectedIndex + dir + n) % n].value);
}
$('#wake-prev').addEventListener('click', () => cycleWake(-1));
$('#wake-next').addEventListener('click', () => cycleWake(1));

// ── Circle to Search state & sync ──────────────────────────────────────────
const csState = { on_screen_trigger: false, hotkey: 'Ctrl+Shift+S', default_action: 'ask', target_lang: 'bn', translate_mode: 'in_place' };
function syncCircleSearchUI() {
  const pill = $('#toggle-cs-pill'), selAct = $('#sel-cs-action'), selLang = $('#sel-cs-lang'), selTrMode = $('#sel-cs-trmode');
  if (pill) pill.checked = !!csState.on_screen_trigger;
  if (selAct) selAct.value = csState.default_action || 'ask';
  if (selLang) selLang.value = csState.target_lang || 'bn';
  if (selTrMode) selTrMode.value = csState.translate_mode || 'in_place';
  const badge = $('#cs-hotkey-badge');
  if (badge) badge.innerHTML = `<i></i>${csState.hotkey || 'Ctrl+Shift+S'}`;
}
async function refreshCsStatus() {
  if (!(T && T.core)) return;
  try {
    const st = await T.core.invoke('get_circle_search_status');
    const ov = $('#cs-status-badge'), sub = $('#cs-hotkey-sub');
    if (ov) {
      if (!st.enabled) ov.innerHTML = '<i></i>Off (disabled in System)';
      else if (st.hotkey_error) ov.innerHTML = `<i></i>Hotkey issue · Trigger Pill: ${st.on_screen_trigger ? 'On' : 'Off'}`;
      else ov.innerHTML = `<i></i>Ready · ${st.hotkey}${st.on_screen_trigger ? ' · Pill On' : ''}`;
    }
    if (sub) {
      sub.textContent = st.hotkey_error || (st.hotkey_registered ? `Active (${st.hotkey}) — freezes active monitor instantly` : 'Shortcut to freeze screen and draw anywhere');
      sub.style.color = st.hotkey_error ? 'var(--danger)' : '';
    }
  } catch (_) {}
}
function formatKeyEventCombo(e) {
  const k = e.key;
  if (['Control', 'Shift', 'Alt', 'Meta'].includes(k)) return null;
  const parts = [];
  if (e.ctrlKey) parts.push('Ctrl');
  if (e.altKey) parts.push('Alt');
  if (e.shiftKey) parts.push('Shift');
  if (e.metaKey) parts.push('Win');

  let main = '';
  const code = e.code || '';
  if (/^Key[A-Z]$/.test(code)) main = code.slice(3);
  else if (/^Digit[0-9]$/.test(code)) main = code.slice(5);
  else if (/^F([1-9]|1\d|2[0-4])$/.test(code)) main = code;
  else if (code === 'Space' || k === ' ') main = 'Space';
  else if (code === 'PrintScreen') main = 'PrintScreen';
  else if (code === 'Backquote') main = '`';
  else if (code === 'Minus') main = '-';
  else if (code === 'Equal') main = '=';
  else if (code === 'BracketLeft') main = '[';
  else if (code === 'BracketRight') main = ']';
  else if (code === 'Backslash') main = '\\';
  else if (code === 'Semicolon') main = ';';
  else if (code === 'Quote') main = "'";
  else if (code === 'Comma') main = ',';
  else if (code === 'Period') main = '.';
  else if (code === 'Slash') main = '/';
  else if (['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(k)) main = k.replace('Arrow', '');
  else if (['Insert', 'Delete', 'Home', 'End', 'PageUp', 'PageDown', 'Tab', 'Enter', 'Pause'].includes(k)) main = k;
  else return { error: 'Unsupported key — try Ctrl/Alt/Shift + letter, number, Space, or F1-F12' };

  const isFn = /^F\d+$/.test(main) || main === 'PrintScreen';
  if (parts.length === 0 && !isFn) return { error: 'Add a modifier (Ctrl, Alt, or Shift) or use F1-F12' };
  parts.push(main);
  return { combo: parts.join('+') };
}
function wireCircleSearchControls() {
  const pill = $('#toggle-cs-pill'), btnKey = $('#btn-cs-hotkey'), selAct = $('#sel-cs-action'), selLang = $('#sel-cs-lang'), selTrMode = $('#sel-cs-trmode'), btnLaunch = $('#btn-cs-launch');
  if (pill) pill.addEventListener('change', () => {
    csState.on_screen_trigger = pill.checked;
    saveSettings(`On-Screen Trigger Pill ${pill.checked ? 'on' : 'off'}`);
  });
  if (selAct) selAct.addEventListener('change', () => {
    csState.default_action = selAct.value;
    syncUI();
    saveSettings(`Default action: ${selAct.selectedOptions[0] ? wakeLabel(selAct.selectedOptions[0]) : selAct.value}`);
  });
  if (selLang) selLang.addEventListener('change', () => {
    csState.target_lang = selLang.value;
    syncUI();
    saveSettings(`Target language: ${selLang.selectedOptions[0] ? wakeLabel(selLang.selectedOptions[0]) : selLang.value}`);
  });
  if (selTrMode) selTrMode.addEventListener('change', () => {
    csState.translate_mode = selTrMode.value;
    syncUI();
    saveSettings(`Translation mode: ${selTrMode.selectedOptions[0] ? wakeLabel(selTrMode.selectedOptions[0]) : selTrMode.value}`);
  });
  if (btnLaunch) btnLaunch.addEventListener('click', async () => {
    if (T && T.core) {
      try { await T.core.invoke('trigger_circle_search'); } catch (e) { showSaveToast(String(e)); }
    }
  });
  if (btnKey) {
    const stopRec = () => {
      csRecordingHotkey = false;
      btnKey.textContent = 'Record Key';
    };
    btnKey.addEventListener('click', () => {
      if (csRecordingHotkey) { stopRec(); return; }
      csRecordingHotkey = true;
      btnKey.textContent = 'Press keys…';
    });
    document.addEventListener('keydown', e => {
      if (!csRecordingHotkey) return;
      e.preventDefault(); e.stopPropagation();
      if (e.key === 'Escape') { stopRec(); return; }
      const parsed = formatKeyEventCombo(e);
      if (!parsed) return; // still holding only modifier
      stopRec();
      if (parsed.error) { showSaveToast(parsed.error); return; }
      csState.hotkey = parsed.combo;
      syncCircleSearchUI();
      saveSettings(`Hotkey set to ${csState.hotkey}`);
    }, true);
  }
}

// ── Sync UI ← state ────────────────────────────────────────────────────────
function syncUI() {
  $$('[data-dd]').forEach(dd => {
    const sel = $('#' + (dd.dataset.select || 'sel-wakeword'));
    const valEl = $('#' + (dd.dataset.value || 'dd-voice-value'));
    if (!sel) return;
    const opt = sel.selectedOptions[0], lbl = opt ? wakeLabel(opt) : '';
    if (valEl) setText(valEl, lbl);
    $$('.dd-item', dd).forEach(i => i.classList.toggle('sel', i.dataset.v === sel.value));
    if (sel === selWakeWord) setText($('#wake-word'), lbl);
  });
  syncCircleSearchUI();
  const cap = (id, on) => { $(id).classList.toggle('on', on); setText($(id + '-v'), on ? 'On' : 'Off'); };
  cap('#cap-corner',  toggleCorner.checked);
  cap('#cap-updater', toggleUpdater.checked);
  resetVoiceLogIfIdle();
}
[['#cap-corner', toggleCorner], ['#cap-updater', toggleUpdater]].forEach(([sel, input]) => {
  $(sel).addEventListener('click', () => input.click());
});

// ── Load / Save ────────────────────────────────────────────────────────────
async function loadSettings() {
  try {
    let data;
    if (T && T.core) data = await T.core.invoke('get_settings');
    else { const res = await fetch('/api/settings'); if (res.ok) data = await res.json(); }
    if (data) {
      if (data.wake_word)                              selWakeWord.value     = data.wake_word;
      if (typeof data.corner_pill === 'boolean')       toggleCorner.checked  = data.corner_pill;
      if (typeof data.launch_at_login === 'boolean')   toggleStartup.checked = data.launch_at_login;
      if (typeof data.auto_updater === 'boolean')      toggleUpdater.checked = data.auto_updater;
      if (data.modules && typeof data.modules === 'object') MOD_KEYS.forEach(k => { const m = data.modules[k]; if (m) modState[k] = { enabled: m.enabled !== false, launch_at_login: !!m.launch_at_login }; });
      if (data.circle_search && typeof data.circle_search === 'object') {
        csState.on_screen_trigger = !!data.circle_search.on_screen_trigger;
        if (data.circle_search.hotkey) csState.hotkey = data.circle_search.hotkey;
        if (data.circle_search.default_action) csState.default_action = data.circle_search.default_action;
        if (data.circle_search.target_lang) csState.target_lang = data.circle_search.target_lang;
        if (data.circle_search.translate_mode) csState.translate_mode = data.circle_search.translate_mode;
      }
    }
  } catch (e) { console.log('Using default settings'); }
  syncUI(); applyModules(); refreshCsStatus();
}
async function saveSettings(msg) {
  const payload = {
    wake_word: selWakeWord.value, corner_pill: toggleCorner.checked,
    launch_at_login: toggleStartup.checked, auto_updater: toggleUpdater.checked,
    modules: JSON.parse(JSON.stringify(modState)),
    circle_search: { ...csState },
  };
  showSaveToast(typeof msg === 'string' ? msg : 'Settings saved');
  try {
    if (T && T.core) {
      await T.core.invoke('save_settings', { newSettings: payload });
      await refreshCsStatus();
    } else {
      await fetch('/api/settings', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(payload) });
    }
  } catch (e) { console.log('Saved locally:', payload); }
}
selWakeWord.addEventListener('change', () => { syncUI(); saveSettings(); });
[toggleCorner, toggleStartup, toggleUpdater].forEach(t => t.addEventListener('change', () => { syncUI(); applyModules(); saveSettings(); }));

// ── Window controls (Tauri window API) ─────────────────────────────────────
const appWin = (() => { try { return T && T.window && (T.window.getCurrentWindow ? T.window.getCurrentWindow() : T.window.getCurrent()); } catch (e) { return null; } })();
async function refreshMaxState() {
  if (!appWin) return;
  try {
    const m = await appWin.isMaximized();
    $('#win').classList.toggle('maxed', m);
    $('#btn-max').title = m ? 'Restore' : 'Maximize';
  } catch (e) {}
}
$('#btn-min').addEventListener('click', async () => { try { appWin ? await appWin.minimize() : null; } catch (e) { console.error(e); } });
$('#btn-max').addEventListener('click', async () => {
  try {
    if (appWin) { await appWin.toggleMaximize(); refreshMaxState(); }
    else if (document.fullscreenElement) document.exitFullscreen(); else document.documentElement.requestFullscreen();   // browser preview only
  } catch (e) { console.error(e); }
});
$('#btn-close').addEventListener('click', async e => {
  e.preventDefault();
  if (T && T.core) { try { await T.core.invoke('close_settings'); } catch (err) { console.error(err); } }   // hides to tray, as before
  else window.close();
});
if (appWin && appWin.onResized) appWin.onResized(refreshMaxState);
refreshMaxState();

// ── Reset App (inside Settings) ────────────────────────────────────────────
let resetArmed = false, resetTimer = null;
const resetRow = $('#btn-reset-app');
function disarmReset() {
  resetArmed = false; clearTimeout(resetTimer); resetRow.classList.remove('confirm');
  setText($('#reset-label'), 'Reset App');
  setText($('#reset-sub'), 'Restores wake word, corner pill, startup, update and module options, and the Dark theme.');
}
resetRow.addEventListener('click', async () => {
  if (!resetArmed) {
    resetArmed = true; resetRow.classList.add('confirm');
    setText($('#reset-label'), 'Click again to confirm');
    setText($('#reset-sub'), 'This cannot be undone.');
    resetTimer = setTimeout(disarmReset, 4000);
    return;
  }
  disarmReset();
  selWakeWord.value = 'hey google';
  toggleCorner.checked = true; toggleStartup.checked = false; toggleUpdater.checked = true;
  resetModState();
  Object.assign(csState, { on_screen_trigger: false, hotkey: 'Ctrl+Shift+S', default_action: 'ask', target_lang: 'bn' });
  prefs.theme = 'dark'; savePrefs(); applyTheme();
  syncUI(); applyModules();
  await saveSettings('App reset to defaults');
});

// ── Check for updates ──────────────────────────────────────────────────────
let checking = false, updateReadyVersion = null;
const SUB_DEFAULT = 'Check GitHub releases for the latest version';
function markUpdateAvailable(ver) {
  const row = $('#btn-check-update'), lbl = $('.row-label', row), sub = $('.row-sub', row);
  updateReadyVersion = ver;
  checking = false;
  row.classList.add('ok');
  setText(lbl, `Update Available: v${ver}`);
  setText(sub, 'Click to install and restart Nexus');
}
$('#btn-check-update').addEventListener('click', async () => {
  if (checking) return;
  checking = true;
  const row = $('#btn-check-update'), lbl = $('.row-label', row), sub = $('.row-sub', row);
  const back = ms => setTimeout(() => { row.classList.remove('ok'); setText(lbl, 'Check for Updates'); setText(sub, SUB_DEFAULT); checking = false; }, ms);
  if (updateReadyVersion && T && T.core) {
    setText(lbl, 'Downloading & installing…'); setText(sub, 'Nexus will restart automatically');
    try { await T.core.invoke('apply_update'); } catch (e) { setText(lbl, 'Update failed'); setText(sub, String(e || 'Error applying update')); back(4000); }
    return;
  }
  row.classList.remove('ok');
  setText(lbl, 'Checking for updates…'); setText(sub, 'Contacting GitHub releases');
  try {
    if (T && T.core) {
      const res = await T.core.invoke('check_update');
      if (res.available) {
        markUpdateAvailable(res.latest_version);
      } else if (res.error) { setText(lbl, 'Check failed'); setText(sub, String(res.error)); back(4000); }
      else { row.classList.add('ok'); setText(lbl, 'Up to date!'); setText(sub, `Nexus v${res.current_version} is the latest release.`); back(3000); }
    } else {
      setTimeout(() => { row.classList.add('ok'); setText(lbl, 'Up to date!'); setText(sub, 'Nexus v1.0.0 is the latest stable release.'); back(3000); }, 600);
    }
  } catch (err) { setText(lbl, 'Check failed'); setText(sub, String(err || 'Could not reach server')); back(3500); }
});

$('#btn-dev-profile').addEventListener('click', () => {
  if (T && T.core) T.core.invoke('open_browser', { url: 'https://github.com/inrwxyz' });
  else window.open('https://github.com/inrwxyz', '_blank');
});

// ── Live voice feed ────────────────────────────────────────────────────────
let vfClearTimer = null, currentMicMuted = false;
function resetVoiceLog() {
  const vfStatus = $('#vf-status'), vfLog = $('#vf-log');
  if (vfStatus && !currentMicMuted) vfStatus.textContent = 'Listening';
  if (vfLog) {
    const o = selWakeWord.selectedOptions[0];
    vfLog.textContent = 'Say "' + (o ? wakeLabel(o) : 'Hey Google') + '" to begin...';
    vfLog.style.color = 'var(--dim)';
  }
}
function resetVoiceLogIfIdle() { if (!vfClearTimer && /^Say "/.test($('#vf-log').textContent)) resetVoiceLog(); }
$('#vf-clear').addEventListener('click', () => { clearTimeout(vfClearTimer); vfClearTimer = null; resetVoiceLog(); });

if (T && T.event) {
  T.event.listen('voice-event', ev => {
    const p = ev.payload; if (!p) return;
    const vfStatus = $('#vf-status'), vfLog = $('#vf-log');
    if (vfStatus && p.status && !currentMicMuted) vfStatus.textContent = p.status;
    if (vfLog && p.text) {
      vfLog.textContent = p.text;
      vfLog.style.color = p.type === 'action' || p.type === 'wake' ? 'var(--accent)' : 'var(--text)';
    }
    clearTimeout(vfClearTimer); vfClearTimer = null;
    if (p.type === 'action') vfClearTimer = setTimeout(() => { vfClearTimer = null; resetVoiceLog(); }, 4200);
  });
  T.event.listen('mic-mute-changed', ev => { if (ev && ev.payload && typeof ev.payload.muted === 'boolean') applyMicMuteUI(ev.payload.muted); });
  T.event.listen('update-available', ev => { if (ev && ev.payload && ev.payload.version) markUpdateAvailable(ev.payload.version); });
}

// ── Mic mute ───────────────────────────────────────────────────────────────
function applyMicMuteUI(isMuted) {
  currentMicMuted = !!isMuted;
  const m = currentMicMuted;
  $('#cap-mic').classList.toggle('muted', m); $('#cap-mic').classList.toggle('on', !m);
  setText($('#cap-mic-v'), m ? 'Muted' : 'Active');
  $('#btn-mute-home').classList.toggle('muted', m); setText($('#btn-mute-home'), m ? 'Unmute' : 'Mute');
  const st = $('#vf-status'); setText(st, m ? 'Muted' : 'Listening'); st.style.color = m ? 'var(--danger)' : '';
  const bt = $('#btn-toggle-mic-voice'); setText(bt, m ? 'Unmute Mic' : 'Mute Mic'); bt.style.color = m ? 'var(--danger)' : '';
  const bd = $('#voice-mic-badge'); bd.innerHTML = m ? '<i></i>Muted' : '<i></i>Calibrated'; bd.style.color = m ? 'var(--danger)' : '';
  setText($('#voice-mic-sub'), m ? 'Microphone is currently muted • Listening paused' : 'Realtek High Definition Audio · Snappy Wake');
}
async function toggleMicMute() {
  try {
    if (T && T.core) applyMicMuteUI(await T.core.invoke('toggle_mic_mute'));
    else applyMicMuteUI(!currentMicMuted);
  } catch (_) {}
}
['#cap-mic', '#btn-mute-home', '#btn-toggle-mic-voice'].forEach(s => $(s).addEventListener('click', toggleMicMute));
if (T && T.core) T.core.invoke('get_mic_mute').then(applyMicMuteUI).catch(() => {});

// ── Boot ───────────────────────────────────────────────────────────────────
applyTheme();
buildPlaceholderPages();
initDropdowns();
wireCircleSearchControls();
buildModuleList();
showModule('voice', 'home');
loadSettings();
