import type { AdbDevice, SavedWirelessDevice } from '../../../types/models.js';
import { $, setText } from '../../core/dom.js';
import { invoke } from '../../core/tauri.js';
import { showToast } from '../../core/toast.js';

const setSub = (id: string, text: string): void => setText(id.replace(/^#/, ''), text);

const errMsg = (err: unknown): string => {
  if (err && typeof err === 'object' && 'message' in err && (err as { message?: unknown }).message) {
    return String((err as { message: unknown }).message);
  }
  return String(err || 'Something went wrong');
};

export interface AdbControllerDeps {
  isEnabled: () => boolean;
  getRunGen: () => number;
  getActiveDevice: () => AdbDevice | null;
  setActiveDevice: (dev: AdbDevice | null) => void;
  refreshStatus: () => Promise<void>;
  startMirror: (serial?: string | null) => Promise<void>;
}

export interface AdbController {
  guard: (btn: HTMLButtonElement | null, fn: () => Promise<unknown>) => Promise<void>;
  prefillSavedIp: () => Promise<void>;
  refreshDevices: () => Promise<void>;
  qrStop: () => void;
  reset: () => void;
  bindAdbControls: () => void;
}

export function createAdbController({
  isEnabled,
  getRunGen,
  getActiveDevice,
  setActiveDevice,
  refreshStatus,
  startMirror,
}: AdbControllerDeps): AdbController {
  const hiddenUsb = new Set<string>();
  let busy = false;
  let qrTimer: ReturnType<typeof setInterval> | undefined;

  async function guard(btn: HTMLButtonElement | null, fn: () => Promise<unknown>): Promise<void> {
    if (!isEnabled() || busy) return;
    busy = true;
    if (btn) btn.disabled = true;
    try {
      await fn();
    } catch (err) {
      if (isEnabled()) showToast(errMsg(err));
    } finally {
      busy = false;
      if (btn) btn.disabled = !isEnabled();
    }
  }

  async function prefillSavedIp(): Promise<void> {
    if (!isEnabled()) return;
    const gen = getRunGen();
    try {
      const saved = await invoke<SavedWirelessDevice>('sc_saved');
      if (!isEnabled() || gen !== getRunGen()) return;
      if (saved?.ip) {
        const ipEl = $<HTMLInputElement>('#sc-ip');
        const portEl = $<HTMLInputElement>('#sc-port');
        if (ipEl && !ipEl.value) ipEl.value = saved.ip;
        if (portEl && !portEl.value) portEl.value = String(saved.port || '5555');
        setSub('sc-ip-sub', `Saved phone: ${saved.model || saved.ip}:${saved.port || 5555}`);
      } else {
        setSub('sc-ip-sub', 'Filled in automatically');
      }
    } catch {
      /* ignore when no saved device */
    }
  }

  async function refreshDevices(): Promise<void> {
    if (!isEnabled()) return;
    const gen = getRunGen();
    try {
      let devs: AdbDevice[] = [];
      try {
        devs = (await invoke<AdbDevice[]>('sc_devices')) || [];
      } catch {
        devs = [];
      }
      if (!isEnabled() || gen !== getRunGen()) return;

      if (devs.length === 0) await prefillSavedIp();
      if (!isEnabled() || gen !== getRunGen()) return;

      const present = new Set(devs.map(d => d.serial));
      for (const serial of [...hiddenUsb]) {
        if (!present.has(serial)) hiddenUsb.delete(serial);
      }

      const connected = devs
        .filter(d => d.state === 'device' && !hiddenUsb.has(d.serial))
        .sort((a, b) => {
          if (!a.is_wireless && b.is_wireless) return -1;
          if (a.is_wireless && !b.is_wireless) return 1;
          if (a.serial.includes(':5555') && !b.serial.includes(':5555')) return -1;
          if (!a.serial.includes(':5555') && b.serial.includes(':5555')) return 1;
          return 0;
        });

      const rmBtn = $<HTMLButtonElement>('#sc-dev-remove');
      if (rmBtn) rmBtn.hidden = connected.length === 0;

      if (connected.length > 0) {
        const primary = connected[0];
        setActiveDevice(primary);
        const model = primary.model && primary.model !== 'Unknown' ? primary.model : 'Android Device';

        setText(
          'sc-dev-name',
          connected.length > 1 ? `${model} (${connected.length} connected)` : `${model} (Connected)`
        );
        setText(
          'sc-dev-sub',
          primary.is_wireless
            ? `Connected wirelessly via ADB (${primary.serial})`
            : `Connected via USB cable (${primary.serial})`
        );

        if (primary.is_wireless && primary.serial.includes(':')) {
          const [ip, port] = primary.serial.split(':');
          const ipEl = $<HTMLInputElement>('#sc-ip');
          const portEl = $<HTMLInputElement>('#sc-port');
          if (ipEl) ipEl.value = ip;
          if (portEl) portEl.value = port || '5555';
          setSub('sc-ip-sub', `Connected: ${ip}:${port || 5555}`);
        }

        const usbDev = connected.find(d => !d.is_wireless);
        setSub('sc-usb-sub', usbDev ? `${usbDev.model || 'Phone'} ready on USB` : 'Plug in your phone, then press Start');
        setSub('sc-perm-sub', 'One click while on USB, then stays connected');
      } else {
        setActiveDevice(null);
        let active = false;
        try {
          active = Boolean(await invoke<boolean>('sc_active'));
        } catch {
          active = false;
        }
        if (!isEnabled() || gen !== getRunGen()) return;
        setText('sc-dev-name', active ? 'Phone (Mirror active)' : 'No phone connected');
        setText(
          'sc-dev-sub',
          active ? 'Phone screen is currently mirrored to PC' : 'Connect via USB cable or Wi-Fi IP/Port'
        );
        setSub('sc-usb-sub', 'Plug in your phone, then press Start');
        setSub('sc-perm-sub', 'One click while on USB, then stays connected');
      }

      await refreshStatus();
    } catch (err) {
      console.error('refreshDevices failed:', err);
    }
  }

  function qrStop(): void {
    if (qrTimer !== undefined) {
      clearInterval(qrTimer);
      qrTimer = undefined;
    }
    const row = $('#sc-qr-row');
    const box = $('#sc-qr-box');
    if (row) row.hidden = true;
    if (box) box.innerHTML = '';
  }

  function reset(): void {
    hiddenUsb.clear();
    busy = false;
    qrStop();
  }

  function bindAdbControls(): void {
    const devRefreshBtn = $<HTMLButtonElement>('#sc-dev-refresh');
    devRefreshBtn?.addEventListener('click', () => {
      void guard(devRefreshBtn, async () => {
        await refreshDevices();
        showToast('Device list updated');
      });
    });

    const devRemoveBtn = $<HTMLButtonElement>('#sc-dev-remove');
    devRemoveBtn?.addEventListener('click', () => {
      void guard(devRemoveBtn, async () => {
        const dev = getActiveDevice();
        if (!dev) return;
        await invoke('sc_remove_device', { serial: dev.serial });
        if (!dev.is_wireless) hiddenUsb.add(dev.serial);
        else {
          const ipEl = $<HTMLInputElement>('#sc-ip');
          if (ipEl) ipEl.value = '';
          setSub('sc-ip-sub', 'Filled in automatically');
        }
        setActiveDevice(null);
        showToast(`${dev.model && dev.model !== 'Unknown' ? dev.model : 'Device'} removed`);
        await refreshDevices();
      });
    });

    const ipEl = $<HTMLInputElement>('#sc-ip');
    const portEl = $<HTMLInputElement>('#sc-port');
    ipEl?.addEventListener('input', () => {
      const val = ipEl.value.trim();
      if (val.includes(':')) {
        const [host, port] = val.split(':');
        ipEl.value = host.trim();
        if (port?.trim() && portEl) portEl.value = port.trim();
      }
    });

    $<HTMLButtonElement>('#sc-usb-go')?.addEventListener('click', e => {
      void guard(e.currentTarget as HTMLButtonElement, async () => {
        const devs = (await invoke<AdbDevice[]>('sc_devices').catch(() => [])) || [];
        const usb = devs.find(d => !d.is_wireless && d.state === 'device');
        if (!usb) {
          showToast('No USB phone found. Plug in phone and enable USB debugging.');
          return;
        }
        setActiveDevice(usb);
        await startMirror(usb.serial);
      });
    });

    $<HTMLButtonElement>('#sc-ip-go')?.addEventListener('click', e => {
      void guard(e.currentTarget as HTMLButtonElement, async () => {
        const ip = ($<HTMLInputElement>('#sc-ip')?.value || '').trim();
        const port = parseInt($<HTMLInputElement>('#sc-port')?.value || '5555', 10) || 5555;
        if (!ip) {
          showToast('Enter an IP address first');
          return;
        }
        if (!/^(\d{1,3}\.){3}\d{1,3}$/.test(ip)) {
          showToast('Invalid IP format (e.g. 192.168.1.10)');
          return;
        }
        showToast(`Connecting to ${ip}:${port}...`);
        const serial = await invoke<string>('sc_connect', { ip, port });
        await refreshDevices();
        await startMirror(serial);
      });
    });

    $<HTMLButtonElement>('#sc-perm-go')?.addEventListener('click', e => {
      void guard(e.currentTarget as HTMLButtonElement, async () => {
        const devs = (await invoke<AdbDevice[]>('sc_devices').catch(() => [])) || [];
        const dev = devs.find(x => x.state === 'device');
        if (!dev) {
          showToast('Plug in your phone with USB first');
          return;
        }
        setSub('sc-perm-sub', 'Configuring permanent Wi-Fi connection...');
        try {
          const cfg = await invoke<SavedWirelessDevice>('sc_make_permanent', { serial: dev.serial });
          if (cfg) {
            const scIp = $<HTMLInputElement>('#sc-ip');
            const scPort = $<HTMLInputElement>('#sc-port');
            if (scIp) scIp.value = cfg.ip;
            if (scPort) scPort.value = String(cfg.port);
            setSub('sc-perm-sub', `Locked to ${cfg.ip}:${cfg.port}. You can unplug USB now.`);
            showToast(`Permanent Wi-Fi setup done (${cfg.ip}:${cfg.port})`);
          }
          await refreshDevices();
        } catch (err) {
          setSub('sc-perm-sub', 'Failed. Re-plug USB and try again.');
          throw err;
        }
      });
    });

    $('#sc-qr-cancel')?.addEventListener('click', () => {
      invoke('sc_qr_cancel').catch(() => {});
      qrStop();
    });

    $('#sc-qr-go')?.addEventListener('click', async () => {
      if (!isEnabled()) return;
      try {
        const svg = await invoke<string>('sc_qr_start');
        if (!svg || !isEnabled()) return;
        const box = $('#sc-qr-box');
        const msg = $('#sc-qr-msg');
        const row = $('#sc-qr-row');
        if (box) box.innerHTML = svg;
        if (msg) msg.textContent = 'Scan via phone Wireless debugging';
        if (row) row.hidden = false;

        if (qrTimer !== undefined) clearInterval(qrTimer);
        qrTimer = setInterval(async () => {
          if (!isEnabled()) {
            qrStop();
            return;
          }
          let result: [string, string] | null;
          try {
            result = await invoke<[string, string]>('sc_qr_status');
          } catch {
            return;
          }
          if (!isEnabled()) {
            qrStop();
            return;
          }
          if (!Array.isArray(result)) return;
          const [state, detail] = result;
          if (state === 'waiting' && msg) {
            msg.textContent = 'Scan via phone Wireless debugging';
          } else if (state === 'pairing' && msg) {
            msg.textContent = detail;
          } else if (state === 'done') {
            qrStop();
            showToast('Device paired successfully!');
            await refreshDevices();
            await startMirror(detail);
          } else if (state === 'error') {
            qrStop();
            showToast(detail);
          }
        }, 1000);
      } catch (err) {
        showToast(errMsg(err));
      }
    });
  }

  return {
    guard,
    prefillSavedIp,
    refreshDevices,
    qrStop,
    reset,
    bindAdbControls,
  };
}
