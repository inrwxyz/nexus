import type { TauriAppWindow, TauriGlobal } from '../../types/global.d.js';

export const T: TauriGlobal | null = typeof window !== 'undefined' ? (window.__TAURI__ ?? null) : null;

export const hasTauri = (): boolean => Boolean(T?.core?.invoke);

export async function invoke<R = unknown>(
  cmd: string,
  args?: Record<string, unknown>
): Promise<R | null> {
  if (!T?.core?.invoke) return null;
  return T.core.invoke<R>(cmd, args);
}

export async function listen<P = unknown>(
  event: string,
  handler: (event: { payload: P }) => void
): Promise<() => void> {
  if (!T?.event?.listen) return () => {};
  return T.event.listen<P>(event, handler);
}

export function getAppWindow(): TauriAppWindow | null {
  return T?.window?.getCurrentWindow ? T.window.getCurrentWindow() : null;
}
