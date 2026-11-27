import { $ } from './dom.js';

let hideTimer: ReturnType<typeof setTimeout> | undefined;

export interface ToastOptions {
  error?: boolean;
  durationMs?: number;
}

export function showToast(msg = 'Saved', opts: number | ToastOptions = 2000): void {
  const toast = $('#save-toast');
  const label = $('#save-toast-msg');
  if (!toast || !label) return;
  const clean = String(msg || '').trim() || 'Saved';
  const isErr =
    typeof opts === 'object' && opts !== null
      ? Boolean(opts.error)
      : /^(HTTP \d+|failed|error|connection timed out|connection refused|dns |invalid )/i.test(clean);
  const durationMs =
    typeof opts === 'number' ? opts : (opts?.durationMs ?? (isErr ? 4200 : 2000));

  label.textContent = clean;
  toast.classList.toggle('err', isErr);
  toast.classList.add('show');
  if (hideTimer !== undefined) clearTimeout(hideTimer);
  hideTimer = setTimeout(() => toast.classList.remove('show'), durationMs);
}
