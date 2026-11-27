import type { ScrcpyPrefs } from '../../../types/models.js';
import { $, $$ } from '../../core/dom.js';
import { invoke } from '../../core/tauri.js';
import { modState } from '../../core/state.js';
import { showToast } from '../../core/toast.js';

export const DEFAULT_SCRCPY_PREFS: Readonly<ScrcpyPrefs> = Object.freeze({
  res: '1080',
  bitrate: '8',
  fps: '60',
  screenOff: false,
  audioEnabled: true,
  audioBuf: '120',
  audioCodec: 'auto',
  audioDup: false,
});

export const VALID_SCRCPY_PREFS: Readonly<Record<'res' | 'bitrate' | 'fps' | 'audioBuf' | 'audioCodec', readonly string[]>> =
  Object.freeze({
    res: ['720', '1080', 'orig'],
    bitrate: ['4', '8', '16'],
    fps: ['60', '120'],
    audioBuf: ['40', '120', '250'],
    audioCodec: ['auto', 'opus', 'aac'],
  });

export interface RawScrcpyPrefsInput {
  res?: string;
  bitrate?: string;
  fps?: string;
  audioEnabled?: boolean;
  audioBuf?: string;
  audioCodec?: string;
  screenOff?: boolean;
  turnScreenOff?: boolean;
  audioDup?: boolean;
  maxSize?: number;
  maxFps?: number;
  bitrateMbps?: number;
  audioBufferMs?: number;
}

export function normalizeScrcpyPrefs(raw: unknown): ScrcpyPrefs {
  if (!raw || typeof raw !== 'object') return { ...DEFAULT_SCRCPY_PREFS };
  const r = raw as RawScrcpyPrefsInput;
  const res =
    r.res ||
    (r.maxSize === 1280 ? '720' : r.maxSize === 1920 ? '1080' : r.maxSize === 0 ? 'orig' : undefined);
  const fps = r.fps || (r.maxFps ? String(r.maxFps) : undefined);
  const bitrate = r.bitrate || (r.bitrateMbps ? String(r.bitrateMbps) : undefined);
  const audioBuf = r.audioBuf || (r.audioBufferMs ? String(r.audioBufferMs) : undefined);
  const audioCodec = r.audioCodec || undefined;
  const screenOff =
    r.screenOff !== undefined
      ? Boolean(r.screenOff)
      : r.turnScreenOff !== undefined
        ? Boolean(r.turnScreenOff)
        : DEFAULT_SCRCPY_PREFS.screenOff;
  const audioEnabled =
    r.audioEnabled !== undefined ? Boolean(r.audioEnabled) : DEFAULT_SCRCPY_PREFS.audioEnabled;
  const audioDup = r.audioDup !== undefined ? Boolean(r.audioDup) : DEFAULT_SCRCPY_PREFS.audioDup;

  return {
    res: (VALID_SCRCPY_PREFS.res.includes(String(res))
      ? String(res)
      : DEFAULT_SCRCPY_PREFS.res) as ScrcpyPrefs['res'],
    bitrate: (VALID_SCRCPY_PREFS.bitrate.includes(String(bitrate))
      ? String(bitrate)
      : DEFAULT_SCRCPY_PREFS.bitrate) as ScrcpyPrefs['bitrate'],
    fps: (VALID_SCRCPY_PREFS.fps.includes(String(fps))
      ? String(fps)
      : DEFAULT_SCRCPY_PREFS.fps) as ScrcpyPrefs['fps'],
    audioEnabled,
    audioBuf: (VALID_SCRCPY_PREFS.audioBuf.includes(String(audioBuf))
      ? String(audioBuf)
      : DEFAULT_SCRCPY_PREFS.audioBuf) as ScrcpyPrefs['audioBuf'],
    audioCodec: (VALID_SCRCPY_PREFS.audioCodec.includes(String(audioCodec))
      ? String(audioCodec)
      : DEFAULT_SCRCPY_PREFS.audioCodec) as ScrcpyPrefs['audioCodec'],
    screenOff,
    audioDup,
  };
}

export function buildStartOptions(prefs: ScrcpyPrefs, serial?: string | null): Record<string, unknown> {
  return {
    serial: serial || null,
    maxSize: prefs.res === '720' ? 1280 : prefs.res === '1080' ? 1920 : 0,
    maxFps: parseInt(prefs.fps, 10) || 60,
    bitrateMbps: parseInt(prefs.bitrate, 10) || 8,
    turnScreenOff: Boolean(prefs.screenOff),
    audioEnabled: prefs.audioEnabled !== false,
    audioBufferMs: parseInt(prefs.audioBuf, 10) || 120,
    audioCodec: prefs.audioCodec || 'auto',
    audioDup: Boolean(prefs.audioDup),
  };
}

export function paintScrcpyPrefs(prefs: ScrcpyPrefs): void {
  const normalized = normalizeScrcpyPrefs(prefs);
  for (const key of Object.keys(VALID_SCRCPY_PREFS) as Array<keyof typeof VALID_SCRCPY_PREFS>) {
    const seg = $(`.seg[data-pref="${key}"]`);
    if (!seg) continue;
    $$<HTMLButtonElement>('.sg', seg).forEach(btn => {
      btn.classList.toggle('on', btn.dataset.v === normalized[key]);
    });
  }
  for (const key of ['screenOff', 'audioEnabled', 'audioDup'] as const) {
    const input = $<HTMLInputElement>(`input[data-pref="${key}"]`);
    if (input) input.checked = Boolean(normalized[key]);
  }
}

export function bindScrcpyPrefControls(
  getPrefs: () => ScrcpyPrefs,
  setPrefs: (next: ScrcpyPrefs) => void,
  getSerial?: () => string | null | undefined
): void {
  const persist = (next: ScrcpyPrefs): void => {
    const clean = normalizeScrcpyPrefs(next);
    setPrefs(clean);
    paintScrcpyPrefs(clean);
    invoke('sc_save_prefs', { prefs: clean }).catch(() => {});
    invoke('sc_live_update', { opts: buildStartOptions(clean, getSerial?.()) }).catch(() => {});
  };

  for (const key of Object.keys(VALID_SCRCPY_PREFS) as Array<keyof typeof VALID_SCRCPY_PREFS>) {
    const seg = $(`.seg[data-pref="${key}"]`);
    if (!seg) continue;
    $$<HTMLButtonElement>('.sg', seg).forEach(btn => {
      btn.addEventListener('click', e => {
        e.preventDefault();
        e.stopPropagation();
        if (!modState.scrcpy?.enabled) return;
        const val = btn.dataset.v || '';
        if (!VALID_SCRCPY_PREFS[key].includes(val)) return;
        persist({ ...getPrefs(), [key]: val } as ScrcpyPrefs);
        showToast('Stream setting updated');
      });
    });
  }

  for (const key of ['screenOff', 'audioEnabled', 'audioDup'] as const) {
    const input = $<HTMLInputElement>(`input[data-pref="${key}"]`);
    if (!input) continue;
    input.addEventListener('change', () => {
      if (!modState.scrcpy?.enabled) return;
      persist({ ...getPrefs(), [key]: Boolean(input.checked) });
      showToast('Stream setting updated');
    });
  }
}
