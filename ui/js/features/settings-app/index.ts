import type { TopModuleKey } from '../../../types/models.js';
import { $, $$ } from '../../core/dom.js';
import { on } from '../../core/bus.js';
import { invoke, listen } from '../../core/tauri.js';
import { applyModuleSelection, resetUiStateToDefaults, setupState } from '../../core/state.js';
import { applyTheme } from '../../core/theme.js';
import { firstEnabledModule, saveSettings, syncAllUI } from '../../core/settings-store.js';
import { showModule } from '../../core/navigation.js';

interface UpdateCheckResult {
  available: boolean;
  current_version: string;
  latest_version: string;
  error?: string | null;
}

const SUB_DEFAULT = 'Check GitHub releases for the latest version';
let resetArmed = false;
let resetTimer: ReturnType<typeof setTimeout> | undefined;
let checking = false;
let updateReadyVersion: string | null = null;

function initResetControl(): void {
  const row = $('#btn-reset-app');
  const label = $('#reset-label');
  const sub = $('#reset-sub');
  if (!row) return;

  function disarm(): void {
    resetArmed = false;
    row?.classList.remove('confirm');
    if (label) label.textContent = 'Reset App';
    if (sub) {
      sub.textContent =
        'Restores wake word, corner pill, startup, update and module options, and the Dark theme.';
    }
    if (resetTimer !== undefined) {
      clearTimeout(resetTimer);
      resetTimer = undefined;
    }
  }

  row.addEventListener('click', async () => {
    if (!resetArmed) {
      resetArmed = true;
      row.classList.add('confirm');
      if (label) label.textContent = 'Click again to confirm';
      if (sub) sub.textContent = 'This cannot be undone.';
      if (resetTimer !== undefined) clearTimeout(resetTimer);
      resetTimer = setTimeout(disarm, 4000);
      return;
    }

    disarm();
    resetUiStateToDefaults();

    const selWake = $<HTMLSelectElement>('#sel-wakeword');
    const tCorner = $<HTMLInputElement>('#toggle-corner');
    const tIsland = $<HTMLInputElement>('#toggle-island');
    const tStartup = $<HTMLInputElement>('#toggle-startup');
    const tUpdater = $<HTMLInputElement>('#toggle-updater');

    if (selWake) selWake.value = 'hey google';
    if (tCorner) tCorner.checked = true;
    if (tIsland) tIsland.checked = false;
    if (tStartup) tStartup.checked = false;
    if (tUpdater) tUpdater.checked = true;

    applyTheme();
    syncAllUI();
    await saveSettings('App reset to defaults');
  });
}

function initUpdaterControl(): void {
  const row = $('#btn-check-update');
  if (!row) return;
  const lbl = $('.row-label', row);
  const sub = $('.row-sub', row);

  const resetBanner = (ms = 3500): ReturnType<typeof setTimeout> =>
    setTimeout(() => {
      if (updateReadyVersion) return;
      row.classList.remove('ok');
      if (lbl) lbl.textContent = 'Check for Updates';
      if (sub) sub.textContent = SUB_DEFAULT;
    }, ms);

  function markUpdateAvailable(ver: string): void {
    updateReadyVersion = ver;
    checking = false;
    row?.classList.add('ok');
    if (lbl) lbl.textContent = `Update available: v${ver}`;
    if (sub) sub.textContent = 'Click to download and install now';
  }

  row.addEventListener('click', async () => {
    if (checking) return;

    if (updateReadyVersion) {
      checking = true;
      if (lbl) lbl.textContent = `Downloading v${updateReadyVersion}...`;
      if (sub) sub.textContent = 'Installing update — app will restart automatically';
      try {
        await invoke('apply_update');
      } catch (err) {
        if (lbl) lbl.textContent = 'Update failed';
        if (sub) sub.textContent = String(err || 'Error applying update');
        updateReadyVersion = null;
        resetBanner(4000);
      } finally {
        checking = false;
      }
      return;
    }

    checking = true;
    if (lbl) lbl.textContent = 'Checking...';
    if (sub) sub.textContent = 'Contacting GitHub releases';

    try {
      const res = await invoke<UpdateCheckResult>('check_update');
      if (res?.available && res.latest_version) {
        markUpdateAvailable(res.latest_version);
      } else {
        row.classList.add('ok');
        if (lbl) lbl.textContent = "You're up to date";
        if (sub) sub.textContent = res?.error || `v${res?.current_version || '2.0.0'} is the latest release`;
        resetBanner();
      }
    } catch (err) {
      if (lbl) lbl.textContent = 'Check failed';
      if (sub) sub.textContent = String(err || 'Network error');
      resetBanner();
    } finally {
      checking = false;
    }
  });

  void listen<{ version?: string }>('update-available', ev => {
    if (ev?.payload?.version) markUpdateAvailable(ev.payload.version);
  });
}

function initFirstRunSetup(): void {
  const modal = $('#setup-modal');
  const finishBtn = $<HTMLButtonElement>('#btn-setup-finish');
  const skipBtn = $<HTMLButtonElement>('#btn-setup-enable-all');
  if (!modal || !finishBtn) return;

  const getChecked = (): TopModuleKey[] =>
    $$<HTMLInputElement>('.setup-mod-check:checked', modal)
      .map(el => el.value as TopModuleKey);

  const syncFinishState = (): void => {
    const selected = getChecked();
    finishBtn.textContent =
      selected.length === 0
        ? 'Continue with all modules paused'
        : `Start with ${selected.length} module${selected.length > 1 ? 's' : ''}`;
  };

  const syncModalVisibility = (): void => {
    modal.hidden = setupState.completed;
    if (!setupState.completed) syncFinishState();
  };

  $$<HTMLLabelElement>('.setup-card', modal).forEach(card => {
    const cb = $<HTMLInputElement>('.setup-mod-check', card);
    cb?.addEventListener('change', () => {
      card.classList.toggle('selected', cb.checked);
      syncFinishState();
    });
  });

  const completeWith = async (chosen: TopModuleKey[]): Promise<void> => {
    applyModuleSelection(chosen);
    modal.hidden = true;
    if (chosen.length > 0) {
      showModule(firstEnabledModule({ [chosen[0]]: { enabled: true } }));
    }
    syncAllUI();
    await saveSettings(
      chosen.length ? `Enabled ${chosen.length} Nexus module(s)` : 'All modules paused — enable any in sidebar'
    );
  };

  finishBtn.addEventListener('click', () => {
    void completeWith(getChecked());
  });

  skipBtn?.addEventListener('click', () => {
    void completeWith(['voice', 'search', 'companion', 'scrcpy']);
  });

  on('settings:sync', syncModalVisibility);
  syncModalVisibility();
}

export function initSettingsAppFeature(): void {
  [$<HTMLInputElement>('#toggle-startup'), $<HTMLInputElement>('#toggle-updater')].forEach(el => {
    el?.addEventListener('change', () => {
      syncAllUI();
      void saveSettings();
    });
  });

  $('#btn-dev-profile')?.addEventListener('click', () => {
    void invoke('open_browser', { url: 'https://github.com/inrwxyz' });
  });

  initResetControl();
  initUpdaterControl();
  initFirstRunSetup();
}
