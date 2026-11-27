import { $, setText } from '../../core/dom.js';
import { on } from '../../core/bus.js';
import { invoke, listen } from '../../core/tauri.js';
import { showToast } from '../../core/toast.js';
import { cleanOptionLabel } from '../../core/navigation.js';
import { saveSettings } from '../../core/settings-store.js';
import { initVoiceAiFeature } from './ai.js';

interface VoiceEventPayload {
  type?: string;
  status?: string;
  text?: string;
}

let vfClearTimer: ReturnType<typeof setTimeout> | undefined;
let currentMicMuted = false;

function applyMicMuteUI(isMuted: boolean): void {
  currentMicMuted = Boolean(isMuted);
  $('#cap-mic')?.classList.toggle('muted', currentMicMuted);
  $('#cap-mic')?.classList.toggle('on', !currentMicMuted);
  setText('cap-mic-v', currentMicMuted ? 'Muted' : 'Active');

  $('#btn-mute-home')?.classList.toggle('muted', currentMicMuted);
  setText('btn-mute-home', currentMicMuted ? 'Unmute' : 'Mute');

  const statusEl = $('#vf-status');
  if (statusEl) {
    statusEl.textContent = currentMicMuted ? 'Muted' : 'Listening';
    statusEl.style.color = currentMicMuted ? 'var(--danger)' : '';
  }

  const toggleBtn = $('#btn-toggle-mic-voice');
  if (toggleBtn) {
    toggleBtn.textContent = currentMicMuted ? 'Unmute Mic' : 'Mute Mic';
    toggleBtn.style.color = currentMicMuted ? 'var(--danger)' : '';
  }

  const badgeEl = $('#voice-mic-badge');
  if (badgeEl) {
    badgeEl.innerHTML = currentMicMuted ? '<i></i>Muted' : '<i></i>Calibrated';
    badgeEl.style.color = currentMicMuted ? 'var(--danger)' : '';
  }

  setText(
    'voice-mic-sub',
    currentMicMuted
      ? 'Microphone is currently muted • Listening paused'
      : 'Realtek High Definition Audio · Snappy Wake'
  );
}

async function toggleMicMute(): Promise<void> {
  try {
    const res = await invoke<boolean>('toggle_mic_mute');
    if (res !== null) applyMicMuteUI(res);
    else applyMicMuteUI(!currentMicMuted);
  } catch (err) {
    console.error('toggleMicMute failed:', err);
  }
}

function resetVoiceLog(): void {
  const vfStatus = $('#vf-status');
  const vfLog = $('#vf-log');
  if (vfStatus && !currentMicMuted) {
    vfStatus.textContent = 'Listening';
    vfStatus.style.color = '';
  }
  if (vfLog) {
    const opt = $<HTMLSelectElement>('#sel-wakeword')?.selectedOptions?.[0];
    const label = opt ? cleanOptionLabel(opt) : 'Hey Google';
    vfLog.textContent = `Say "${label}" to begin...`;
    vfLog.style.color = 'var(--dim)';
  }
}

function resetVoiceLogIfIdle(): void {
  const vfLog = $('#vf-log');
  if (vfClearTimer === undefined && vfLog && /^Say "/.test(vfLog.textContent || '')) {
    resetVoiceLog();
  }
}

function syncVoiceCapsules(): void {
  const cornerOn = Boolean($<HTMLInputElement>('#toggle-corner')?.checked);
  const islandOn = Boolean($<HTMLInputElement>('#toggle-island')?.checked);
  const updaterOn = Boolean($<HTMLInputElement>('#toggle-updater')?.checked);

  $('#cap-corner')?.classList.toggle('on', cornerOn || islandOn);
  setText('cap-corner-v', islandOn ? 'Inex Island' : cornerOn ? 'On' : 'Off');

  $('#cap-updater')?.classList.toggle('on', updaterOn);
  setText('cap-updater-v', updaterOn ? 'On' : 'Off');
}

function cycleWakeWord(dir: number): void {
  const sel = $<HTMLSelectElement>('#sel-wakeword');
  if (!sel?.options?.length) return;
  const i = Math.max(0, sel.selectedIndex);
  const next = (i + dir + sel.options.length) % sel.options.length;
  sel.value = sel.options[next].value;
  sel.dispatchEvent(new Event('change'));
}

export function initVoiceFeature(): void {
  const selWake = $<HTMLSelectElement>('#sel-wakeword');
  const toggleCorner = $<HTMLInputElement>('#toggle-corner');
  const toggleIsland = $<HTMLInputElement>('#toggle-island');
  const toggleUpdater = $<HTMLInputElement>('#toggle-updater');

  $('#wake-prev')?.addEventListener('click', () => cycleWakeWord(-1));
  $('#wake-next')?.addEventListener('click', () => cycleWakeWord(1));
  document.addEventListener('keydown', e => {
    if ($('#page-voice-home')?.classList.contains('active')) {
      if (e.key === 'ArrowLeft') cycleWakeWord(-1);
      if (e.key === 'ArrowRight') cycleWakeWord(1);
    }
  });

  selWake?.addEventListener('change', () => {
    const opt = selWake.selectedOptions[0];
    if (opt) setText('wake-word', cleanOptionLabel(opt));
    resetVoiceLogIfIdle();
    syncVoiceCapsules();
    void saveSettings();
  });

  toggleCorner?.addEventListener('change', () => {
    if (toggleCorner.checked && toggleIsland) {
      toggleIsland.checked = false;
    }
    syncVoiceCapsules();
    void saveSettings(`Corner Pill ${toggleCorner.checked ? 'on' : 'off'}`);
  });

  toggleIsland?.addEventListener('change', async () => {
    if (toggleIsland.checked) {
      let installed = false;
      try {
        installed = Boolean(await invoke<boolean>('check_inex_island_installed'));
      } catch {
        installed = false;
      }
      if (!installed) {
        toggleIsland.checked = false;
        syncVoiceCapsules();
        showToast('Inex Island is not installed');
        return;
      }
      if (toggleCorner) toggleCorner.checked = false;
    }
    syncVoiceCapsules();
    void saveSettings(`Inex Island display ${toggleIsland.checked ? 'on' : 'off'}`);
  });

  $('#cap-corner')?.addEventListener('click', () => toggleCorner?.click());
  $('#cap-updater')?.addEventListener('click', () => toggleUpdater?.click());
  ['#cap-mic', '#btn-mute-home', '#btn-toggle-mic-voice'].forEach(sel => {
    $(sel)?.addEventListener('click', () => {
      void toggleMicMute();
    });
  });

  $('#vf-clear')?.addEventListener('click', () => {
    if (vfClearTimer !== undefined) {
      clearTimeout(vfClearTimer);
      vfClearTimer = undefined;
    }
    resetVoiceLog();
  });

  on('settings:sync', () => {
    syncVoiceCapsules();
    resetVoiceLogIfIdle();
  });

  void listen<VoiceEventPayload>('voice-event', ev => {
    const payload = ev?.payload;
    if (!payload) return;
    const vfStatus = $('#vf-status');
    const vfLog = $('#vf-log');
    if (vfStatus && payload.status && !currentMicMuted) {
      vfStatus.textContent = payload.status;
      vfStatus.style.color = '';
    }
    if (vfLog && payload.text) {
      vfLog.textContent = payload.text;
      vfLog.style.color =
        payload.type === 'action' || payload.type === 'wake' ? 'var(--accent)' : 'var(--text)';
    }
    if (vfClearTimer !== undefined) {
      clearTimeout(vfClearTimer);
      vfClearTimer = undefined;
    }
    if (payload.type === 'action') {
      vfClearTimer = setTimeout(() => {
        vfClearTimer = undefined;
        resetVoiceLog();
      }, 4200);
    }
  });

  void listen<{ muted?: boolean }>('mic-mute-changed', ev => {
    if (typeof ev?.payload?.muted === 'boolean') applyMicMuteUI(ev.payload.muted);
  });

  invoke<boolean>('get_mic_mute')
    .then(val => {
      if (typeof val === 'boolean') applyMicMuteUI(val);
    })
    .catch(() => {});

  syncVoiceCapsules();
  initVoiceAiFeature();
}
