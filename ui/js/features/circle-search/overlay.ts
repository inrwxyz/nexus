import type { CircleOverlayInitInfo, CssBox, PhysicalBox } from '../../../types/models.js';
import { $ } from '../../core/dom.js';
import { invoke } from '../../core/tauri.js';
import {
  anchorRailToPixels,
  bindRailDrag,
  boundingBoxFromPoints,
  cssBoxToPhysical,
  positionAskBar,
  positionBesideButton,
  positionResultCard,
  renderScrim,
  type Point2D,
} from './overlay-canvas.js';
import { createOverlayTranslator, shortLangLabel } from './overlay-translate.js';

const bg = $<HTMLCanvasElement>('#bg')!;
const draw = $<HTMLCanvasElement>('#draw')!;
const trLayer = $('#tr-layer')!;
const bgCtx = bg.getContext('2d')!;
const ctx = draw.getContext('2d')!;

const railWrap = $('#rail-wrap')!;
const railGrip = $('#rail-grip')!;
const toolLasso = $('#tool-lasso')!;
const toolTranslate = $('#tool-translate')!;
const toolMusic = $('#tool-music')!;
const langPill = $('#lang-pill')!;
const langBadge = $('#lang-badge')!;
const popLang = $<HTMLSelectElement>('#pop-lang')!;
const btnGTranslate = $('#btn-gtranslate')!;
const miniChip = $('#mini-chip')!;
const miniChipText = $('#mini-chip-text')!;

const card = $('#result-card')!;
const ocrBox = $<HTMLTextAreaElement>('#ocr-text')!;
const selSizeEl = $('#sel-size')!;
const btnCopy = $<HTMLButtonElement>('#btn-copy')!;
const btnWeb = $<HTMLButtonElement>('#btn-web')!;
const btnRedraw = $<HTMLButtonElement>('#btn-redraw')!;
const btnClose = $<HTMLButtonElement>('#btn-close-overlay')!;
const copyToast = $('#copy-toast')!;
const askBar = $('#ask-bar')!;
const askLens = $<HTMLButtonElement>('#ask-lens')!;
const askOcr = $<HTMLButtonElement>('#ask-ocr')!;
const askTranslate = $<HTMLButtonElement>('#ask-translate')!;
const askGTranslate = $<HTMLButtonElement>('#ask-gtranslate')!;
const askRedraw = $<HTMLButtonElement>('#ask-redraw')!;

let frameUrl: string | null = null;
let initInfo: CircleOverlayInitInfo = {
  width: window.innerWidth,
  height: window.innerHeight,
  default_action: 'ask',
  target_lang: 'bn',
  translate_mode: 'in_place',
};
let activeTool = 'ask';
let drawing = false;
let points: Point2D[] = [];
let activeBox: CssBox | null = null;
let activePhys: PhysicalBox | null = null;
let lastTranslatePhys: PhysicalBox = { x: 0, y: 0, width: 0, height: 0 };
let musicListening = false;
let closing = false;
let toastTimer: ReturnType<typeof setTimeout> | undefined;

function showToast(msg: string, durationMs = 1400): void {
  copyToast.textContent = msg;
  copyToast.classList.add('show');
  if (toastTimer !== undefined) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => copyToast.classList.remove('show'), durationMs);
}

function showChip(anchorBtn: HTMLElement, text: string): void {
  miniChipText.textContent = text;
  miniChip.classList.add('open');
  positionBesideButton(miniChip, railWrap, anchorBtn);
}

function hideChip(): void {
  miniChip.classList.remove('open');
}

function positionAllAttachments(): void {
  if (langPill.classList.contains('open')) positionBesideButton(langPill, railWrap, toolTranslate);
  if (miniChip.classList.contains('open')) {
    positionBesideButton(miniChip, railWrap, musicListening ? toolMusic : toolLasso);
  }
}

bindRailDrag(railWrap, railGrip, positionAllAttachments);

const translator = createOverlayTranslator(
  { trLayer, popLang, card, toolTranslate, langBadge, langPill, railWrap },
  {
    showToast,
    showChip,
    hideChip,
    cleanupResources,
    isClosing: () => closing,
    setLastTranslatePhys: phys => {
      lastTranslatePhys = phys;
    },
  }
);

function resizeCanvases(): void {
  const dpr = window.devicePixelRatio || 1;
  draw.width = Math.max(1, Math.round(window.innerWidth * dpr));
  draw.height = Math.max(1, Math.round(window.innerHeight * dpr));
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  renderScrim(ctx, activeBox, points);
}

function cleanupResources(): void {
  translator.clearInPlace();
  askBar.classList.remove('open');
  if (frameUrl) {
    URL.revokeObjectURL(frameUrl);
    frameUrl = null;
  }
  bg.width = 0;
  bg.height = 0;
  draw.width = 0;
  draw.height = 0;
}

async function closeOverlay(): Promise<void> {
  if (closing) return;
  closing = true;
  cleanupResources();
  try {
    await invoke('close_circle_search_overlay');
  } catch {
    /* ignore close error */
  }
}

function setTool(tool: string): void {
  activeTool = tool;
  document
    .querySelectorAll<HTMLElement>('.tool-btn[data-tool]')
    .forEach(b => b.classList.toggle('active', b.dataset.tool === tool));
  hideChip();
  askBar.classList.remove('open');

  if (tool === 'translate') {
    card.classList.remove('open');
    activeBox = null;
    activePhys = null;
    points = [];
    renderScrim(ctx, null, []);
    langBadge.textContent = shortLangLabel(popLang.value);
    langPill.classList.add('open');
    positionBesideButton(langPill, railWrap, toolTranslate);
    if (initInfo.translate_mode === 'google_web') {
      translator.clearInPlace();
      showToast('Draw area for Google Translate, or click G-Translate ↗ for full screen', 2800);
    } else {
      void translator.runTranslate({ x: 0, y: 0, width: 0, height: 0 }, initInfo);
    }
  } else {
    langPill.classList.remove('open');
    translator.clearInPlace();
    card.classList.remove('open');
  }
}

function resetSelection(): void {
  const hadBox = Boolean(activeBox);
  activeBox = null;
  activePhys = null;
  points = [];
  askBar.classList.remove('open');
  card.classList.remove('open');
  ocrBox.value = '';
  ocrBox.classList.add('empty');
  btnCopy.disabled = true;
  btnWeb.disabled = true;
  if (!(activeTool === 'translate' && !hadBox)) {
    lastTranslatePhys = { x: 0, y: 0, width: 0, height: 0 };
    translator.clearInPlace();
  }
  renderScrim(ctx, null, []);
}

async function runOcr(phys: PhysicalBox): Promise<void> {
  translator.clearInPlace();
  ocrBox.value = 'Extracting text offline…';
  ocrBox.classList.add('empty');
  btnCopy.disabled = true;
  btnWeb.disabled = true;
  try {
    const res = await invoke<{ error?: string; text?: string }>('process_circle_selection', {
      x: phys.x,
      y: phys.y,
      width: phys.width,
      height: phys.height,
      mode: 'ocr',
    });
    if (!res) return;
    if (res.error) {
      ocrBox.value = res.error;
      return;
    }
    const txt = (res.text || '').trim();
    if (txt) {
      ocrBox.value = txt;
      ocrBox.classList.remove('empty');
      btnCopy.disabled = false;
      btnWeb.disabled = false;
    } else {
      ocrBox.value = 'No text detected in this area.';
    }
  } catch (err) {
    ocrBox.value = `OCR failed: ${err}`;
  }
}

async function launchLens(phys: PhysicalBox | null): Promise<void> {
  if (!phys) return;
  toolLasso.classList.add('busy');
  showChip(toolLasso, 'Searching Google Lens…');
  try {
    await invoke('lens_visual_search', {
      x: phys.x,
      y: phys.y,
      width: phys.width,
      height: phys.height,
    });
    cleanupResources();
  } catch (err) {
    hideChip();
    toolLasso.classList.remove('busy');
    showToast(`Visual search failed: ${err}`);
  }
}

document.querySelectorAll<HTMLElement>('.tool-btn[data-tool]').forEach(btn => {
  btn.addEventListener('click', () => setTool(btn.dataset.tool || 'ask'));
});

popLang.addEventListener('change', () => {
  langBadge.textContent = shortLangLabel(popLang.value);
  positionBesideButton(langPill, railWrap, toolTranslate);
  if (activeTool === 'translate' && initInfo.translate_mode !== 'google_web') {
    void translator.runTranslate(lastTranslatePhys || { x: 0, y: 0, width: 0, height: 0 }, initInfo);
  }
});

btnGTranslate.addEventListener('click', () => {
  void translator.launchGoogleTranslateWeb(
    activePhys || lastTranslatePhys || { x: 0, y: 0, width: 0, height: 0 }
  );
});

toolMusic.addEventListener('click', async () => {
  if (musicListening || closing) return;
  musicListening = true;
  closing = true;
  askBar.classList.remove('open');
  translator.clearInPlace();
  card.classList.remove('open');
  langPill.classList.remove('open');
  hideChip();
  toolMusic.classList.add('busy');
  try {
    await invoke('recognize_desktop_music');
    cleanupResources();
  } catch (err) {
    closing = false;
    musicListening = false;
    toolMusic.classList.remove('busy');
    showChip(toolMusic, `Audio error: ${err}`);
    setTimeout(() => {
      if (!musicListening) hideChip();
    }, 2500);
  }
});

draw.addEventListener('pointerdown', e => {
  if (e.button !== 0 || closing) return;
  if (!musicListening) hideChip();
  askBar.classList.remove('open');
  card.classList.remove('open');
  if (activeTool !== 'translate') translator.clearInPlace();
  drawing = true;
  points = [{ x: e.clientX, y: e.clientY }];
  draw.setPointerCapture(e.pointerId);
  renderScrim(ctx, null, points);
});

draw.addEventListener('pointermove', e => {
  if (!drawing) return;
  const last = points[points.length - 1];
  if (!last || Math.hypot(e.clientX - last.x, e.clientY - last.y) >= 3) {
    points.push({ x: e.clientX, y: e.clientY });
    renderScrim(ctx, null, points);
  }
});

async function finishStroke(): Promise<void> {
  if (!drawing) return;
  drawing = false;
  if (points.length < 2) {
    resetSelection();
    return;
  }
  const box = boundingBoxFromPoints(points);
  if (box.w < 10 || box.h < 10) {
    resetSelection();
    return;
  }

  activeBox = box;
  activePhys = cssBoxToPhysical(box, initInfo);
  selSizeEl.textContent = `${activePhys.width} × ${activePhys.height}`;
  renderScrim(ctx, box, []);

  if (activeTool === 'translate') {
    if (initInfo.translate_mode === 'google_web') await translator.launchGoogleTranslateWeb(activePhys);
    else await translator.runTranslate(activePhys, initInfo);
  } else if (activeTool === 'ocr') {
    positionResultCard(card, box);
    await runOcr(activePhys);
  } else if (activeTool === 'lasso') {
    await launchLens(activePhys);
  } else {
    positionAskBar(askBar, box);
  }
}

draw.addEventListener('pointerup', () => {
  void finishStroke();
});
draw.addEventListener('pointercancel', () => {
  drawing = false;
  resetSelection();
});

askLens.addEventListener('click', async () => {
  if (!activePhys) return;
  askBar.classList.remove('open');
  await launchLens(activePhys);
});
askOcr.addEventListener('click', async () => {
  if (!activePhys || !activeBox) return;
  askBar.classList.remove('open');
  positionResultCard(card, activeBox);
  await runOcr(activePhys);
});
askTranslate.addEventListener('click', async () => {
  if (!activePhys) return;
  askBar.classList.remove('open');
  if (initInfo.translate_mode === 'google_web') {
    await translator.launchGoogleTranslateWeb(activePhys);
    return;
  }
  activeTool = 'translate';
  document
    .querySelectorAll<HTMLElement>('.tool-btn[data-tool]')
    .forEach(b => b.classList.toggle('active', b.dataset.tool === 'translate'));
  langBadge.textContent = shortLangLabel(popLang.value);
  langPill.classList.add('open');
  positionBesideButton(langPill, railWrap, toolTranslate);
  await translator.runTranslate(activePhys, initInfo);
});
askGTranslate.addEventListener('click', async () => {
  if (!activePhys) return;
  askBar.classList.remove('open');
  await translator.launchGoogleTranslateWeb(activePhys);
});
askRedraw.addEventListener('click', resetSelection);
btnClose.addEventListener('click', () => {
  void closeOverlay();
});
btnRedraw.addEventListener('click', resetSelection);

btnCopy.addEventListener('click', async () => {
  const txt = ocrBox.value.trim();
  if (!txt) return;
  try {
    await invoke('copy_circle_text', { text: txt });
    const orig = btnCopy.textContent;
    btnCopy.textContent = 'Copied!';
    setTimeout(() => {
      btnCopy.textContent = orig;
    }, 1300);
  } catch {
    /* ignore */
  }
});

btnWeb.addEventListener('click', async () => {
  const txt = ocrBox.value.trim();
  if (!txt) return;
  cleanupResources();
  try {
    await invoke('search_circle_text', { query: txt });
  } catch {
    /* ignore */
  }
});

document.addEventListener('keydown', e => {
  if (e.key === 'Escape') {
    e.preventDefault();
    void closeOverlay();
  }
});
window.addEventListener('resize', resizeCanvases);
window.addEventListener('beforeunload', cleanupResources);

void (async function boot() {
  resizeCanvases();
  try {
    const raw = await invoke<BlobPart>('get_frozen_frame');
    if (!raw) return;
    const blob = new Blob([raw], { type: 'image/bmp' });
    frameUrl = URL.createObjectURL(blob);
    const img = new Image();
    const done = async (): Promise<void> => {
      const ready = await invoke<CircleOverlayInitInfo>('overlay_ready');
      if (ready) initInfo = ready;
      if (initInfo?.target_lang && [...popLang.options].some(o => o.value === initInfo.target_lang)) {
        popLang.value = initInfo.target_lang;
      }
      langBadge.textContent = shortLangLabel(popLang.value);
      anchorRailToPixels(railWrap);
      if (initInfo?.default_action === 'translate') setTool('translate');
      else if (initInfo?.default_action === 'ocr') setTool('ocr');
      else if (initInfo?.default_action === 'lens') setTool('lasso');
      else setTool('ask');
    };
    img.onload = async () => {
      bg.width = img.naturalWidth || window.innerWidth;
      bg.height = img.naturalHeight || window.innerHeight;
      bgCtx.drawImage(img, 0, 0, bg.width, bg.height);
      if (frameUrl) {
        URL.revokeObjectURL(frameUrl);
        frameUrl = null;
      }
      await done();
    };
    img.onerror = () => {
      void done();
    };
    img.src = frameUrl;
  } catch (err) {
    console.error('Overlay init failed:', err);
    try {
      await invoke('overlay_ready');
    } catch {
      /* ignore */
    }
  }
})();
