import type { AdbDevice, ScrcpyPrefs } from '../../../types/models.js';
import { $, $$, setText } from '../../core/dom.js';
import { on } from '../../core/bus.js';
import { invoke } from '../../core/tauri.js';
import { modState } from '../../core/state.js';
import { showToast } from '../../core/toast.js';
import {
  DEFAULT_SCRCPY_PREFS,
  bindScrcpyPrefControls,
  buildStartOptions,
  normalizeScrcpyPrefs,
  paintScrcpyPrefs,
} from './prefs.js';
import { createAdbController } from './adb.js';

export function initScrcpyFeature(): void {
  let prefs: ScrcpyPrefs = { ...DEFAULT_SCRCPY_PREFS };
  let activeDevice: AdbDevice | null = null;
  let statusTimer: ReturnType<typeof setInterval> | undefined;
  let prefsLoaded = false;
  let pendingAutoConnect = false;
  let runGen = 0;

  const isEnabled = (): boolean => Boolean(modState.scrcpy?.enabled);

  function setControlsSuspended(suspended: boolean): void {
    ['#page-scrcpy-connect', '#page-scrcpy-config'].forEach(sel => {
      const sec = $(sel);
      if (!sec) return;
      $$<HTMLButtonElement | HTMLInputElement>('button, input', sec).forEach(el => {
        el.disabled = Boolean(suspended);
      });
    });
  }

  function clearStatusTimer(): void {
    if (statusTimer !== undefined) {
      clearInterval(statusTimer);
      statusTimer = undefined;
    }
  }

  function suspendScrcpyUI(): void {
    runGen += 1;
    pendingAutoConnect = false;
    clearStatusTimer();
    activeDevice = null;
    adb.reset();
    setControlsSuspended(true);

    const rmBtn = $<HTMLButtonElement>('#sc-dev-remove');
    if (rmBtn) rmBtn.hidden = true;
    const ipEl = $<HTMLInputElement>('#sc-ip');
    const portEl = $<HTMLInputElement>('#sc-port');
    if (ipEl) ipEl.value = '';
    if (portEl) portEl.value = '';

    setText('sc-dev-name', 'Screen Mirror is off');
    setText('sc-dev-sub', 'Turn Screen Mirror on in System to detect devices');
    setText('sc-usb-sub', 'Turn Screen Mirror on first');
    setText('sc-ip-sub', 'Turn Screen Mirror on first');
    setText('sc-perm-sub', 'Turn Screen Mirror on first');
    setText('sc-mirror-title', 'Screen mirroring');
    setText('sc-status', 'Screen Mirror is off · Suspended');

    const btn = $<HTMLButtonElement>('#sc-stop');
    if (btn) {
      btn.textContent = 'Start';
      btn.classList.remove('btn-danger');
    }
  }

  async function refreshStatus(): Promise<void> {
    if (!isEnabled()) {
      clearStatusTimer();
      return;
    }
    const gen = runGen;
    let active = false;
    try {
      active = Boolean(await invoke<boolean>('sc_active'));
    } catch {
      active = false;
    }
    if (!isEnabled() || gen !== runGen) {
      clearStatusTimer();
      return;
    }

    if (active && statusTimer === undefined) {
      statusTimer = setInterval(() => {
        void refreshStatus();
      }, 1000);
    }
    if (!active) clearStatusTimer();

    const btn = $<HTMLButtonElement>('#sc-stop');
    const title = $('#sc-mirror-title');
    if (active) {
      if (title) title.textContent = 'Stop mirroring';
      setText(
        'sc-status',
        activeDevice ? `${activeDevice.model || 'Phone'} mirroring is active` : 'Mirroring is currently running'
      );
      if (btn) {
        btn.textContent = 'Stop';
        btn.disabled = false;
        btn.classList.add('btn-danger');
      }
    } else {
      if (title) title.textContent = 'Screen mirroring';
      setText(
        'sc-status',
        activeDevice ? `${activeDevice.model || 'Phone'} connected · Ready to mirror` : 'Not running · Press Start to mirror'
      );
      if (btn) {
        btn.textContent = 'Start';
        btn.disabled = false;
        btn.classList.remove('btn-danger');
      }
    }
  }

  async function startMirror(serial?: string | null): Promise<void> {
    if (!isEnabled()) {
      showToast('Screen Mirror is turned off');
      return;
    }
    const gen = runGen;
    try {
      await invoke('sc_start', { opts: buildStartOptions(prefs, serial || activeDevice?.serial) });
      if (isEnabled() && gen === runGen) showToast('Mirroring started');
    } catch (err) {
      if (isEnabled() && gen === runGen) {
        const msg =
          err && typeof err === 'object' && 'message' in err
            ? String((err as { message: unknown }).message)
            : String(err || 'Failed to start mirroring');
        showToast(msg);
      }
    }
    setTimeout(() => {
      if (isEnabled() && gen === runGen) void adb.refreshDevices();
    }, 600);
    setTimeout(() => {
      if (isEnabled() && gen === runGen) void refreshStatus();
    }, 800);
  }

  const adb = createAdbController({
    isEnabled,
    getRunGen: () => runGen,
    getActiveDevice: () => activeDevice,
    setActiveDevice: dev => {
      activeDevice = dev;
    },
    refreshStatus,
    startMirror,
  });

  bindScrcpyPrefControls(
    () => prefs,
    next => {
      prefs = next;
    },
    () => activeDevice?.serial
  );
  adb.bindAdbControls();

  $<HTMLButtonElement>('#sc-stop')?.addEventListener('click', e => {
    void adb.guard(e.currentTarget as HTMLButtonElement, async () => {
      let active = false;
      try {
        active = Boolean(await invoke<boolean>('sc_active'));
      } catch {
        active = false;
      }

      if (active) {
        try {
          await invoke('sc_stop');
          showToast('Mirroring stopped');
        } catch {
          /* ignore stop error */
        }
        setTimeout(() => {
          void refreshStatus();
        }, 400);
        return;
      }

      if (!activeDevice) {
        const ip = ($<HTMLInputElement>('#sc-ip')?.value || '').trim();
        const port = parseInt($<HTMLInputElement>('#sc-port')?.value || '5555', 10) || 5555;
        if (ip) {
          if (!/^(\d{1,3}\.){3}\d{1,3}$/.test(ip)) {
            showToast('Invalid IP format (e.g. 192.168.1.10)');
            return;
          }
          showToast(`Connecting to ${ip}:${port}...`);
          try {
            const serial = await invoke<string>('sc_connect', { ip, port });
            await adb.refreshDevices();
            await startMirror(serial);
          } catch (err) {
            const msg =
              err && typeof err === 'object' && 'message' in err
                ? String((err as { message: unknown }).message)
                : String(err);
            showToast(`Could not connect: ${msg}`);
          }
          return;
        }
        await adb.refreshDevices();
        if (!activeDevice) {
          showToast('No device found. Connect phone with USB or Wi-Fi first.');
          return;
        }
      }

      await startMirror(activeDevice?.serial);
    });
  });

  async function ensurePrefsLoaded(): Promise<void> {
    if (prefsLoaded) return;
    prefsLoaded = true;
    try {
      const loaded = await invoke<unknown>('sc_load_prefs');
      prefs = normalizeScrcpyPrefs(loaded);
    } catch {
      prefs = { ...DEFAULT_SCRCPY_PREFS };
    }
    paintScrcpyPrefs(prefs);
  }

  async function activateScrcpy(): Promise<void> {
    if (!isEnabled()) {
      suspendScrcpyUI();
      return;
    }
    const gen = ++runGen;
    setControlsSuspended(false);
    await ensurePrefsLoaded();
    if (!isEnabled() || gen !== runGen) return;
    paintScrcpyPrefs(prefs);
    setText('sc-dev-name', 'Checking connected phone…');
    setText('sc-dev-sub', 'Detecting USB and Wi-Fi ADB devices');
    setText('sc-status', 'Detecting devices…');
    await adb.prefillSavedIp();
    if (!isEnabled() || gen !== runGen) return;
    await adb.refreshDevices();
  }

  async function handleScrcpyTab(tab?: string): Promise<void> {
    await ensurePrefsLoaded();
    paintScrcpyPrefs(prefs);
    if (!isEnabled()) {
      suspendScrcpyUI();
      return;
    }
    setControlsSuspended(false);
    if (!tab || tab === 'connect') await adb.refreshDevices();
    else await refreshStatus();
  }

  function stopAll(): void {
    suspendScrcpyUI();
    invoke('sc_stop').catch(() => {});
    invoke('sc_qr_cancel').catch(() => {});
  }

  on('module:toggled', ({ module, enabled }) => {
    if (module !== 'scrcpy') return;
    if (!enabled) {
      stopAll();
    } else {
      pendingAutoConnect = true;
      setControlsSuspended(false);
      setText('sc-dev-name', 'Checking connected phone…');
      setText('sc-dev-sub', 'Detecting USB and Wi-Fi ADB devices');
      setText('sc-status', 'Detecting devices…');
    }
  });

  on('settings:saved', () => {
    if (!isEnabled()) {
      stopAll();
      return;
    }
    if (pendingAutoConnect) {
      pendingAutoConnect = false;
      void activateScrcpy();
    }
  });

  on('settings:sync', () => {
    if (!isEnabled()) {
      suspendScrcpyUI();
    } else {
      void activateScrcpy();
    }
  });

  on('module:changed', ({ module, tab }) => {
    if (module === 'scrcpy') void handleScrcpyTab(tab);
  });

  on('tab:changed', ({ module, tab }) => {
    if (module === 'scrcpy') void handleScrcpyTab(tab);
  });

  suspendScrcpyUI();
  void ensurePrefsLoaded();
}
