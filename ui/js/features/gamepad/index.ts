import type { GamepadProfile, GamepadStatus, GamepadStore } from '../../../types/models.js';
import { $, $$, setText } from '../../core/dom.js';
import { on } from '../../core/bus.js';
import { invoke, listen } from '../../core/tauri.js';
import { compState, gamepadState, modState, navState } from '../../core/state.js';
import { setModuleEnabled } from '../../core/navigation.js';
import { showToast } from '../../core/toast.js';
import { saveSettings } from '../../core/settings-store.js';

let statusPollTimer: ReturnType<typeof setInterval> | undefined;

function esc(s: unknown): string {
  const map: Record<string, string> = {
    '&': '&amp;',
    '<': '&lt;',
    '>': '&gt;',
    '"': '&quot;',
    "'": '&#39;',
  };
  return String(s || '').replace(/[&<>"']/g, c => map[c] ?? c);
}

function profileCategoryIcon(cat?: string): string {
  if (cat === 'racing') {
    return '<svg viewBox="0 0 24 24" width="18" height="18" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="2.5"/><path d="M12 3v6.5M4.2 15.5l5.6-2.5M19.8 15.5l-5.6-2.5"/></svg>';
  }
  if (cat === 'console') {
    return '<svg viewBox="0 0 24 24" width="18" height="18" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="6" width="20" height="12" rx="5"/><path d="M6 12h4M8 10v4"/><circle cx="15.5" cy="10.5" r="1"/><circle cx="18" cy="12.5" r="1"/><circle cx="15.5" cy="14" r="1"/></svg>';
  }
  if (cat === 'action') {
    return '<svg viewBox="0 0 24 24" width="18" height="18" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="7"/><circle cx="12" cy="12" r="2"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3"/></svg>';
  }
  return '<svg viewBox="0 0 24 24" width="18" height="18" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3l2.6 5.3 5.9.8-4.2 4.1 1 5.8L12 16.2 6.7 19l1-5.8L3.5 9.1l5.9-.8z"/></svg>';
}

function profileShortDesc(p: GamepadProfile): string {
  if (p.id === 'preset_action') return 'WASD · Aim · Fire';
  if (p.id === 'preset_racing') return 'Pedals · Gyro';
  if (p.id === 'preset_xbox') return 'ABXY · Triggers';
  if (p.description) return p.description;
  return 'Custom HUD';
}

const RESET_ICON =
  '<svg viewBox="0 0 24 24" width="13" height="13" stroke="currentColor" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 1 0 3-6.7"/><path d="M3 3v5h5"/></svg>';
const DELETE_ICON =
  '<svg viewBox="0 0 24 24" width="13" height="13" stroke="currentColor" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18M6 6l12 12"/></svg>';

function gamepadReady(): boolean {
  return Boolean(modState.gamepad?.enabled) && Boolean(modState.companion?.enabled) && Boolean(compState.paired);
}

function showGpPanel(target: string): void {
  $$<HTMLButtonElement>('#gp-subnav .gp-subtab').forEach(b =>
    b.classList.toggle('on', b.dataset.gpTab === target)
  );
  $$<HTMLElement>('#page-companion-gamepad .gp-subcard').forEach(card =>
    card.classList.toggle('on', card.dataset.gpPanel === target)
  );
}

function renderProfilesList(store: GamepadStore | null | undefined): void {
  const host = $('#gp-profiles-list');
  if (!host || !store?.profiles) return;
  const visible = store.profiles.filter(p => p.id !== 'preset_minecraft');
  const activeId = store.active_profile_id;
  const enabled = gamepadReady();

  const activeProf = visible.find(p => p.id === activeId) || visible[0];
  if (activeProf) {
    const det = store.detected_game ? ` · Auto: ${store.detected_game}` : '';
    setText('gp-overview-active', `${activeProf.name} (${activeProf.items?.length || 0} controls)${det}`);
  }

  host.innerHTML = '';
  for (const p of visible) {
    const isActive = p.id === activeId;
    const isPreset = p.id.startsWith('preset_');
    const card = document.createElement('div');
    card.className = `gp-prof-card${isActive ? ' active' : ''}${enabled ? '' : ' disabled'}`;
    card.dataset.profileId = p.id;
    card.setAttribute('role', 'button');
    card.tabIndex = 0;

    card.innerHTML = `
      <div class="gp-card-top">
        <div class="gp-card-ico">${profileCategoryIcon(p.category)}</div>
        <div class="gp-card-tools">
          ${isActive ? '<span class="gp-card-dot">Active</span>' : ''}
          ${
            isPreset
              ? `<button type="button" class="gp-icon-btn" data-act="reset" title="Reset to default">${RESET_ICON}</button>`
              : `<button type="button" class="gp-icon-btn danger" data-act="delete" title="Delete profile">${DELETE_ICON}</button>`
          }
        </div>
      </div>
      <div class="gp-card-mid">
        <div class="gp-prof-name">${esc(p.name)}</div>
        <div class="gp-prof-sub">${esc(profileShortDesc(p))}</div>
      </div>
      <div class="gp-card-foot">
        <span>${p.items?.length || 0} keys</span>
        <span class="gp-chip">${p.gyro ? 'Gyro' : esc(p.category || 'Custom')}</span>
      </div>
    `;

    const activateCard = async (): Promise<void> => {
      if (!gamepadReady()) {
        showToast(
          !modState.gamepad?.enabled
            ? 'Turn Game Controller on first'
            : 'Pair your phone in Phone Link first'
        );
        return;
      }
      try {
        const next = await invoke<GamepadStore>('select_gamepad_profile', {
          profileId: p.id,
          sendToPhone: true,
        });
        if (next) renderProfilesList(next);
        showToast(`"${p.name}" active & sent to phone`);
      } catch (err) {
        showToast(String(err || 'Turn Game Controller ON first'));
      }
    };

    card.addEventListener('click', () => {
      void activateCard();
    });
    card.addEventListener('keydown', ev => {
      if (ev.key === 'Enter' || ev.key === ' ') {
        ev.preventDefault();
        void activateCard();
      }
    });

    card.querySelector('[data-act="reset"]')?.addEventListener('click', async ev => {
      ev.stopPropagation();
      try {
        const next = await invoke<GamepadStore>('reset_gamepad_preset', { profileId: p.id });
        if (next) renderProfilesList(next);
        showToast(`Reset "${p.name}" to default`);
      } catch (err) {
        showToast(String(err || 'Reset failed'));
      }
    });

    card.querySelector('[data-act="delete"]')?.addEventListener('click', async ev => {
      ev.stopPropagation();
      try {
        const next = await invoke<GamepadStore>('delete_gamepad_profile', { profileId: p.id });
        if (next) renderProfilesList(next);
        showToast(`Deleted "${p.name}"`);
      } catch (err) {
        showToast(String(err || 'Delete failed'));
      }
    });

    host.appendChild(card);
  }
}

async function refreshProfiles(): Promise<void> {
  try {
    const store = await invoke<GamepadStore>('get_gamepad_profiles');
    if (store) renderProfilesList(store);
  } catch {
    /* ignore in mock */
  }
}

function syncGamepadControls(): void {
  const masterToggle = $<HTMLInputElement>('#toggle-gp-enabled');
  const autoSprint = $<HTMLInputElement>('#toggle-gp-autosprint');
  const haptics = $<HTMLInputElement>('#toggle-gp-haptics');
  const autoDetect = $<HTMLInputElement>('#toggle-gp-autodetect');
  const aimSlider = $<HTMLInputElement>('#gp-aim-slider');
  const gyroSlider = $<HTMLInputElement>('#gp-gyro-slider');
  const dzSlider = $<HTMLInputElement>('#gp-dz-slider');

  if (masterToggle) masterToggle.checked = Boolean(modState.gamepad?.enabled);
  if (autoSprint) autoSprint.checked = gamepadState.auto_sprint !== false;
  if (haptics) haptics.checked = gamepadState.haptics !== false;
  if (autoDetect) autoDetect.checked = gamepadState.auto_detect !== false;

  const aim = Number.isFinite(Number(gamepadState.aim_sensitivity))
    ? Number(gamepadState.aim_sensitivity)
    : 1.2;
  const gyro = Number.isFinite(Number(gamepadState.gyro_sensitivity))
    ? Number(gamepadState.gyro_sensitivity)
    : 1.3;
  const dz = Number.isFinite(Number(gamepadState.gyro_deadzone))
    ? Number(gamepadState.gyro_deadzone)
    : 0.08;

  if (aimSlider) aimSlider.value = String(aim);
  if (gyroSlider) gyroSlider.value = String(gyro);
  if (dzSlider) dzSlider.value = String(dz);

  setText('gp-aim-val', `${aim.toFixed(1)}x`);
  setText('gp-gyro-val', `${gyro.toFixed(1)}x`);
  setText('gp-dz-val', `${Math.round(dz * 100)}%`);

  const modeSeg = $('.seg[data-gp-mode]');
  if (modeSeg) {
    $$<HTMLButtonElement>('.sg', modeSeg).forEach(btn => {
      btn.classList.toggle('on', btn.dataset.v === (gamepadState.default_mode || 'action'));
    });
  }

  const isGpOff = !modState.gamepad?.enabled;
  const pageGp = $('#page-companion-gamepad');
  if (pageGp) pageGp.classList.toggle('gp-off', isGpOff);
  $$(
    '#page-companion-gamepad .gp-subcard[data-gp-panel="overview"] .row:not(.gp-master-row), #gp-subnav .gp-subtab:not([data-gp-tab="overview"]), #page-companion-gamepad .gp-subcard:not([data-gp-panel="overview"])'
  ).forEach(el => {
    el.inert = isGpOff;
  });
  if (isGpOff) showGpPanel('overview');
}

async function refreshGamepadStatus(): Promise<void> {
  const onMod = Boolean(modState.gamepad?.enabled);
  const compOn = Boolean(modState.companion?.enabled);
  const paired = Boolean(compState.paired);
  const title = $('#gp-conn-title');
  const sub = $('#gp-conn-sub');
  if (!title || !sub) return;

  if (!onMod) {
    title.textContent = 'Game Controller is off';
    sub.textContent = 'Enable Game Controller above to play from your phone';
    setText('gp-driver-sub', 'ScanCode Keyboard + Mouse engine suspended');
    return;
  }

  if (!compOn || !paired) {
    title.textContent = 'Gamepad Suspended';
    sub.textContent = !compOn
      ? 'Enable Phone Link first to use Game Controller'
      : 'Pair your phone in Phone Link → Pairing to activate Game Controller';
    setText('gp-driver-sub', 'Waiting for paired Phone Link device');
    return;
  }

  try {
    const [st, profStore] = await Promise.all([
      invoke<GamepadStatus>('get_gamepad_status'),
      invoke<GamepadStore>('get_gamepad_profiles').catch(() => null),
    ]);
    if (profStore) renderProfilesList(profStore);
    if (!st) return;
    const det = profStore?.detected_game ? ` · Game: ${profStore.detected_game}` : '';
    if (st.connected) {
      title.textContent = 'Phone Controller Active';
      sub.textContent = `Receiving live input over UDP · ${st.held_keys_count} held · ${st.total_packets} packets${det}`;
    } else {
      title.textContent = 'Ready for Phone Controller';
      sub.textContent = `Open Gamepad on your phone or pick a Profile HUD card${det}`;
    }
    setText(
      'gp-driver-sub',
      st.vigem_installed
        ? 'Hardware ScanCode + Mouse ready (ViGEmBus driver detected)'
        : 'Hardware ScanCode + Raw Mouse ready (works in PC games with zero extra drivers)'
    );
  } catch {
    /* ignore in mock browser */
  }
}

function updateStatusPolling(): void {
  const active = navState.curModule === 'companion' && navState.curTab === 'gamepad';
  if (active && statusPollTimer === undefined) {
    void refreshGamepadStatus();
    statusPollTimer = setInterval(() => {
      if (!document.hidden && modState.gamepad?.enabled) void refreshGamepadStatus();
    }, 1500);
  } else if (!active && statusPollTimer !== undefined) {
    clearInterval(statusPollTimer);
    statusPollTimer = undefined;
  }
}

export function initGamepadFeature(): void {
  const masterToggle = $<HTMLInputElement>('#toggle-gp-enabled');
  const autoSprint = $<HTMLInputElement>('#toggle-gp-autosprint');
  const haptics = $<HTMLInputElement>('#toggle-gp-haptics');
  const autoDetect = $<HTMLInputElement>('#toggle-gp-autodetect');
  const aimSlider = $<HTMLInputElement>('#gp-aim-slider');
  const gyroSlider = $<HTMLInputElement>('#gp-gyro-slider');
  const dzSlider = $<HTMLInputElement>('#gp-dz-slider');

  masterToggle?.addEventListener('change', () => {
    setModuleEnabled('gamepad', masterToggle.checked, msg => {
      syncGamepadControls();
      void saveSettings(msg);
    });
  });

  $$<HTMLButtonElement>('#gp-subnav .gp-subtab').forEach(btn => {
    btn.addEventListener('click', () => {
      showGpPanel(btn.dataset.gpTab || 'overview');
    });
  });

  $$<HTMLButtonElement>('.seg[data-gp-mode] .sg').forEach(btn => {
    btn.addEventListener('click', () => {
      gamepadState.default_mode = btn.dataset.v || 'action';
      syncGamepadControls();
      void saveSettings(`Default layout: ${btn.textContent?.trim()}`);
    });
  });

  autoDetect?.addEventListener('change', () => {
    gamepadState.auto_detect = autoDetect.checked;
    void saveSettings(`Auto game detection ${autoDetect.checked ? 'on' : 'off'}`);
  });

  autoSprint?.addEventListener('change', () => {
    gamepadState.auto_sprint = autoSprint.checked;
    void saveSettings(`Stick Auto-Sprint ${autoSprint.checked ? 'on' : 'off'}`);
  });

  haptics?.addEventListener('change', () => {
    gamepadState.haptics = haptics.checked;
    void saveSettings(`Controller haptics ${haptics.checked ? 'on' : 'off'}`);
  });

  aimSlider?.addEventListener('input', () => {
    gamepadState.aim_sensitivity = parseFloat(aimSlider.value) || 1.2;
    setText('gp-aim-val', `${gamepadState.aim_sensitivity.toFixed(1)}x`);
  });
  aimSlider?.addEventListener('change', () => {
    void saveSettings('Aim sensitivity updated');
  });

  gyroSlider?.addEventListener('input', () => {
    gamepadState.gyro_sensitivity = parseFloat(gyroSlider.value) || 1.3;
    setText('gp-gyro-val', `${gamepadState.gyro_sensitivity.toFixed(1)}x`);
  });
  gyroSlider?.addEventListener('change', () => {
    void saveSettings('Gyro sensitivity updated');
  });

  dzSlider?.addEventListener('input', () => {
    gamepadState.gyro_deadzone = parseFloat(dzSlider.value) || 0.08;
    setText('gp-dz-val', `${Math.round(gamepadState.gyro_deadzone * 100)}%`);
  });
  dzSlider?.addEventListener('change', () => {
    void saveSettings('Gyro deadzone updated');
  });

  $('#gp-overview-profile-row')?.addEventListener('click', () => {
    if (modState.gamepad?.enabled) showGpPanel('profiles');
  });

  $('#btn-gp-release')?.addEventListener('click', async () => {
    try {
      await invoke('release_gamepad_inputs');
      showToast('All held keys & mouse buttons released');
      void refreshGamepadStatus();
    } catch {
      /* ignore */
    }
  });

  void listen<GamepadStore>('gamepad-profiles-changed', ev => {
    if (ev?.payload) renderProfilesList(ev.payload);
  });

  on('module:toggled', ({ module, enabled }) => {
    if (module === 'gamepad' || module === 'companion') {
      if (!enabled) invoke('release_gamepad_inputs').catch(() => {});
      syncGamepadControls();
      void refreshGamepadStatus();
      void refreshProfiles();
    }
  });

  on('companion:pairing-changed', ({ paired }) => {
    if (!paired) invoke('release_gamepad_inputs').catch(() => {});
    void refreshGamepadStatus();
    void refreshProfiles();
  });

  on('settings:sync', () => {
    syncGamepadControls();
    void refreshGamepadStatus();
    void refreshProfiles();
  });
  on('module:changed', updateStatusPolling);
  on('tab:changed', updateStatusPolling);

  syncGamepadControls();
  void refreshProfiles();
}
