import { describe, expect, it } from 'vitest';
import {
  DEFAULT_SCRCPY_PREFS,
  buildStartOptions,
  normalizeScrcpyPrefs,
} from '../ui/js/features/scrcpy/prefs.js';

describe('screen mirror prefs normalization & start options', () => {
  it('returns default prefs for invalid inputs', () => {
    expect(normalizeScrcpyPrefs(null)).toEqual(DEFAULT_SCRCPY_PREFS);
    expect(normalizeScrcpyPrefs({ res: '4k', fps: '999', bitrate: '99' })).toEqual(
      DEFAULT_SCRCPY_PREFS
    );
  });

  it('migrates legacy camelCase fields and builds expected backend payload', () => {
    const clean = normalizeScrcpyPrefs({
      maxSize: 1280,
      maxFps: 120,
      bitrateMbps: 16,
      turnScreenOff: true,
      audioBufferMs: 40,
      audioCodec: 'opus',
      audioDup: true,
    });
    expect(clean).toEqual({
      res: '720',
      fps: '120',
      bitrate: '16',
      screenOff: true,
      audioEnabled: true,
      audioBuf: '40',
      audioCodec: 'opus',
      audioDup: true,
    });
    expect(buildStartOptions(clean, '192.168.1.50:5555')).toEqual({
      serial: '192.168.1.50:5555',
      maxSize: 1280,
      maxFps: 120,
      bitrateMbps: 16,
      turnScreenOff: true,
      audioEnabled: true,
      audioBufferMs: 40,
      audioCodec: 'opus',
      audioDup: true,
    });
  });
});
