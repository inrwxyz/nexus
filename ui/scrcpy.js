// Nexus · Scrcpy module UI. Talks to modules/scrcpy.rs (sc_* commands).
(() => {
  const T = window.__TAURI__; if (!T || !T.core) return;
  const inv = (c, a) => T.core.invoke(c, a);
  const $ = (s, r) => (r || document).querySelector(s), $$ = (s, r) => [...(r || document).querySelectorAll(s)];
  const DEF = {
    res: '1080',
    bitrate: '8',
    fps: '60',
    screenOff: false,
    stayAwake: false,
    audioBuf: '120',
    audioCodec: 'auto',
    audioDup: false,
  };
  let p = { ...DEF }, qrTimer = null, busy = false, statusTimer = null;
  const hiddenUsb = new Set(); // USB phones the user removed from the list (until re-plugged)
  let activeDevice = null;
  const toast = m => { try { showSaveToast(m); } catch (e) {} };
  const msg = e => String((e && e.message) || e || 'Something went wrong');

  const VALID = {
    res: ['720', '1080', 'orig'],
    bitrate: ['4', '8', '16'],
    fps: ['60', '120'],
    audioBuf: ['40', '120', '250'],
    audioCodec: ['auto', 'opus', 'aac'],
  };

  // ── Normalize preferences across different versions ───────────────────
  function normalizePrefs(raw) {
    if (!raw || typeof raw !== 'object') return { ...DEF };
    const r = raw;
    const res = r.res || (r.maxSize === 1280 ? '720' : r.maxSize === 1920 ? '1080' : r.maxSize === 0 ? 'orig' : undefined);
    const fps = r.fps || (r.maxFps ? String(r.maxFps) : undefined);
    const bitrate = r.bitrate || (r.bitrateMbps ? String(r.bitrateMbps) : undefined);
    const audioBuf = r.audioBuf || (r.audioBufferMs ? String(r.audioBufferMs) : undefined);
    const audioCodec = r.audioCodec || undefined;
    const screenOff = r.screenOff !== undefined ? Boolean(r.screenOff) : (r.turnScreenOff !== undefined ? Boolean(r.turnScreenOff) : DEF.screenOff);
    const stayAwake = r.stayAwake !== undefined ? Boolean(r.stayAwake) : DEF.stayAwake;
    const audioDup = r.audioDup !== undefined ? Boolean(r.audioDup) : DEF.audioDup;

    return {
      res: VALID.res.includes(String(res)) ? String(res) : DEF.res,
      bitrate: VALID.bitrate.includes(String(bitrate)) ? String(bitrate) : DEF.bitrate,
      fps: VALID.fps.includes(String(fps)) ? String(fps) : DEF.fps,
      audioBuf: VALID.audioBuf.includes(String(audioBuf)) ? String(audioBuf) : DEF.audioBuf,
      audioCodec: VALID.audioCodec.includes(String(audioCodec)) ? String(audioCodec) : DEF.audioCodec,
      screenOff,
      stayAwake,
      audioDup,
    };
  }

  // ── Stream settings (saved on this PC) ──────────────────────────────────
  const save = () => {
    p = normalizePrefs(p);
    return inv('sc_save_prefs', { prefs: p }).catch(() => {});
  };

  function paint() {
    p = normalizePrefs(p);
    Object.keys(VALID).forEach(key => {
      const seg = $(`.seg[data-pref="${key}"]`);
      if (!seg) return;
      const cur = p[key];
      $$('.sg', seg).forEach(b => {
        b.classList.toggle('on', b.dataset.v === cur);
      });
    });
    ['screenOff', 'stayAwake', 'audioDup'].forEach(key => {
      const input = $(`input[data-pref="${key}"]`);
      if (input) input.checked = Boolean(p[key]);
    });
  }

  // Bind click handlers directly to all segment options
  Object.keys(VALID).forEach(key => {
    const seg = $(`.seg[data-pref="${key}"]`);
    if (!seg) return;
    $$('.sg', seg).forEach(b => {
      b.onclick = (e) => {
        e.preventDefault();
        e.stopPropagation();
        const val = b.dataset.v;
        if (VALID[key].includes(val)) {
          p[key] = val;
          paint();
          save();
          toast('Stream setting updated');
        }
      };
    });
  });

  // Bind change handlers to checkboxes
  ['screenOff', 'stayAwake', 'audioDup'].forEach(key => {
    const input = $(`input[data-pref="${key}"]`);
    if (!input) return;
    input.onchange = () => {
      p[key] = Boolean(input.checked);
      save();
      toast('Stream setting updated');
    };
  });

  const opts = serial => ({
    serial: serial || (activeDevice ? activeDevice.serial : null),
    maxSize: p.res === '720' ? 1280 : p.res === '1080' ? 1920 : 0,
    maxFps: parseInt(p.fps, 10) || 60,
    bitrateMbps: parseInt(p.bitrate, 10) || 8,
    stayAwake: !!p.stayAwake,
    turnScreenOff: !!p.screenOff,
    audioBufferMs: parseInt(p.audioBuf, 10) || 120,
    audioCodec: p.audioCodec || 'auto',
    audioDup: !!p.audioDup,
  });

  // ── Helpers ─────────────────────────────────────────────────────────────
  const setSub = (id, t) => { const el = $(id); if (el) el.textContent = t; };

  // ── Status & Mirror Control (Start/Stop toggle) ──────────────────────────
  async function refreshStatus() {
    let on = false; try { on = await inv('sc_active'); } catch (e) {}
    // While mirroring, watch for the scrcpy window being closed directly
    if (on && !statusTimer) statusTimer = setInterval(refreshStatus, 1000);
    if (!on && statusTimer) { clearInterval(statusTimer); statusTimer = null; }
    const btn = $('#sc-stop');
    const title = $('#sc-mirror-title');
    if (on) {
      if (title) title.textContent = 'Stop mirroring';
      setSub('#sc-status', activeDevice ? `${activeDevice.model || 'Phone'} mirroring is active` : 'Mirroring is currently running');
      if (btn) {
        btn.textContent = 'Stop';
        btn.disabled = false;
        btn.classList.add('btn-danger');
      }
    } else {
      if (title) title.textContent = 'Screen mirroring';
      if (activeDevice) {
        setSub('#sc-status', `${activeDevice.model || 'Phone'} connected · Ready to mirror`);
      } else {
        setSub('#sc-status', 'Not running · Press Start to mirror');
      }
      if (btn) {
        btn.textContent = 'Start';
        btn.disabled = false;
        btn.classList.remove('btn-danger');
      }
    }
  }

  async function start(serial) {
    if (typeof modState !== 'undefined' && modState.scrcpy && !modState.scrcpy.enabled) {
      toast('Scrcpy module is turned off');
      return;
    }
    try {
      await inv('sc_start', { opts: opts(serial) });
      toast('Mirroring started');
    } catch (e) {
      toast(msg(e));
    }
    setTimeout(refreshDevices, 600);
    setTimeout(refreshStatus, 800);
  }

  const guard = async (btn, fn) => {
    if (busy) return; busy = true; if (btn) btn.disabled = true;
    try { await fn(); } catch (e) { toast(msg(e)); } finally { busy = false; if (btn) btn.disabled = false; }
  };

  // ── Device Detection & IP Auto-fill ─────────────────────────────────────
  async function refreshDevices() {
    try {
      let devs = [];
      try { devs = await inv('sc_devices'); } catch (_) {}

      // If no devices connected, check if there is a saved IP
      if (!devs || devs.length === 0) {
        try {
          const saved = await inv('sc_saved');
          if (saved && saved.ip) {
            if (!$('#sc-ip').value) $('#sc-ip').value = saved.ip;
            if (!$('#sc-port').value) $('#sc-port').value = saved.port || '5555';
            setSub('#sc-ip-sub', `Saved phone: ${saved.model || saved.ip}:${saved.port || 5555}`);
          }
        } catch (_) {}
      }

      const present = new Set((devs || []).map(d => d.serial));
      [...hiddenUsb].forEach(s => { if (!present.has(s)) hiddenUsb.delete(s); }); // re-plugged -> show again
      const connected = (devs || []).filter(d => d.state === 'device' && !hiddenUsb.has(d.serial));
      const rmBtn = $('#sc-dev-remove');
      if (rmBtn) rmBtn.hidden = connected.length === 0;
      // Sort: USB first, then direct IP:5555, then TLS mDNS
      connected.sort((a, b) => {
        if (!a.is_wireless && b.is_wireless) return -1;
        if (a.is_wireless && !b.is_wireless) return 1;
        if (a.serial.includes(':5555') && !b.serial.includes(':5555')) return -1;
        if (!a.serial.includes(':5555') && b.serial.includes(':5555')) return 1;
        return 0;
      });

      if (connected.length > 0) {
        activeDevice = connected[0];
        const model = (activeDevice.model && activeDevice.model !== 'Unknown') ? activeDevice.model : 'Android Device';
        const isWire = activeDevice.is_wireless;

        if ($('#sc-dev-name')) {
          $('#sc-dev-name').textContent = connected.length > 1
            ? `${model} (${connected.length} connected)`
            : `${model} (Connected)`;
        }
        if ($('#sc-dev-sub')) {
          $('#sc-dev-sub').textContent = isWire
            ? `Connected wirelessly via ADB (${activeDevice.serial})`
            : `Connected via USB cable (${activeDevice.serial})`;
        }

        if (isWire && activeDevice.serial.includes(':')) {
          const [ip, port] = activeDevice.serial.split(':');
          $('#sc-ip').value = ip;
          $('#sc-port').value = port || '5555';
          setSub('#sc-ip-sub', `Connected: ${ip}:${port || 5555}`);
        }

        const usbDev = connected.find(d => !d.is_wireless);
        if (usbDev) {
          setSub('#sc-usb-sub', `${usbDev.model || 'Phone'} ready on USB`);
        } else {
          setSub('#sc-usb-sub', 'Plug in your phone, then press Start');
        }
      } else {
        activeDevice = null;
        let on = false; try { on = await inv('sc_active'); } catch (_) {}
        if (on) {
          if ($('#sc-dev-name')) $('#sc-dev-name').textContent = 'Phone (Mirror active)';
          if ($('#sc-dev-sub')) $('#sc-dev-sub').textContent = 'Phone screen is currently mirrored to PC';
        } else {
          if ($('#sc-dev-name')) $('#sc-dev-name').textContent = 'No phone connected';
          if ($('#sc-dev-sub')) $('#sc-dev-sub').textContent = 'Connect via USB cable or Wi-Fi IP/Port';
        }
        setSub('#sc-usb-sub', 'Plug in your phone, then press Start');
      }
      refreshStatus();
    } catch (e) {
      console.error('refreshDevices error', e);
    }
  }

  // ── Refresh device button ───────────────────────────────────────────────
  const devRefreshBtn = $('#sc-dev-refresh');
  if (devRefreshBtn) {
    devRefreshBtn.addEventListener('click', () => guard(devRefreshBtn, async () => {
      await refreshDevices();
      toast('Device list updated');
    }));
  }

  // ── Remove device ───────────────────────────────────────────────────────
  const devRemoveBtn = $('#sc-dev-remove');
  if (devRemoveBtn) {
    devRemoveBtn.addEventListener('click', () => guard(devRemoveBtn, async () => {
      if (!activeDevice) return;
      const dev = activeDevice;
      await inv('sc_remove_device', { serial: dev.serial });
      if (!dev.is_wireless) hiddenUsb.add(dev.serial);
      else { if ($('#sc-ip')) $('#sc-ip').value = ''; setSub('#sc-ip-sub', 'Filled in automatically'); }
      activeDevice = null;
      toast(`${(dev.model && dev.model !== 'Unknown') ? dev.model : 'Device'} removed`);
      await refreshDevices();
    }));
  }

  // ── Mirror Control Button (Start/Stop) ───────────────────────────────────
  $('#sc-stop').addEventListener('click', e => guard(e.currentTarget, async () => {
    let on = false; try { on = await inv('sc_active'); } catch (_) {}
    if (on) {
      try { await inv('sc_stop'); toast('Mirroring stopped'); } catch (_) {}
      setTimeout(refreshStatus, 400);
    } else {
      // If we don't have an active device yet, try finding or connecting to IP
      if (!activeDevice) {
        const ip = $('#sc-ip').value.trim();
        const port = parseInt($('#sc-port').value, 10) || 5555;
        if (ip) {
          if (!/^(\d{1,3}\.){3}\d{1,3}$/.test(ip)) {
            toast('Invalid IP format (e.g. 192.168.1.10)');
            return;
          }
          toast(`Connecting to ${ip}:${port}…`);
          try {
            const serial = await inv('sc_connect', { ip, port });
            await refreshDevices();
            await start(serial);
            return;
          } catch (err) {
            toast(`Could not connect: ${msg(err)}`);
            return;
          }
        }
        await refreshDevices();
        if (!activeDevice) {
          toast('No device found. Connect phone with USB or Wi-Fi first.');
          return;
        }
      }
      await start(activeDevice ? activeDevice.serial : null);
    }
  }));

  // ── Auto-split IP:Port if typed/pasted ──────────────────────────────────
  const ipEl = $('#sc-ip'), portEl = $('#sc-port');
  if (ipEl) {
    ipEl.addEventListener('input', () => {
      const val = ipEl.value.trim();
      if (val.includes(':')) {
        const parts = val.split(':');
        ipEl.value = parts[0].trim();
        if (parts[1] && parts[1].trim() && portEl) {
          portEl.value = parts[1].trim();
        }
      }
    });
  }

  // ── USB Cable ───────────────────────────────────────────────────────────
  $('#sc-usb-go').addEventListener('click', e => guard(e.currentTarget, async () => {
    let devs = [];
    try { devs = await inv('sc_devices'); } catch (_) {}
    const usb = (devs || []).find(d => !d.is_wireless && d.state === 'device');
    if (!usb) return toast('No USB phone found. Plug in phone and enable USB debugging.');
    activeDevice = usb;
    await start(usb.serial);
  }));

  // ── IP + port ───────────────────────────────────────────────────────────
  $('#sc-ip-go').addEventListener('click', e => guard(e.currentTarget, async () => {
    const ip = $('#sc-ip').value.trim(), port = parseInt($('#sc-port').value, 10) || 5555;
    if (!ip) return toast('Enter the phone IP address');
    if (!/^(\d{1,3}\.){3}\d{1,3}$/.test(ip)) {
      return toast('Invalid IP format (e.g. 192.168.1.10)');
    }
    toast(`Connecting to ${ip}:${port}…`);
    const serial = await inv('sc_connect', { ip, port });
    await refreshDevices();
    await start(serial);
  }));

  // ── Permanent connection ────────────────────────────────────────────────
  $('#sc-perm-go').addEventListener('click', e => guard(e.currentTarget, async () => {
    let devs = [];
    try { devs = await inv('sc_devices'); } catch (_) {}
    const d = (devs || []).find(x => x.state === 'device');
    if (!d) return toast('Plug in your phone with USB first');
    setSub('#sc-perm-sub', 'Configuring permanent Wi-Fi connection…');
    try {
      const c = await inv('sc_make_permanent', { serial: d.serial });
      $('#sc-ip').value = c.ip; $('#sc-port').value = c.port;
      setSub('#sc-perm-sub', `Locked to ${c.ip}:${c.port}. You can unplug USB now.`);
      toast(`Permanent Wi-Fi setup done (${c.ip}:${c.port})`);
      await refreshDevices();
    } catch (err) {
      setSub('#sc-perm-sub', 'Failed. Re-plug USB and try again.');
      throw err;
    }
  }));

  // ── QR ──────────────────────────────────────────────────────────────────
  function qrStop() {
    clearInterval(qrTimer); qrTimer = null;
    $('#sc-qr-row').hidden = true;
    $('#sc-qr-box').innerHTML = '';
  }
  $('#sc-qr-cancel').addEventListener('click', () => { inv('sc_qr_cancel').catch(() => {}); qrStop(); });
  $('#sc-qr-go').addEventListener('click', async () => {
    try {
      $('#sc-qr-box').innerHTML = await inv('sc_qr_start');
      $('#sc-qr-msg').textContent = 'Scan via phone Wireless debugging';
      $('#sc-qr-row').hidden = false;
      clearInterval(qrTimer);
      qrTimer = setInterval(async () => {
        let r; try { r = await inv('sc_qr_status'); } catch (e) { return; }
        const [st, m] = r;
        if (st === 'waiting') $('#sc-qr-msg').textContent = 'Scan via phone Wireless debugging';
        else if (st === 'pairing') $('#sc-qr-msg').textContent = m;
        else if (st === 'done') {
          qrStop();
          toast('Device paired successfully!');
          await refreshDevices();
          start(m);
        } else if (st === 'error') {
          qrStop();
          toast(m);
        }
      }, 1000);
    } catch (e) { toast(msg(e)); }
  });

  // ── Stop Scrcpy (Instant 0ms stop, NEVER kills system ADB) ───────────────
  window.stopScrcpy = () => {
    inv('sc_stop').catch(()=>{});
    inv('sc_qr_cancel').catch(()=>{});
    qrStop();
    refreshStatus();
  };

  // ── Lifecycle & Navigation Hooks ────────────────────────────────────────
  let initialized = false;
  async function init() {
    if (!initialized) {
      initialized = true;
      try {
        const v = await inv('sc_load_prefs');
        p = normalizePrefs(v);
      } catch (e) {
        p = { ...DEF };
      }
      paint();

      // Pre-fill saved IP into input fields without running commands
      try {
        const saved = await inv('sc_saved');
        if (saved && saved.ip) {
          if (!$('#sc-ip').value) $('#sc-ip').value = saved.ip;
          if (!$('#sc-port').value) $('#sc-port').value = saved.port || '5555';
          setSub('#sc-ip-sub', `Saved phone: ${saved.model || saved.ip}:${saved.port || 5555}`);
        }
      } catch (_) {}
    }
  }

  // Handle switching between tabs: only query ADB on 'connect' tab
  window.onScrcpyTab = async (tab) => {
    await init();
    paint();
    if (!tab || tab === 'connect') {
      await refreshDevices();
      await refreshStatus();
    } else {
      await refreshStatus();
    }
  };

  // On initial script load: load prefs and paint default/saved states
  init();
})();