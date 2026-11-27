import { describe, expect, it } from 'vitest';
import { formatKeyEventCombo } from '../ui/js/features/circle-search/index.js';

describe('circle to search hotkey parser', () => {
  it('formats modifier + key combos consistently', () => {
    expect(
      formatKeyEventCombo({
        key: 's',
        code: 'KeyS',
        ctrlKey: true,
        shiftKey: true,
        altKey: false,
        metaKey: false,
      })
    ).toEqual({ combo: 'Ctrl+Shift+S' });
  });

  it('allows standalone F-keys and ignores bare modifier presses', () => {
    expect(
      formatKeyEventCombo({
        key: 'F9',
        code: 'F9',
        ctrlKey: false,
        shiftKey: false,
        altKey: false,
        metaKey: false,
      })
    ).toEqual({ combo: 'F9' });
    expect(
      formatKeyEventCombo({
        key: 'Control',
        code: 'ControlLeft',
        ctrlKey: true,
        shiftKey: false,
        altKey: false,
        metaKey: false,
      })
    ).toBeNull();
  });

  it('rejects unmodified letters', () => {
    const res = formatKeyEventCombo({
      key: 'a',
      code: 'KeyA',
      ctrlKey: false,
      shiftKey: false,
      altKey: false,
      metaKey: false,
    });
    expect(res?.error).toContain('modifier');
  });
});
