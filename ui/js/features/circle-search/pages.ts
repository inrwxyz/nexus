import { $ } from '../../core/dom.js';

const ICO_ARROW =
  '<svg viewBox="0 0 24 24" width="16" height="16" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M13 6l6 6-6 6"/></svg>';
const ICO_PILL =
  '<svg viewBox="0 0 24 24" width="16" height="16" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="3"/><rect x="8" y="15" width="8" height="2" rx="1"/></svg>';
const ICO_KEY =
  '<svg viewBox="0 0 24 24" width="16" height="16" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10"/></svg>';
const ICO_ACT =
  '<svg viewBox="0 0 24 24" width="16" height="16" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.35-4.35"/></svg>';
const ICO_LANG =
  '<svg viewBox="0 0 24 24" width="16" height="16" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="m5 8 6 6"/><path d="m4 14 6-6 2-3"/><path d="M2 5h12"/><path d="M7 2h1"/><path d="m22 22-5-10-5 10"/><path d="M14 18h6"/></svg>';
const CHEVRON_SVG =
  '<svg viewBox="0 0 24 24" width="13" height="13" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg>';

function makeSplitSection(
  host: HTMLElement,
  id: string,
  kicker: string,
  titleHtml: string,
  blurb: string,
  mainHtml: string
): void {
  const sec = document.createElement('section');
  sec.id = id;
  sec.className = 'page-section split';
  sec.innerHTML = `<aside class="side"><div class="kicker">${kicker}</div><h1>${titleHtml}</h1><p>${blurb}</p></aside><div class="main">${mainHtml}</div>`;
  host.appendChild(sec);
}

export function buildCircleSearchPages(): void {
  const host = $('#page');
  if (!host || $('#page-search-overview')) return;

  makeSplitSection(
    host,
    'page-search-overview',
    '01',
    'Circle to Search',
    'Circle anything on screen, extract text, translate in-place, or search visually.',
    `<button class="launch-big" id="btn-cs-launch" type="button">
       <div class="launch-ico">${ICO_ACT}</div>
       <div class="launch-text">
         <div class="launch-title">Launch Circle to Search</div>
         <div class="launch-sub">Freeze the active monitor, then draw around anything to copy text or search it with Google Lens.</div>
       </div>
       <div class="launch-arrow">${ICO_ARROW}</div>
     </button>
     <div class="launch-foot"><span class="badge" id="cs-status-badge"><i></i>Ready</span></div>`
  );

  makeSplitSection(
    host,
    'page-search-config',
    '02',
    'Configu&shy;ration',
    'Options for Circle to Search.',
    `<div class="sheet">
       <div class="row">
         <div class="row-left">
           <div class="ico">${ICO_PILL}</div>
           <div class="row-text">
             <div class="row-label">On-Screen Trigger Pill</div>
             <div class="row-sub">Show a quick corner launcher pill for Circle to Search</div>
           </div>
         </div>
         <div class="row-ctrl">
           <label class="switch"><input type="checkbox" id="toggle-cs-pill" /><span class="slider"></span></label>
         </div>
       </div>
       <div class="row">
         <div class="row-left">
           <div class="ico">${ICO_KEY}</div>
           <div class="row-text">
             <div class="row-label">Global Hotkey</div>
             <div class="row-sub" id="cs-hotkey-sub">Press shortcut anywhere to freeze and circle</div>
           </div>
         </div>
         <div class="row-ctrl row-ctrl-inline">
           <span class="badge" id="cs-hotkey-badge"><i></i>Ctrl+Shift+S</span>
           <button class="btn" id="btn-cs-hotkey" type="button">Record Key</button>
         </div>
       </div>
       <div class="row">
         <div class="row-left">
           <div class="ico">${ICO_ACT}</div>
           <div class="row-text">
             <div class="row-label">Default Action</div>
             <div class="row-sub">What runs right after circling a region</div>
           </div>
         </div>
         <div class="row-ctrl">
           <div class="dd" data-dd data-select="sel-cs-action" data-value="dd-cs-action-value">
             <button class="dd-trigger field" type="button" aria-haspopup="listbox">
               <span class="dd-value" id="dd-cs-action-value">Show Options</span>${CHEVRON_SVG}
             </button>
             <div class="dd-menu" role="listbox"></div>
           </div>
           <select id="sel-cs-action" class="sr-only" tabindex="-1" aria-hidden="true">
             <option value="ask">Show Options  (Default)</option>
             <option value="ocr">Extract Text (OCR)</option>
             <option value="translate">Translate Selection</option>
             <option value="lens">Google Lens</option>
           </select>
         </div>
       </div>
       <div class="row">
         <div class="row-left">
           <div class="ico">${ICO_LANG}</div>
           <div class="row-text">
             <div class="row-label">Default Target Language</div>
             <div class="row-sub">Used automatically by both In-Place and Google Translate Web</div>
           </div>
         </div>
         <div class="row-ctrl">
           <div class="dd" data-dd data-select="sel-cs-lang" data-value="dd-cs-lang-value">
             <button class="dd-trigger field" type="button" aria-haspopup="listbox">
               <span class="dd-value" id="dd-cs-lang-value">Bengali</span>${CHEVRON_SVG}
             </button>
             <div class="dd-menu" role="listbox"></div>
           </div>
           <select id="sel-cs-lang" class="sr-only" tabindex="-1" aria-hidden="true">
             <option value="bn">Bengali  (Default)</option>
             <option value="en">English</option>
             <option value="hi">Hindi</option>
             <option value="es">Spanish</option>
             <option value="ja">Japanese</option>
             <option value="fr">French</option>
             <option value="de">German</option>
             <option value="ar">Arabic</option>
             <option value="ko">Korean</option>
             <option value="zh-CN">Chinese</option>
             <option value="pt">Portuguese</option>
             <option value="ru">Russian</option>
           </select>
         </div>
       </div>
       <div class="row">
         <div class="row-left">
           <div class="ico">${ICO_ACT}</div>
           <div class="row-text">
             <div class="row-label">Translation Mode</div>
             <div class="row-sub">In-Place overlay (fast) or Google Translate Web for all languages</div>
           </div>
         </div>
         <div class="row-ctrl">
           <div class="dd" data-dd data-select="sel-cs-trmode" data-value="dd-cs-trmode-value">
             <button class="dd-trigger field" type="button" aria-haspopup="listbox">
               <span class="dd-value" id="dd-cs-trmode-value">In-Place Screen Overlay</span>${CHEVRON_SVG}
             </button>
             <div class="dd-menu" role="listbox"></div>
           </div>
           <select id="sel-cs-trmode" class="sr-only" tabindex="-1" aria-hidden="true">
             <option value="in_place">In-Place Screen Overlay  (Default)</option>
             <option value="google_web">Google Translate Web  (All Languages)</option>
           </select>
         </div>
       </div>
     </div>`
  );

  makeSplitSection(
    host,
    'page-search-system',
    '03',
    'Sys<em>tem</em>',
    'Turn Circle to Search on or off.',
    '<div class="sheet" id="sys-search"></div>'
  );
}
