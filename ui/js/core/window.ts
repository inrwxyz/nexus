import { $ } from './dom.js';
import { getAppWindow, invoke } from './tauri.js';

export function initWindowControls(): void {
  const appWin = getAppWindow();
  const winEl = $('#win');
  const btnMin = $<HTMLButtonElement>('#btn-min');
  const btnMax = $<HTMLButtonElement>('#btn-max');
  const btnClose = $<HTMLButtonElement>('#btn-close');

  async function refreshMaxState(): Promise<void> {
    if (!appWin) return;
    try {
      const maxed = await appWin.isMaximized();
      if (winEl) winEl.classList.toggle('maxed', maxed);
      if (btnMax) btnMax.title = maxed ? 'Restore' : 'Maximize';
    } catch {
      /* ignore when outside Tauri window */
    }
  }

  btnMin?.addEventListener('click', async () => {
    try {
      if (appWin) await appWin.minimize();
    } catch (err) {
      console.error('Minimize failed:', err);
    }
  });

  btnMax?.addEventListener('click', async () => {
    if (!appWin) return;
    try {
      const maxed = await appWin.isMaximized();
      if (maxed) await appWin.unmaximize();
      else await appWin.maximize();
      await refreshMaxState();
    } catch (err) {
      console.error('Maximize toggle failed:', err);
    }
  });

  btnClose?.addEventListener('click', async e => {
    e.stopPropagation();
    try {
      await invoke('close_settings');
    } catch (err) {
      console.error('Close settings failed:', err);
    }
  });

  if (appWin?.onResized) {
    appWin.onResized(() => {
      void refreshMaxState();
    }).catch(() => {});
  }
  void refreshMaxState();
}
