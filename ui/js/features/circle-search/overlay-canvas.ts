import type { CircleOverlayInitInfo, CssBox, PhysicalBox } from '../../../types/models.js';

export interface Point2D {
  x: number;
  y: number;
}

export function anchorRailToPixels(railWrap: HTMLElement): void {
  const r = railWrap.getBoundingClientRect();
  railWrap.style.left = `${Math.round(r.left)}px`;
  railWrap.style.top = `${Math.round(r.top)}px`;
  railWrap.style.right = 'auto';
  railWrap.style.transform = 'none';
}

export function positionBesideButton(
  el: HTMLElement,
  railWrap: HTMLElement,
  btnEl: HTMLElement
): void {
  const rr = railWrap.getBoundingClientRect();
  const br = btnEl.getBoundingClientRect();
  const ew = el.offsetWidth || 72;
  const eh = el.offsetHeight || 30;
  let left = rr.left - ew - 6;
  if (left < 8) left = Math.min(window.innerWidth - ew - 8, rr.right + 6);
  const top = Math.max(8, Math.min(window.innerHeight - eh - 8, br.top + (br.height - eh) / 2));
  el.style.left = `${Math.round(left)}px`;
  el.style.top = `${Math.round(top)}px`;
}

export function bindRailDrag(
  railWrap: HTMLElement,
  railGrip: HTMLElement,
  onMove?: () => void
): void {
  let dragging = false;
  let startX = 0;
  let startY = 0;
  let baseL = 0;
  let baseT = 0;

  railGrip.addEventListener('pointerdown', e => {
    e.preventDefault();
    e.stopPropagation();
    anchorRailToPixels(railWrap);
    const r = railWrap.getBoundingClientRect();
    dragging = true;
    startX = e.clientX;
    startY = e.clientY;
    baseL = r.left;
    baseT = r.top;
    railGrip.setPointerCapture(e.pointerId);
  });

  railGrip.addEventListener('pointermove', e => {
    if (!dragging) return;
    const r = railWrap.getBoundingClientRect();
    const nl = Math.max(8, Math.min(window.innerWidth - r.width - 8, baseL + (e.clientX - startX)));
    const nt = Math.max(8, Math.min(window.innerHeight - r.height - 8, baseT + (e.clientY - startY)));
    railWrap.style.left = `${Math.round(nl)}px`;
    railWrap.style.top = `${Math.round(nt)}px`;
    if (onMove) onMove();
  });

  const endDrag = (): void => {
    dragging = false;
  };
  railGrip.addEventListener('pointerup', endDrag);
  railGrip.addEventListener('pointercancel', endDrag);
}

export function renderScrim(
  ctx: CanvasRenderingContext2D,
  box: CssBox | null,
  pts: ReadonlyArray<Point2D>
): void {
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  ctx.clearRect(0, 0, vw, vh);
  ctx.fillStyle = box ? 'rgba(6, 10, 16, 0.44)' : 'rgba(6, 10, 16, 0.14)';
  ctx.fillRect(0, 0, vw, vh);

  if (box) {
    ctx.save();
    ctx.globalCompositeOperation = 'destination-out';
    ctx.beginPath();
    ctx.roundRect(box.x, box.y, box.w, box.h, 12);
    ctx.fill();
    ctx.restore();

    ctx.save();
    const grad = ctx.createLinearGradient(box.x, box.y, box.x + box.w, box.y + box.h);
    grad.addColorStop(0, '#67e8f9');
    grad.addColorStop(0.5, '#06b6d4');
    grad.addColorStop(1, '#3b82f6');
    ctx.strokeStyle = grad;
    ctx.lineWidth = 2.5;
    ctx.shadowColor = 'rgba(6, 182, 212, 0.75)';
    ctx.shadowBlur = 16;
    ctx.beginPath();
    ctx.roundRect(box.x, box.y, box.w, box.h, 12);
    ctx.stroke();
    ctx.restore();
  }

  if (pts && pts.length > 1) {
    ctx.save();
    const grad = ctx.createLinearGradient(
      pts[0].x,
      pts[0].y,
      pts[pts.length - 1].x,
      pts[pts.length - 1].y
    );
    grad.addColorStop(0, '#67e8f9');
    grad.addColorStop(0.5, '#06b6d4');
    grad.addColorStop(1, '#6366f1');
    ctx.strokeStyle = grad;
    ctx.lineWidth = 4;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.shadowColor = 'rgba(6, 182, 212, 0.9)';
    ctx.shadowBlur = 16;
    ctx.beginPath();
    ctx.moveTo(pts[0].x, pts[0].y);
    for (let i = 1; i < pts.length; i++) ctx.lineTo(pts[i].x, pts[i].y);
    ctx.stroke();
    ctx.restore();
  }
}

export function boundingBoxFromPoints(pts: ReadonlyArray<Point2D>): CssBox {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const p of pts) {
    if (p.x < minX) minX = p.x;
    if (p.y < minY) minY = p.y;
    if (p.x > maxX) maxX = p.x;
    if (p.y > maxY) maxY = p.y;
  }
  if (maxY - minY < 16 && maxX - minX >= 20) {
    minY -= 8;
    maxY += 8;
  }
  minX = Math.max(0, minX);
  minY = Math.max(0, minY);
  maxX = Math.min(window.innerWidth, maxX);
  maxY = Math.min(window.innerHeight, maxY);
  return { x: minX, y: minY, w: maxX - minX, h: maxY - minY };
}

export function cssBoxToPhysical(
  box: CssBox,
  initInfo: Pick<CircleOverlayInitInfo, 'width' | 'height'>
): PhysicalBox {
  const sx = initInfo.width / Math.max(1, window.innerWidth);
  const sy = initInfo.height / Math.max(1, window.innerHeight);
  return {
    x: Math.max(0, Math.round(box.x * sx)),
    y: Math.max(0, Math.round(box.y * sy)),
    width: Math.max(1, Math.round(box.w * sx)),
    height: Math.max(1, Math.round(box.h * sy)),
  };
}

export function positionAskBar(askBar: HTMLElement, box: CssBox | null): void {
  if (!box) return;
  askBar.classList.add('open');
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const bw = askBar.offsetWidth || 320;
  const bh = askBar.offsetHeight || 44;
  const left = Math.max(12, Math.min(vw - bw - 86, box.x + (box.w - bw) / 2));
  let top = box.y + box.h + 10;
  if (top + bh > vh - 12) top = box.y - bh - 10;
  if (top < 12) top = Math.max(12, vh - bh - 12);
  askBar.style.left = `${Math.round(left)}px`;
  askBar.style.top = `${Math.round(top)}px`;
}

export function positionResultCard(card: HTMLElement, box: CssBox | null): void {
  if (!box) return;
  card.classList.add('open');
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const cw = 340;
  const ch = card.offsetHeight || 200;
  const left = Math.max(12, Math.min(vw - cw - 86, box.x));
  let top = box.y + box.h + 12;
  if (top + ch > vh - 12) top = box.y - ch - 12;
  if (top < 16) top = Math.max(16, vh - ch - 16);
  card.style.left = `${Math.round(left)}px`;
  card.style.top = `${Math.round(top)}px`;
}
