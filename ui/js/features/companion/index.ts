import { $, setText } from '../../core/dom.js';
import { emit, on } from '../../core/bus.js';
import { invoke, listen } from '../../core/tauri.js';
import { compState, navState } from '../../core/state.js';
import { showToast } from '../../core/toast.js';
import { saveSettings } from '../../core/settings-store.js';

interface PairingInfo {
  paired?: boolean;
  device?: string;
  paired_device_name?: string;
  qr_svg?: string;
}

let qrRefreshTimer: ReturnType<typeof setInterval> | undefined;
let qrVisible = false;
let lastQrSvg = '';

function syncCompanionControls(): void {
  const toggleVoice = $<HTMLInputElement>('#toggle-comp-voice');
  const toggleIsland = $<HTMLInputElement>('#toggle-comp-island');
  const toggleTrackpad = $<HTMLInputElement>('#toggle-comp-trackpad');
  const toggleInertia = $<HTMLInputElement>('#toggle-comp-inertia');
  const toggleTerminal = $<HTMLInputElement>('#toggle-comp-terminal');
  const sliderSens = $<HTMLInputElement>('#comp-sens-slider');

  if (toggleVoice) toggleVoice.checked = compState.voice_relay !== false;
  if (toggleIsland) toggleIsland.checked = compState.show_on_island !== false;
  if (toggleTrackpad) toggleTrackpad.checked = compState.touchpad !== false;
  if (toggleInertia) toggleInertia.checked = compState.inertia_scrolling !== false;
  if (toggleTerminal) toggleTerminal.checked = Boolean(compState.remote_terminal);

  const sens = Number.isFinite(Number(compState.sensitivity)) ? Number(compState.sensitivity) : 1.2;
  if (sliderSens) sliderSens.value = String(sens);
  setText('comp-sens-val', `${sens.toFixed(1)}x`);
}

function setQrVisibility(visible: boolean, svg = ''): void {
  const idleEl = $('#comp-qr-idle');
  const activeEl = $('#comp-qr-active');
  const compQr = $('#comp-qr');
  qrVisible = visible && Boolean(svg || lastQrSvg);
  if (qrVisible) {
    if (svg && compQr && svg !== lastQrSvg) {
      lastQrSvg = svg;
      compQr.innerHTML = svg;
    }
    if (idleEl) idleEl.hidden = true;
    if (activeEl) activeEl.hidden = false;
  } else {
    lastQrSvg = '';
    if (compQr) compQr.innerHTML = '';
    if (activeEl) activeEl.hidden = true;
    if (idleEl) idleEl.hidden = false;
  }
}

function stopPairingPoll(): void {
  if (qrRefreshTimer !== undefined) {
    clearInterval(qrRefreshTimer);
    qrRefreshTimer = undefined;
  }
}

function startPairingPoll(): void {
  if (qrRefreshTimer !== undefined) return;
  qrRefreshTimer = setInterval(() => {
    if (!document.hidden && qrVisible && !$('#row-comp-unpaired')?.hidden) {
      void refreshCompanionPairingUI();
    }
  }, 2500);
}

function renderPairingCard(info: PairingInfo): void {
  const rowUnpaired = $('#row-comp-unpaired');
  const rowPaired = $('#row-comp-paired');
  const wasPaired = Boolean(compState.paired);
  compState.paired = Boolean(info.paired);
  if (info.paired) {
    compState.paired_device =
      info.device || info.paired_device_name || compState.paired_device || null;
  } else {
    compState.paired_device = null;
    compState.auth_token = null;
  }

  if (rowUnpaired && rowPaired) {
    if (info.paired) {
      rowUnpaired.hidden = true;
      rowPaired.hidden = false;
      setText('comp-device-label', info.device || info.paired_device_name || 'Paired Device');
      setQrVisibility(false);
      stopPairingPoll();
    } else {
      rowUnpaired.hidden = false;
      rowPaired.hidden = true;
      if (qrVisible && info.qr_svg) {
        setQrVisibility(true, info.qr_svg);
      } else if (!qrVisible) {
        setQrVisibility(false);
      }
    }
  }

  if (wasPaired !== compState.paired) {
    emit('companion:pairing-changed', { paired: compState.paired });
  }
}

export async function refreshCompanionPairingUI(): Promise<void> {
  try {
    let info = await invoke<PairingInfo>('get_companion_pairing_info');
    if (!info) {
      const res = await fetch('/api/companion/status').catch(() => null);
      if (res?.ok) info = (await res.json()) as PairingInfo;
    }
    if (info) renderPairingCard(info);
  } catch {
    /* ignore when standalone offline */
  }
}

async function generatePairingQr(): Promise<void> {
  try {
    let info = await invoke<PairingInfo>('generate_companion_qr');
    if (!info) {
      const res = await fetch('/api/companion/generate_code', { method: 'POST' }).catch(() => null);
      if (res?.ok) info = (await res.json()) as PairingInfo;
    }
    if (info?.paired) {
      renderPairingCard(info);
      return;
    }
    if (info?.qr_svg) {
      setQrVisibility(true, info.qr_svg);
      startPairingPoll();
    } else {
      showToast('Could not generate QR code');
    }
  } catch {
    showToast('Could not generate QR code');
  }
}

function updatePairingPolling(): void {
  const page = $('#page-companion-pairing');
  const active = navState.curModule === 'companion' && page?.classList.contains('active');

  if (active) {
    void refreshCompanionPairingUI();
    if (qrVisible) startPairingPoll();
  } else {
    stopPairingPoll();
    if (qrVisible) setQrVisibility(false);
  }
}

export function initCompanionFeature(): void {
  const toggleVoice = $<HTMLInputElement>('#toggle-comp-voice');
  const toggleIsland = $<HTMLInputElement>('#toggle-comp-island');
  const toggleTrackpad = $<HTMLInputElement>('#toggle-comp-trackpad');
  const toggleInertia = $<HTMLInputElement>('#toggle-comp-inertia');
  const toggleTerminal = $<HTMLInputElement>('#toggle-comp-terminal');
  const sliderSens = $<HTMLInputElement>('#comp-sens-slider');

  $('#btn-comp-gen-qr')?.addEventListener('click', () => {
    void generatePairingQr();
  });
  $('#btn-comp-regen-qr')?.addEventListener('click', () => {
    void generatePairingQr();
  });
  $('#btn-comp-hide-qr')?.addEventListener('click', () => {
    stopPairingPoll();
    setQrVisibility(false);
  });

  $('#btn-comp-unpair')?.addEventListener('click', async () => {
    try {
      const res = await invoke<boolean>('unpair_companion_device');
      if (res === null) {
        await fetch('/api/companion/unpair', { method: 'POST' }).catch(() => {});
      }
      showToast('Device unpaired');
      await refreshCompanionPairingUI();
    } catch (err) {
      console.error('Unpair failed:', err);
    }
  });

  toggleVoice?.addEventListener('change', () => {
    compState.voice_relay = toggleVoice.checked;
    void saveSettings(`Voice relay ${toggleVoice.checked ? 'on' : 'off'}`);
  });

  toggleIsland?.addEventListener('change', () => {
    compState.show_on_island = toggleIsland.checked;
    void saveSettings(`Phone commands on Island ${toggleIsland.checked ? 'on' : 'off'}`);
  });

  toggleTrackpad?.addEventListener('change', () => {
    compState.touchpad = toggleTrackpad.checked;
    void saveSettings(`Trackpad ${toggleTrackpad.checked ? 'on' : 'off'}`);
  });

  toggleInertia?.addEventListener('change', () => {
    compState.inertia_scrolling = toggleInertia.checked;
    void saveSettings(`Inertia scrolling ${toggleInertia.checked ? 'on' : 'off'}`);
  });

  const termModal = $('#comp-term-modal');
  const termAck = $<HTMLInputElement>('#comp-term-ack');
  const btnTermCancel = $('#btn-term-cancel');
  const btnTermGrant = $<HTMLButtonElement>('#btn-term-grant');

  const closeTermModal = (): void => {
    if (termModal) termModal.hidden = true;
    if (termAck) termAck.checked = false;
    if (btnTermGrant) btnTermGrant.disabled = true;
    if (toggleTerminal) toggleTerminal.checked = Boolean(compState.remote_terminal);
  };

  toggleTerminal?.addEventListener('change', () => {
    if (toggleTerminal.checked) {
      toggleTerminal.checked = false;
      if (termAck) termAck.checked = false;
      if (btnTermGrant) btnTermGrant.disabled = true;
      if (termModal) termModal.hidden = false;
    } else {
      compState.remote_terminal = false;
      void saveSettings('Remote terminal access disabled');
    }
  });

  termAck?.addEventListener('change', () => {
    if (btnTermGrant) btnTermGrant.disabled = !termAck.checked;
  });

  btnTermCancel?.addEventListener('click', closeTermModal);
  termModal?.addEventListener('click', ev => {
    if (ev.target === termModal) closeTermModal();
  });

  btnTermGrant?.addEventListener('click', () => {
    if (!termAck?.checked) return;
    compState.remote_terminal = true;
    if (toggleTerminal) toggleTerminal.checked = true;
    if (termModal) termModal.hidden = true;
    void saveSettings('Remote terminal access granted');
  });

  void listen<{ paired?: boolean }>('companion-pairing-changed', ev => {
    const p = ev?.payload;
    if (!p) return;
    if (!p.paired) {
      showToast('Phone unpaired');
    }
    void refreshCompanionPairingUI();
    emit('companion:pairing-changed', { paired: Boolean(p.paired) });
  });

  sliderSens?.addEventListener('input', () => {
    compState.sensitivity = parseFloat(sliderSens.value) || 1.2;
    setText('comp-sens-val', `${compState.sensitivity.toFixed(1)}x`);
  });

  sliderSens?.addEventListener('change', () => {
    void saveSettings('Trackpad sensitivity updated');
  });

  on('settings:sync', () => {
    syncCompanionControls();
    void refreshCompanionPairingUI();
  });
  on('module:changed', updatePairingPolling);
  on('tab:changed', updatePairingPolling);

  syncCompanionControls();
}
