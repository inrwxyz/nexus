import { createServer } from 'node:http';
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { extname, join, resolve } from 'node:path';
import { chromium } from 'playwright-core';
import pixelmatch from 'pixelmatch';
import { PNG } from 'pngjs';

const rootDir = resolve(import.meta.dirname, '..');
const captureMode = process.argv.includes('--capture');
const targetSubdir = process.argv.includes('--dist') ? 'dist' : 'ui';
const serveRoot = join(rootDir, targetSubdir);
const goldenDir = join(rootDir, 'test-output', 'golden');
const currentDir = join(rootDir, 'test-output', 'current');
const diffDir = join(rootDir, 'test-output', 'diff');

mkdirSync(goldenDir, { recursive: true });
mkdirSync(currentDir, { recursive: true });
mkdirSync(diffDir, { recursive: true });

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'application/javascript; charset=utf-8',
  '.mjs': 'application/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.png': 'image/png',
  '.svg': 'image/svg+xml',
};

function startStaticServer(dir) {
  return new Promise(resolveSrv => {
    const srv = createServer((req, res) => {
      const urlPath = (req.url || '/').split('?')[0];
      const safeRel = urlPath === '/' ? 'settings.html' : urlPath.replace(/^\/+/, '');
      const filePath = join(dir, safeRel);
      if (!filePath.startsWith(dir) || !existsSync(filePath) || statSync(filePath).isDirectory()) {
        res.writeHead(404);
        res.end('Not found');
        return;
      }
      const ext = extname(filePath).toLowerCase();
      res.writeHead(200, { 'Content-Type': MIME[ext] || 'application/octet-stream' });
      res.end(readFileSync(filePath));
    });
    srv.listen(0, '127.0.0.1', () => {
      const addr = srv.address();
      resolveSrv({ srv, baseUrl: `http://127.0.0.1:${addr.port}` });
    });
  });
}

function findEdge() {
  const candidates = [
    'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
  ];
  for (const c of candidates) {
    if (existsSync(c)) return c;
  }
  throw new Error('Microsoft Edge not found on this machine.');
}

function installTauriMock() {
  return ({ setupCompleted }) => {
    let settingsState = {
      setup_completed: setupCompleted,
      wake_word: 'hey google',
      corner_pill: true,
      show_on_island: false,
      launch_at_login: false,
      auto_updater: true,
      modules: {
        voice: { enabled: true },
        search: { enabled: true },
        companion: { enabled: true },
        scrcpy: { enabled: true },
        gamepad: { enabled: true },
      },
      circle_search: {
        on_screen_trigger: false,
        hotkey: 'Ctrl+Shift+S',
        default_action: 'ask',
        target_lang: 'bn',
        translate_mode: 'in_place',
      },
      companion: {
        voice_relay: true,
        touchpad: true,
        inertia_scrolling: true,
        remote_terminal: false,
        sensitivity: 1.2,
        show_on_island: true,
        paired_device: 'RMX5070',
        auth_token: 'mock-token-123456',
        pairing_code: null,
      },
      gamepad: {
        default_mode: 'action',
        aim_sensitivity: 1.2,
        gyro_sensitivity: 1.3,
        gyro_deadzone: 0.08,
        auto_sprint: true,
        haptics: true,
        auto_detect: true,
      },
      ai: {
        enabled: false,
        voice_reply: true,
        voice_style: 'google_natural',
        dictation_enabled: true,
        screen_context: true,
        local_llm_enabled: false,
        local_model: 'llama3.2:3b',
        routing_mode: 'hybrid',
        allow_ai_terminal: false,
      },
    };

    window.__TAURI__ = {
      core: {
        invoke: async (cmd, args) => {
          switch (cmd) {
            case 'get_settings':
              return settingsState;
            case 'save_settings':
              if (args?.newSettings) settingsState = args.newSettings;
              return true;
            case 'get_mic_mute':
              return false;
            case 'toggle_mic_mute':
              return true;
            case 'get_circle_search_status':
              return { enabled: true, on_screen_trigger: false, hotkey: 'Ctrl+Shift+S' };
            case 'get_companion_pairing_info':
              return { paired: false, device: null };
            case 'generate_companion_qr':
              return {
                paired: false,
                qr_svg: '<svg viewBox="0 0 29 29" xmlns="http://www.w3.org/2000/svg"><rect width="29" height="29" fill="#ffffff"/><rect x="4" y="4" width="7" height="7" fill="#000000"/><rect x="18" y="4" width="7" height="7" fill="#000000"/><rect x="4" y="18" width="7" height="7" fill="#000000"/></svg>',
              };
            case 'get_gamepad_status':
              return { enabled: true, connected: false, held_keys_count: 0, total_packets: 0, vigem_installed: false };
            case 'get_gamepad_profiles':
              return {
                active_profile_id: 'preset_action',
                profiles: [
                  { id: 'preset_action', name: 'Action / Shooter', category: 'action', items: [1, 2, 3] },
                  { id: 'preset_racing', name: 'Racing Wheel', category: 'racing', gyro: true, items: [1, 2] },
                  { id: 'preset_xbox', name: 'Console Classic', category: 'console', items: [1, 2, 3, 4] },
                ],
              };
            case 'sc_load_prefs':
              return { res: '1080', bitrate: '8', fps: '60', audioBuf: '120', audioCodec: 'auto' };
            case 'sc_devices':
              return [{ serial: '192.168.1.10:5555', state: 'device', model: 'RMX5070', is_wireless: true }];
            case 'sc_active':
              return false;
            case 'ai_list_keys':
            case 'ai_list_proposals':
              return [];
            case 'ai_local_status':
              return { running: false, ollama_installed: false, models: [] };
            default:
              return null;
          }
        },
      },
      event: { listen: async () => () => {} },
      window: {
        getCurrentWindow: () => ({
          isMaximized: async () => false,
          minimize: async () => {},
          maximize: async () => {},
          unmaximize: async () => {},
          onResized: async () => () => {},
        }),
      },
    };
  };
}

function comparePng(name, buf, maxDiffRatio = 0.002) {
  const curPath = join(currentDir, name);
  const goldPath = join(goldenDir, name);
  writeFileSync(curPath, buf);

  if (captureMode || !existsSync(goldPath)) {
    writeFileSync(goldPath, buf);
    return { name, mode: 'captured', diffPixels: 0, ratio: 0 };
  }

  const a = PNG.sync.read(readFileSync(goldPath));
  const b = PNG.sync.read(buf);
  if (a.width !== b.width || a.height !== b.height) {
    throw new Error(`Dimension mismatch for ${name}: golden=${a.width}x${a.height}, current=${b.width}x${b.height}`);
  }
  const diff = new PNG({ width: a.width, height: a.height });
  const diffPixels = pixelmatch(a.data, b.data, diff.data, a.width, a.height, { threshold: 0.1 });
  const ratio = diffPixels / (a.width * a.height);
  if (ratio > maxDiffRatio) {
    writeFileSync(join(diffDir, name), PNG.sync.write(diff));
  }
  return { name, mode: 'compared', diffPixels, ratio, pass: ratio <= maxDiffRatio };
}

async function run() {
  const { srv, baseUrl } = await startStaticServer(serveRoot);
  const browser = await chromium.launch({
    executablePath: findEdge(),
    headless: true,
    args: ['--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage'],
  });

  const errors = [];
  const results = [];

  try {
    const ctx = await browser.newContext({ viewport: { width: 1100, height: 720 }, deviceScaleFactor: 1 });
    const page = await ctx.newPage();
    page.on('pageerror', e => errors.push(`pageerror: ${e.message}`));
    await page.addInitScript(installTauriMock(), { setupCompleted: true });
    await page.goto(`${baseUrl}/settings.html`, { waitUntil: 'domcontentloaded' });
    await page.waitForTimeout(350);

    // 1. Voice Home
    results.push(comparePng('01_voice_home.png', await page.screenshot()));

    // 2. Circle to Search
    await page.click('.mod[data-module="search"]');
    await page.waitForTimeout(200);
    results.push(comparePng('02_circle_search.png', await page.screenshot()));

    // 3. Phone Link Pairing (starts unpaired & idle without pre-generated QR, generates on click)
    await page.click('.mod[data-module="companion"]');
    await page.waitForTimeout(250);
    const qrIdleVisible = await page.isVisible('#comp-qr-idle');
    const qrActiveHidden = !(await page.isVisible('#comp-qr-active'));
    if (!qrIdleVisible || !qrActiveHidden) {
      throw new Error('Expected Phone Link QR to stay hidden until Generate QR is clicked');
    }
    results.push(comparePng('03_phone_link_pairing.png', await page.screenshot()));

    // Verify clicking Generate QR renders unclipped SVG inside #comp-qr
    await page.click('#btn-comp-gen-qr');
    await page.waitForTimeout(150);
    const qrBoundsOk = await page.evaluate(() => {
      const box = document.querySelector('#comp-qr')?.getBoundingClientRect();
      const svg = document.querySelector('#comp-qr svg')?.getBoundingClientRect();
      if (!box || !svg || svg.width <= 0 || svg.height <= 0) return false;
      return (
        svg.left >= box.left + 8 &&
        svg.top >= box.top + 8 &&
        svg.right <= box.right - 8 &&
        svg.bottom <= box.bottom - 8
      );
    });
    if (!qrBoundsOk) throw new Error('Generated QR SVG clipped or missing padding inside #comp-qr');
    await page.click('#btn-comp-hide-qr');

    // 4. Phone Link -> Gamepad subsection -> Profile HUD
    await page.click('#subnav .tab[data-tab="gamepad"]');
    await page.waitForTimeout(200);
    await page.click('#gp-subnav .gp-subtab[data-gp-tab="profiles"]');
    await page.waitForTimeout(250);
    results.push(comparePng('04_phone_link_gamepad.png', await page.screenshot()));

    // 5. Screen Mirror (Connect)
    await page.click('.mod[data-module="scrcpy"]');
    await page.waitForTimeout(250);
    results.push(comparePng('05_screen_mirror.png', await page.screenshot()));

    // Functional check: First-run 4-module chooser activates only chosen modules
    const ctxSetup = await browser.newContext({ viewport: { width: 1100, height: 720 }, deviceScaleFactor: 1 });
    const setupPage = await ctxSetup.newPage();
    setupPage.on('pageerror', e => errors.push(`setup pageerror: ${e.message}`));
    await setupPage.addInitScript(installTauriMock(), { setupCompleted: false });
    await setupPage.goto(`${baseUrl}/settings.html`, { waitUntil: 'domcontentloaded' });
    await setupPage.waitForTimeout(300);

    const modalVisible = await setupPage.isVisible('#setup-modal');
    if (!modalVisible) throw new Error('Expected First-Run 4-Module Chooser modal to be visible');
    const cardCount = await setupPage.locator('#setup-modal .setup-card').count();
    if (cardCount !== 4) throw new Error(`Expected 4 module setup cards, found ${cardCount}`);

    if (captureMode) {
      console.log(`Captured ${results.length} views from ${targetSubdir}/ -> ${goldenDir}`);
    } else {
      console.log('\n=== NEXUS GOLDEN IMAGE COMPARISON ===');
      let failed = 0;
      for (const r of results) {
        const pct = (r.ratio * 100).toFixed(3);
        const flag = r.pass ? 'PASS' : 'FAIL';
        console.log(`  ${flag}  ${r.name}: diff=${r.diffPixels} px (${pct}%)`);
        if (!r.pass) failed++;
      }
      if (failed > 0 || errors.length > 0) {
        if (errors.length) console.error('Errors:', errors);
        process.exitCode = 1;
      }
    }
  } finally {
    await browser.close();
    srv.close();
  }
}

run().catch(err => {
  console.error(err);
  process.exit(1);
});
