import type { CircleOverlayInitInfo, PhysicalBox, TranslateLine } from '../../../types/models.js';
import { invoke } from '../../core/tauri.js';
import { positionBesideButton } from './overlay-canvas.js';

interface TranslatorDom {
  trLayer: HTMLElement;
  popLang: HTMLSelectElement;
  card: HTMLElement;
  toolTranslate: HTMLElement;
  langBadge: HTMLElement;
  langPill: HTMLElement;
  railWrap: HTMLElement;
}

interface TranslatorCallbacks {
  showToast: (msg: string, durationMs?: number) => void;
  showChip: (anchorBtn: HTMLElement, text: string) => void;
  hideChip: () => void;
  cleanupResources: () => void;
  isClosing: () => boolean;
  setLastTranslatePhys: (phys: PhysicalBox) => void;
}

interface TranslateSelectionResult {
  ok?: boolean;
  error?: string;
  lines?: TranslateLine[];
}

export function shortLangLabel(code?: string): string {
  return (code || 'en').split('-')[0].toUpperCase() + ' ▾';
}

export function createOverlayTranslator(dom: TranslatorDom, callbacks: TranslatorCallbacks) {
  let reqSeq = 0;

  function clearInPlace(): void {
    dom.trLayer.textContent = '';
  }

  function renderLines(
    lines: ReadonlyArray<TranslateLine>,
    initInfo: Pick<CircleOverlayInitInfo, 'width' | 'height'>
  ): void {
    clearInPlace();
    const sx = window.innerWidth / Math.max(1, initInfo.width);
    const sy = window.innerHeight / Math.max(1, initInfo.height);

    const rects = lines.map(item => {
      const cssX = Math.max(0, item.x * sx - 4);
      const cssY = Math.max(0, item.y * sy - 3);
      const cssW = Math.max(20, item.width * sx + 8);
      const cssH = Math.max(18, item.height * sy + 6);
      return { item, cssX, cssY, cssW, cssH, cy: cssY + cssH / 2 };
    });

    for (let i = 0; i < rects.length; i++) {
      const cur = rects[i];
      let rightBound = window.innerWidth - 8;
      for (let j = 0; j < rects.length; j++) {
        if (i === j) continue;
        const other = rects[j];
        const sameRow = Math.abs(cur.cy - other.cy) < Math.max(cur.cssH, other.cssH) * 0.65;
        if (sameRow && other.cssX > cur.cssX + cur.cssW * 0.45) {
          rightBound = Math.min(rightBound, other.cssX - 4);
        }
      }
      const extraBudget = Math.min(88, Math.max(22, cur.cssW * 0.45));
      const maxW = Math.max(cur.cssW, Math.min(rightBound - cur.cssX, cur.cssW + extraBudget));

      const el = document.createElement('div');
      el.className = 'tr-box';
      el.title = `${cur.item.source_text}\nClick to copy`;
      el.textContent = cur.item.translated_text;
      el.style.left = `${cur.cssX.toFixed(1)}px`;
      el.style.top = `${cur.cssY.toFixed(1)}px`;
      el.style.minWidth = `${cur.cssW.toFixed(1)}px`;
      el.style.maxWidth = `${maxW.toFixed(1)}px`;
      el.style.minHeight = `${cur.cssH.toFixed(1)}px`;
      el.style.background = cur.item.bg_color || 'rgb(15, 19, 25)';
      el.style.color = cur.item.fg_color || '#f8fafc';

      let fs = Math.max(10.0, Math.min(44, cur.item.height * sy * 0.78));
      el.style.fontSize = `${fs.toFixed(1)}px`;
      dom.trLayer.appendChild(el);

      if (el.scrollWidth > maxW + 1) {
        fs = Math.max(8.0, fs * (maxW / el.scrollWidth));
        el.style.fontSize = `${fs.toFixed(1)}px`;
      }

      el.addEventListener('click', async ev => {
        ev.stopPropagation();
        try {
          await invoke('copy_circle_text', { text: cur.item.translated_text });
          callbacks.showToast('Copied');
        } catch {
          /* ignore clipboard errors */
        }
      });
    }
  }

  async function runTranslate(phys: PhysicalBox, initInfo: CircleOverlayInitInfo): Promise<void> {
    const seq = ++reqSeq;
    const tl = dom.popLang.value || 'en';
    callbacks.setLastTranslatePhys(phys);
    dom.card.classList.remove('open');
    dom.toolTranslate.classList.add('busy');
    dom.langBadge.textContent = `${tl.split('-')[0].toUpperCase()} …`;
    positionBesideButton(dom.langPill, dom.railWrap, dom.toolTranslate);

    try {
      const res = await invoke<TranslateSelectionResult>('translate_selection', {
        x: phys.x,
        y: phys.y,
        width: phys.width,
        height: phys.height,
        targetLang: tl,
      });
      if (seq !== reqSeq || callbacks.isClosing()) return;
      if (!res || res.error || !res.ok || !res.lines || res.lines.length === 0) {
        clearInPlace();
        const msg =
          !res?.error || res.error.includes('No readable text')
            ? 'No local OCR text — click G-Translate ↗ for all languages'
            : res.error;
        callbacks.showToast(msg, 3400);
        return;
      }
      renderLines(res.lines, initInfo);
    } catch (err) {
      if (seq !== reqSeq || callbacks.isClosing()) return;
      clearInPlace();
      callbacks.showToast(`Translate error: ${err}`, 3400);
    } finally {
      if (seq === reqSeq) {
        dom.toolTranslate.classList.remove('busy');
        dom.langBadge.textContent = shortLangLabel(dom.popLang.value);
        positionBesideButton(dom.langPill, dom.railWrap, dom.toolTranslate);
      }
    }
  }

  async function launchGoogleTranslateWeb(phys?: PhysicalBox | null): Promise<void> {
    const p = phys || { x: 0, y: 0, width: 0, height: 0 };
    const tl = dom.popLang.value || 'bn';
    dom.toolTranslate.classList.add('busy');
    callbacks.showChip(dom.toolTranslate, 'Opening Google Translate…');
    try {
      await invoke('google_translate_image', {
        x: p.x,
        y: p.y,
        width: p.width,
        height: p.height,
        targetLang: tl,
      });
      callbacks.cleanupResources();
    } catch (err) {
      callbacks.hideChip();
      dom.toolTranslate.classList.remove('busy');
      callbacks.showToast(`Google Translate failed: ${err}`, 3400);
    }
  }

  return {
    clearInPlace,
    runTranslate,
    launchGoogleTranslateWeb,
  };
}
