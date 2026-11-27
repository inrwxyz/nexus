/**
 * Lightweight typed DOM query and manipulation helpers.
 */

export const $ = <T extends Element = HTMLElement>(
  selector: string,
  root: ParentNode = document
): T | null => root.querySelector<T>(selector);

export const $$ = <T extends Element = HTMLElement>(
  selector: string,
  root: ParentNode = document
): T[] => [...root.querySelectorAll<T>(selector)];

export function setText(id: string, value: string): void {
  const el = document.getElementById(id);
  if (el) el.textContent = value;
}
