    const T = window.__TAURI__;
    const bg = document.getElementById('bg');
    const draw = document.getElementById('draw');
    const trLayer = document.getElementById('tr-layer');
    const bgCtx = bg.getContext('2d');
    const ctx = draw.getContext('2d');

    const railWrap = document.getElementById('rail-wrap');
    const railGrip = document.getElementById('rail-grip');
    const toolLasso = document.getElementById('tool-lasso');
    const toolTranslate = document.getElementById('tool-translate');
    const toolMusic = document.getElementById('tool-music');
    const langPill = document.getElementById('lang-pill');
    const langBadge = document.getElementById('lang-badge');
    const popLang = document.getElementById('pop-lang');
    const btnGTranslate = document.getElementById('btn-gtranslate');
    const miniChip = document.getElementById('mini-chip');
    const miniChipText = document.getElementById('mini-chip-text');

    const card = document.getElementById('result-card');
    const ocrBox = document.getElementById('ocr-text');
    const selSizeEl = document.getElementById('sel-size');
    const btnCopy = document.getElementById('btn-copy');
    const btnWeb = document.getElementById('btn-web');
    const btnRedraw = document.getElementById('btn-redraw');
    const btnClose = document.getElementById('btn-close-overlay');
    const copyToast = document.getElementById('copy-toast');
    const askBar = document.getElementById('ask-bar');
    const askLens = document.getElementById('ask-lens');
    const askOcr = document.getElementById('ask-ocr');
    const askTranslate = document.getElementById('ask-translate');
    const askGTranslate = document.getElementById('ask-gtranslate');
    const askRedraw = document.getElementById('ask-redraw');

    let frameUrl = null;
    let initInfo = { width: window.innerWidth, height: window.innerHeight, default_action: 'ask', target_lang: 'bn', translate_mode: 'in_place' };
    let activeTool = 'ask'; // 'ask' | 'lasso' | 'translate' | 'ocr'
    let drawing = false;
    let points = [];
    let activeBox = null;
    let activePhys = null;
    let lastTranslatePhys = { x: 0, y: 0, width: 0, height: 0 };
    let translateReqSeq = 0;
    let musicListening = false;
    let closing = false;
    let toastTimer = null;

    function showToast(msg, durationMs = 1400) {
      copyToast.textContent = msg;
      copyToast.classList.add('show');
      clearTimeout(toastTimer);
      toastTimer = setTimeout(() => copyToast.classList.remove('show'), durationMs);
    }

    function shortLangLabel(code) {
      return (code || 'en').split('-')[0].toUpperCase() + ' ▾';
    }

    function anchorRailToPixels() {
      const r = railWrap.getBoundingClientRect();
      railWrap.style.left = `${Math.round(r.left)}px`;
      railWrap.style.top = `${Math.round(r.top)}px`;
      railWrap.style.right = 'auto';
      railWrap.style.transform = 'none';
    }

    // Position a tiny floating pill cleanly with a tight 6px gap beside a button on the rail
    function positionBesideButton(el, btnEl) {
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

    function positionAllAttachments() {
      if (langPill.classList.contains('open')) positionBesideButton(langPill, toolTranslate);
      if (miniChip.classList.contains('open')) {
        positionBesideButton(miniChip, musicListening ? toolMusic : toolLasso);
      }
    }

    // Freely draggable Vertical Action Pill
    (function enableRailDrag() {
      let draggingRail = false, startX = 0, startY = 0, baseL = 0, baseT = 0;
      railGrip.addEventListener('pointerdown', e => {
        e.preventDefault(); e.stopPropagation();
        anchorRailToPixels();
        const r = railWrap.getBoundingClientRect();
        draggingRail = true; startX = e.clientX; startY = e.clientY; baseL = r.left; baseT = r.top;
        railGrip.setPointerCapture(e.pointerId);
      });
      railGrip.addEventListener('pointermove', e => {
        if (!draggingRail) return;
        const r = railWrap.getBoundingClientRect();
        const nl = Math.max(8, Math.min(window.innerWidth - r.width - 8, baseL + (e.clientX - startX)));
        const nt = Math.max(8, Math.min(window.innerHeight - r.height - 8, baseT + (e.clientY - startY)));
        railWrap.style.left = `${Math.round(nl)}px`;
        railWrap.style.top = `${Math.round(nt)}px`;
        positionAllAttachments();
      });
      const endDrag = () => { draggingRail = false; };
      railGrip.addEventListener('pointerup', endDrag);
      railGrip.addEventListener('pointercancel', endDrag);
    })();

    function showChip(anchorBtn, text) {
      miniChipText.textContent = text;
      miniChip.classList.add('open');
      positionBesideButton(miniChip, anchorBtn);
    }

    function hideChip() {
      miniChip.classList.remove('open');
    }

    function clearInPlaceTranslation() {
      trLayer.textContent = '';
    }

    function setTool(tool) {
      activeTool = tool;
      document.querySelectorAll('.tool-btn[data-tool]').forEach(b => b.classList.toggle('active', b.dataset.tool === tool));
      hideChip();
      askBar.classList.remove('open');

      if (tool === 'translate') {
        card.classList.remove('open');
        activeBox = null;
        activePhys = null;
        points = [];
        renderScrim(null, []);
        langBadge.textContent = shortLangLabel(popLang.value);
        langPill.classList.add('open');
        positionBesideButton(langPill, toolTranslate);
        if (initInfo.translate_mode === 'google_web') {
          clearInPlaceTranslation();
          showToast('Draw area for Google Translate, or click G-Translate ↗ for full screen', 2800);
        } else {
          // Immediately trigger full-screen in-place translation automatically
          runTranslate({ x: 0, y: 0, width: 0, height: 0 });
        }
      } else {
        langPill.classList.remove('open');
        clearInPlaceTranslation();
        card.classList.remove('open');
      }
    }

    document.querySelectorAll('.tool-btn[data-tool]').forEach(btn => {
      btn.addEventListener('click', () => setTool(btn.dataset.tool));
    });

    popLang.addEventListener('change', () => {
      langBadge.textContent = shortLangLabel(popLang.value);
      positionBesideButton(langPill, toolTranslate);
      if (activeTool === 'translate' && initInfo.translate_mode !== 'google_web') {
        runTranslate(lastTranslatePhys || { x: 0, y: 0, width: 0, height: 0 });
      }
    });

    btnGTranslate.addEventListener('click', async () => {
      const target = activePhys || lastTranslatePhys || { x: 0, y: 0, width: 0, height: 0 };
      await launchGoogleTranslateWeb(target);
    });

    toolMusic.addEventListener('click', async () => {
      if (musicListening || closing) return;
      musicListening = true;
      closing = true;
      askBar.classList.remove('open');
      clearInPlaceTranslation();
      card.classList.remove('open');
      langPill.classList.remove('open');
      hideChip();
      toolMusic.classList.add('busy');

      try {
        await T.core.invoke('recognize_desktop_music');
        cleanupResources();
      } catch (e) {
        closing = false;
        musicListening = false;
        toolMusic.classList.remove('busy');
        showChip(toolMusic, `Audio error: ${e}`);
        setTimeout(() => { if (!musicListening) hideChip(); }, 2500);
      }
    });

    function resizeCanvases() {
      const dpr = window.devicePixelRatio || 1;
      draw.width = Math.max(1, Math.round(window.innerWidth * dpr));
      draw.height = Math.max(1, Math.round(window.innerHeight * dpr));
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      renderScrim(activeBox, points);
    }

    function renderScrim(box, pts) {
      const vw = window.innerWidth, vh = window.innerHeight;
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
        const grad = ctx.createLinearGradient(pts[0].x, pts[0].y, pts[pts.length - 1].x, pts[pts.length - 1].y);
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

    function resetSelection() {
      const hadBox = !!activeBox;
      activeBox = null; activePhys = null; points = [];
      askBar.classList.remove('open');
      card.classList.remove('open');
      ocrBox.value = ''; ocrBox.classList.add('empty');
      btnCopy.disabled = true; btnWeb.disabled = true;
      if (activeTool === 'translate' && !hadBox) {
        renderScrim(null, []);
      } else {
        lastTranslatePhys = { x: 0, y: 0, width: 0, height: 0 };
        clearInPlaceTranslation();
        renderScrim(null, []);
      }
    }

    function positionAskBar(box) {
      if (!box) return;
      askBar.classList.add('open');
      const vw = window.innerWidth, vh = window.innerHeight;
      const bw = askBar.offsetWidth || 320, bh = askBar.offsetHeight || 44;
      const left = Math.max(12, Math.min(vw - bw - 86, box.x + (box.w - bw) / 2));
      let top = box.y + box.h + 10;
      if (top + bh > vh - 12) top = box.y - bh - 10;
      if (top < 12) top = Math.max(12, vh - bh - 12);
      askBar.style.left = `${Math.round(left)}px`;
      askBar.style.top = `${Math.round(top)}px`;
    }

    function cssBoxToPhysical(box) {
      const sx = initInfo.width / Math.max(1, window.innerWidth);
      const sy = initInfo.height / Math.max(1, window.innerHeight);
      return {
        x: Math.max(0, Math.round(box.x * sx)),
        y: Math.max(0, Math.round(box.y * sy)),
        width: Math.max(1, Math.round(box.w * sx)),
        height: Math.max(1, Math.round(box.h * sy)),
      };
    }

    function positionCard(box) {
      if (!box) return;
      card.classList.add('open');
      const vw = window.innerWidth, vh = window.innerHeight;
      const cw = 340, ch = card.offsetHeight || 200;
      let left = Math.max(12, Math.min(vw - cw - 86, box.x));
      let top = box.y + box.h + 12;
      if (top + ch > vh - 12) top = box.y - ch - 12;
      if (top < 16) top = Math.max(16, vh - ch - 16);
      card.style.left = `${Math.round(left)}px`;
      card.style.top = `${Math.round(top)}px`;
    }

    async function runOcr(phys) {
      clearInPlaceTranslation();
      ocrBox.value = 'Extracting text offline…'; ocrBox.classList.add('empty');
      btnCopy.disabled = true; btnWeb.disabled = true;
      try {
        const res = await T.core.invoke('process_circle_selection', {
          x: phys.x, y: phys.y, width: phys.width, height: phys.height, mode: 'ocr'
        });
        if (res.error) { ocrBox.value = res.error; return; }
        const txt = (res.text || '').trim();
        if (txt) {
          ocrBox.value = txt; ocrBox.classList.remove('empty');
          btnCopy.disabled = false; btnWeb.disabled = false;
        } else {
          ocrBox.value = 'No text detected in this area.';
        }
      } catch (e) { ocrBox.value = `OCR failed: ${e}`; }
    }

    function renderInPlaceLines(lines) {
      clearInPlaceTranslation();
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

        let fs = Math.max(10.0, Math.min(44, (cur.item.height * sy) * 0.78));
        el.style.fontSize = `${fs.toFixed(1)}px`;
        trLayer.appendChild(el);

        if (el.scrollWidth > maxW + 1) {
          fs = Math.max(8.0, fs * (maxW / el.scrollWidth));
          el.style.fontSize = `${fs.toFixed(1)}px`;
        }

        el.addEventListener('click', async ev => {
          ev.stopPropagation();
          try {
            await T.core.invoke('copy_circle_text', { text: cur.item.translated_text });
            showToast('Copied');
          } catch (_) {}
        });
      }
    }

    async function runTranslate(phys) {
      const reqSeq = ++translateReqSeq;
      const tl = popLang.value || 'en';
      lastTranslatePhys = phys;
      card.classList.remove('open');
      toolTranslate.classList.add('busy');
      langBadge.textContent = `${(tl.split('-')[0]).toUpperCase()} …`;
      positionBesideButton(langPill, toolTranslate);

      try {
        const res = await T.core.invoke('translate_selection', {
          x: phys.x, y: phys.y, width: phys.width, height: phys.height, targetLang: tl
        });
        if (reqSeq !== translateReqSeq || closing) return;
        if (res.error || !res.ok || !res.lines || res.lines.length === 0) {
          clearInPlaceTranslation();
          const msg = (!res.error || res.error.includes('No readable text'))
            ? 'No local OCR text — click G-Translate ↗ for all languages'
            : res.error;
          showToast(msg, 3400);
          return;
        }
        renderInPlaceLines(res.lines);
      } catch (e) {
        if (reqSeq !== translateReqSeq || closing) return;
        clearInPlaceTranslation();
        showToast(`Translate error: ${e}`, 3400);
      } finally {
        if (reqSeq === translateReqSeq) {
          toolTranslate.classList.remove('busy');
          langBadge.textContent = shortLangLabel(popLang.value);
          positionBesideButton(langPill, toolTranslate);
        }
      }
    }

    async function launchGoogleTranslateWeb(phys) {
      const p = phys || { x: 0, y: 0, width: 0, height: 0 };
      const tl = popLang.value || 'bn';
      toolTranslate.classList.add('busy');
      showChip(toolTranslate, 'Opening Google Translate…');
      try {
        await T.core.invoke('google_translate_image', {
          x: p.x, y: p.y, width: p.width, height: p.height, targetLang: tl
        });
        cleanupResources();
      } catch (e) {
        hideChip();
        toolTranslate.classList.remove('busy');
        showToast(`Google Translate failed: ${e}`, 3400);
      }
    }

    async function launchLens(phys) {
      if (!phys) return;
      toolLasso.classList.add('busy');
      showChip(toolLasso, 'Searching Google Lens…');
      try {
        await T.core.invoke('lens_visual_search', {
          x: phys.x, y: phys.y, width: phys.width, height: phys.height
        });
        cleanupResources();
      } catch (e) {
        hideChip();
        toolLasso.classList.remove('busy');
        showToast(`Visual search failed: ${e}`);
      }
    }

    function cleanupResources() {
      clearInPlaceTranslation();
      askBar.classList.remove('open');
      if (frameUrl) { URL.revokeObjectURL(frameUrl); frameUrl = null; }
      bg.width = 0; bg.height = 0; draw.width = 0; draw.height = 0;
    }

    async function closeOverlay() {
      if (closing) return;
      closing = true;
      cleanupResources();
      try { if (T && T.core) await T.core.invoke('close_circle_search_overlay'); } catch (_) {}
    }

    draw.addEventListener('pointerdown', e => {
      if (e.button !== 0 || closing) return;
      if (!musicListening) hideChip();
      askBar.classList.remove('open');
      card.classList.remove('open');
      if (activeTool !== 'translate') clearInPlaceTranslation();
      drawing = true;
      points = [{ x: e.clientX, y: e.clientY }];
      draw.setPointerCapture(e.pointerId);
      renderScrim(null, points);
    });

    draw.addEventListener('pointermove', e => {
      if (!drawing) return;
      const last = points[points.length - 1];
      if (!last || Math.hypot(e.clientX - last.x, e.clientY - last.y) >= 3) {
        points.push({ x: e.clientX, y: e.clientY });
        renderScrim(null, points);
      }
    });

    async function finishStroke() {
      if (!drawing) return;
      drawing = false;
      if (points.length < 2) { resetSelection(); return; }

      let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
      for (const p of points) {
        if (p.x < minX) minX = p.x; if (p.y < minY) minY = p.y;
        if (p.x > maxX) maxX = p.x; if (p.y > maxY) maxY = p.y;
      }
      if (maxY - minY < 16 && maxX - minX >= 20) { minY -= 8; maxY += 8; }
      minX = Math.max(0, minX); minY = Math.max(0, minY);
      maxX = Math.min(window.innerWidth, maxX); maxY = Math.min(window.innerHeight, maxY);

      const box = { x: minX, y: minY, w: maxX - minX, h: maxY - minY };
      if (box.w < 10 || box.h < 10) { resetSelection(); return; }

      activeBox = box;
      activePhys = cssBoxToPhysical(box);
      selSizeEl.textContent = `${activePhys.width} × ${activePhys.height}`;
      renderScrim(box, []);

      if (activeTool === 'translate') {
        if (initInfo.translate_mode === 'google_web') {
          await launchGoogleTranslateWeb(activePhys);
        } else {
          await runTranslate(activePhys);
        }
      } else if (activeTool === 'ocr') {
        positionCard(box);
        await runOcr(activePhys);
      } else if (activeTool === 'lasso') {
        await launchLens(activePhys);
      } else {
        positionAskBar(box);
      }
    }

    draw.addEventListener('pointerup', finishStroke);
    draw.addEventListener('pointercancel', () => { drawing = false; resetSelection(); });

    askLens.addEventListener('click', async () => {
      if (!activePhys) return;
      askBar.classList.remove('open');
      await launchLens(activePhys);
    });
    askOcr.addEventListener('click', async () => {
      if (!activePhys || !activeBox) return;
      askBar.classList.remove('open');
      positionCard(activeBox);
      await runOcr(activePhys);
    });
    askTranslate.addEventListener('click', async () => {
      if (!activePhys) return;
      askBar.classList.remove('open');
      if (initInfo.translate_mode === 'google_web') {
        await launchGoogleTranslateWeb(activePhys);
        return;
      }
      activeTool = 'translate';
      document.querySelectorAll('.tool-btn[data-tool]').forEach(b => b.classList.toggle('active', b.dataset.tool === 'translate'));
      langBadge.textContent = shortLangLabel(popLang.value);
      langPill.classList.add('open');
      positionBesideButton(langPill, toolTranslate);
      await runTranslate(activePhys);
    });
    askGTranslate.addEventListener('click', async () => {
      if (!activePhys) return;
      askBar.classList.remove('open');
      await launchGoogleTranslateWeb(activePhys);
    });
    askRedraw.addEventListener('click', resetSelection);

    btnClose.addEventListener('click', closeOverlay);
    btnRedraw.addEventListener('click', resetSelection);

    btnCopy.addEventListener('click', async () => {
      const txt = ocrBox.value.trim();
      if (!txt) return;
      try {
        await T.core.invoke('copy_circle_text', { text: txt });
        const orig = btnCopy.textContent;
        btnCopy.textContent = 'Copied!';
        setTimeout(() => { btnCopy.textContent = orig; }, 1300);
      } catch (_) {}
    });

    btnWeb.addEventListener('click', async () => {
      const txt = ocrBox.value.trim();
      if (!txt) return;
      cleanupResources();
      try { await T.core.invoke('search_circle_text', { query: txt }); } catch (_) {}
    });

    document.addEventListener('keydown', e => {
      if (e.key === 'Escape') { e.preventDefault(); closeOverlay(); }
    });
    window.addEventListener('resize', resizeCanvases);
    window.addEventListener('beforeunload', cleanupResources);

    (async function boot() {
      resizeCanvases();
      try {
        const raw = await T.core.invoke('get_frozen_frame');
        const blob = new Blob([raw], { type: 'image/bmp' });
        frameUrl = URL.createObjectURL(blob);
        const img = new Image();
        const done = async () => {
          initInfo = await T.core.invoke('overlay_ready');
          if (initInfo.target_lang && [...popLang.options].some(o => o.value === initInfo.target_lang)) {
            popLang.value = initInfo.target_lang;
          }
          langBadge.textContent = shortLangLabel(popLang.value);
          anchorRailToPixels();
          if (initInfo.default_action === 'translate') setTool('translate');
          else if (initInfo.default_action === 'ocr') setTool('ocr');
          else if (initInfo.default_action === 'lens') setTool('lasso');
          else setTool('ask');
        };
        img.onload = async () => {
          bg.width = img.naturalWidth || window.innerWidth;
          bg.height = img.naturalHeight || window.innerHeight;
          bgCtx.drawImage(img, 0, 0, bg.width, bg.height);
          URL.revokeObjectURL(frameUrl);
          frameUrl = null;
          await done();
        };
        img.onerror = done;
        img.src = frameUrl;
      } catch (e) {
        console.error('Overlay init failed:', e);
        try { await T.core.invoke('overlay_ready'); } catch (_) {}
      }
    })();
