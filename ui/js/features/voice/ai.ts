import type { AiConfig, AiKeyInfo, AiLocalStatus, AiProposal } from '../../../types/models.js';
import { $, $$, setText } from '../../core/dom.js';
import { on } from '../../core/bus.js';
import { invoke, listen } from '../../core/tauri.js';
import { showToast } from '../../core/toast.js';
import { aiState } from '../../core/state.js';
import { saveSettings } from '../../core/settings-store.js';

const PROVIDER_DEFAULTS: Readonly<Record<string, string>> = {
  gemini: 'gemini-3.8-flash',
  groq: 'llama-3.3-70b-versatile',
  openrouter: 'openrouter/free',
  openai: 'gpt-4o-mini',
  claude: 'claude-3-5-haiku-latest',
};

let cooldownTimer: ReturnType<typeof setInterval> | undefined;

function stopCooldownTicker(): void {
  if (cooldownTimer !== undefined) {
    clearInterval(cooldownTimer);
    cooldownTimer = undefined;
  }
}

function startCooldownTicker(listEl: HTMLElement): void {
  stopCooldownTicker();
  const badges = $$<HTMLElement>('.ai-cd-badge[data-cd]', listEl);
  if (!badges.length) return;
  cooldownTimer = setInterval(() => {
    let anyLeft = false;
    badges.forEach(b => {
      const next = Math.max(0, (parseInt(b.dataset.cd || '0', 10) || 0) - 1);
      b.dataset.cd = String(next);
      if (next > 0) {
        b.textContent = `Cooldown ${next}s`;
        anyLeft = true;
      } else {
        b.remove();
      }
    });
    if (!anyLeft) stopCooldownTicker();
  }, 1000);
}

function esc(s: unknown): string {
  const map: Record<string, string> = {
    '&': '&amp;',
    '<': '&lt;',
    '>': '&gt;',
    '"': '&quot;',
    "'": '&#39;',
  };
  return String(s ?? '').replace(/[&<>"']/g, c => map[c] ?? c);
}

export function syncAiControls(): void {
  const map: Array<[string, boolean]> = [
    ['#toggle-ai-enabled', aiState.enabled],
    ['#toggle-ai-voice-reply', aiState.voice_reply],
    ['#toggle-ai-dictation', aiState.dictation_enabled],
    ['#toggle-ai-screen', aiState.screen_context],
    ['#toggle-ai-local', aiState.local_llm_enabled],
    ['#toggle-ai-terminal', aiState.allow_ai_terminal],
  ];
  for (const [sel, val] of map) {
    const el = $<HTMLInputElement>(sel);
    if (el) el.checked = Boolean(val);
  }
  const modelIn = $<HTMLInputElement>('#ai-local-model');
  if (modelIn && aiState.local_model) {
    modelIn.value = aiState.local_model;
  }
  const vsSel = $<HTMLSelectElement>('#ai-voice-style');
  if (vsSel) {
    vsSel.value = aiState.voice_style || 'google_natural';
  }
  $$<HTMLButtonElement>('#seg-ai-route .sg').forEach(btn => {
    btn.classList.toggle('on', btn.dataset.v === (aiState.routing_mode || 'hybrid'));
  });
  const isAiOff = !aiState.enabled;
  const pageAi = $('#page-voice-ai');
  if (pageAi) pageAi.classList.toggle('ai-off', isAiOff);
  $$(
    '#page-voice-ai .ai-subcard[data-ai-panel="assistant"] .row:not(.ai-master-row), #ai-subnav .ai-subtab:not([data-ai-tab="assistant"]), #page-voice-ai .ai-subcard:not([data-ai-panel="assistant"])'
  ).forEach(el => {
    el.inert = isAiOff;
  });
  if (isAiOff) {
    $$<HTMLButtonElement>('#ai-subnav .ai-subtab').forEach(b =>
      b.classList.toggle('on', b.dataset.aiTab === 'assistant')
    );
    $$<HTMLElement>('#page-voice-ai .ai-subcard').forEach(c =>
      c.classList.toggle('on', c.dataset.aiPanel === 'assistant')
    );
  }
}

async function refreshKeys(): Promise<void> {
  const listEl = $('#ai-key-list');
  if (!listEl) return;
  stopCooldownTicker();
  let keys: AiKeyInfo[] = [];
  try {
    keys = (await invoke<AiKeyInfo[]>('ai_list_keys')) || [];
  } catch {
    keys = [];
  }
  if (!keys.length) {
    listEl.innerHTML =
      '<div class="ai-empty">No API keys added yet. Keys are encrypted via Windows DPAPI and stay masked.</div>';
    return;
  }
  listEl.innerHTML = keys
    .map(k => {
      const cd =
        k.cooldown_secs > 0
          ? `<span class="ai-chip warn ai-cd-badge" data-cd="${k.cooldown_secs}">Cooldown ${k.cooldown_secs}s</span>`
          : '';
      return `<div class="ai-key-row" data-key-id="${esc(k.id)}">
        <div class="ai-key-main">
          <div class="ai-key-title">
            <b>${esc(k.label)}</b>
            <span class="ai-chip">${esc(k.provider)}</span>
            <input type="text" class="ai-model-inline" data-id="${esc(k.id)}" data-orig="${esc(k.model)}" value="${esc(k.model)}" title="Edit model ID and press Enter or blur to save" />
            <span class="ai-chip masked">${esc(k.masked)}</span>
            ${cd}
          </div>
        </div>
        <div class="ai-key-actions">
          <button type="button" class="btn ai-btn-test" data-id="${esc(k.id)}">Test</button>
          <label class="switch"><input type="checkbox" class="ai-key-toggle" data-id="${esc(k.id)}" ${k.enabled ? 'checked' : ''} /><span class="slider"></span></label>
          <button type="button" class="btn btn-danger ai-btn-del" data-id="${esc(k.id)}">Remove</button>
        </div>
      </div>`;
    })
    .join('');

  startCooldownTicker(listEl);

  $$<HTMLInputElement>('.ai-model-inline', listEl).forEach(inp => {
    const saveModel = async (): Promise<void> => {
      const next = inp.value.trim();
      const prev = inp.dataset.orig || '';
      if (!next) {
        inp.value = prev;
        return;
      }
      if (next === prev) return;
      try {
        await invoke('ai_update_key_model', { id: inp.dataset.id, model: next });
        inp.dataset.orig = next;
        showToast(`Model updated to ${next}`);
        void refreshKeys();
      } catch (err) {
        inp.value = prev;
        showToast(String(err || 'Model update failed'), { error: true });
      }
    };
    inp.addEventListener('keydown', ev => {
      if (ev.key === 'Enter') {
        ev.preventDefault();
        inp.blur();
      }
    });
    inp.addEventListener('blur', () => {
      void saveModel();
    });
  });

  $$<HTMLInputElement>('.ai-key-toggle', listEl).forEach(el => {
    el.addEventListener('change', async () => {
      await invoke('ai_set_key_enabled', { id: el.dataset.id, enabled: el.checked }).catch(() => {});
      showToast(el.checked ? 'API key enabled' : 'API key paused');
    });
  });

  $$<HTMLButtonElement>('.ai-btn-test', listEl).forEach(btn => {
    btn.addEventListener('click', async () => {
      btn.disabled = true;
      const orig = btn.textContent;
      btn.textContent = '...';
      try {
        const res = await invoke<string>('ai_test_key', { id: btn.dataset.id });
        showToast(res || 'API key works!');
      } catch (err) {
        showToast(String(err || 'Key test failed'), { error: true });
      } finally {
        btn.disabled = false;
        btn.textContent = orig;
        void refreshKeys();
      }
    });
  });

  $$<HTMLButtonElement>('.ai-btn-del', listEl).forEach(btn => {
    btn.addEventListener('click', async () => {
      await invoke('ai_remove_key', { id: btn.dataset.id }).catch(() => {});
      showToast('API key removed');
      void refreshKeys();
    });
  });
}

async function refreshLocalStatus(): Promise<void> {
  const statusEl = $('#ai-local-status');
  const modelsEl = $('#ai-local-models-chip');
  const btnInstall = $('#btn-ai-install-local');
  if (!statusEl) return;
  try {
    const st = await invoke<AiLocalStatus>('ai_local_status');
    if (!st) return;
    if (st.running) {
      statusEl.textContent = `Running · ${st.models?.length || 0} model(s) ready`;
      if (btnInstall) btnInstall.textContent = 'Ollama Ready';
    } else if (st.ollama_installed) {
      statusEl.textContent = 'Ollama installed · starts automatically on use';
      if (btnInstall) btnInstall.textContent = 'Start Ollama';
    } else {
      statusEl.textContent = 'Not installed · zero model bloat (on-demand setup only)';
      if (btnInstall) btnInstall.textContent = 'Setup Ollama';
    }
    if (modelsEl) {
      modelsEl.textContent = st.models?.length ? st.models.join(', ') : 'No models downloaded yet';
    }
  } catch {
    statusEl.textContent = 'Local LLM check unavailable';
  }
}

async function refreshProposals(): Promise<void> {
  const listEl = $('#ai-proposal-list');
  if (!listEl) return;
  let props: AiProposal[] = [];
  try {
    props = (await invoke<AiProposal[]>('ai_list_proposals')) || [];
  } catch {
    props = [];
  }
  if (!props.length) {
    listEl.innerHTML = '<div class="ai-empty">No pending PowerShell proposals.</div>';
    return;
  }
  listEl.innerHTML = props
    .map(
      p => `<div class="ai-prop-row" data-prop-id="${esc(p.id)}">
      <div class="ai-prop-text">
        <div class="ai-prop-reason">${esc(p.reason)}</div>
        <code class="ai-prop-cmd">${esc(p.command)}</code>
      </div>
      <div class="ai-key-actions">
        <button type="button" class="btn btn-danger ai-prop-run" data-id="${esc(p.id)}">Run</button>
        <button type="button" class="btn ai-prop-dismiss" data-id="${esc(p.id)}">Dismiss</button>
      </div>
    </div>`
    )
    .join('');

  $$<HTMLButtonElement>('.ai-prop-run', listEl).forEach(btn => {
    btn.addEventListener('click', async () => {
      btn.disabled = true;
      try {
        const out = await invoke<string>('ai_approve_proposal', { id: btn.dataset.id });
        setText('ai-chat-out', `Command Output:\n${out ?? ''}`);
        showToast('Command executed');
      } catch (err) {
        setText('ai-chat-out', `Error: ${err}`);
        showToast(String(err || 'Execution failed'));
      } finally {
        void refreshProposals();
      }
    });
  });

  $$<HTMLButtonElement>('.ai-prop-dismiss', listEl).forEach(btn => {
    btn.addEventListener('click', async () => {
      await invoke('ai_dismiss_proposal', { id: btn.dataset.id }).catch(() => {});
      void refreshProposals();
    });
  });
}

type AiBoolKey = {
  [K in keyof AiConfig]: AiConfig[K] extends boolean ? K : never;
}[keyof AiConfig];

export function initVoiceAiFeature(): void {
  const provSel = $<HTMLSelectElement>('#ai-key-provider');
  const modelIn = $<HTMLInputElement>('#ai-key-model');
  const labelIn = $<HTMLInputElement>('#ai-key-label');
  const secretIn = $<HTMLInputElement>('#ai-key-secret');

  if (provSel && modelIn && !modelIn.value) {
    modelIn.value = PROVIDER_DEFAULTS[provSel.value] || 'gemini-3.8-flash';
  }
  provSel?.addEventListener('change', () => {
    if (modelIn) modelIn.value = PROVIDER_DEFAULTS[provSel.value] || '';
  });

  $('#btn-ai-add-key')?.addEventListener('click', async () => {
    const provider = provSel?.value || 'gemini';
    const model = modelIn?.value?.trim() || PROVIDER_DEFAULTS[provider];
    const label = labelIn?.value?.trim() || `${provider} key`;
    const apiKey = secretIn?.value?.trim() || '';
    if (!apiKey) {
      showToast('Paste an API key first');
      return;
    }
    try {
      await invoke('ai_add_key', { label, provider, model, apiKey });
      if (secretIn) secretIn.value = '';
      if (labelIn) labelIn.value = '';
      showToast('Key encrypted & saved');
      void refreshKeys();
    } catch (err) {
      showToast(String(err || 'Failed adding key'));
    }
  });

  const bindSwitch = (sel: string, key: AiBoolKey, labelText: string): void => {
    $<HTMLInputElement>(sel)?.addEventListener('change', ev => {
      const inp = ev.target as HTMLInputElement;
      aiState[key] = Boolean(inp.checked);
      if (key === 'enabled') syncAiControls();
      void saveSettings(`${labelText} ${aiState[key] ? 'on' : 'off'}`);
    });
  };
  bindSwitch('#toggle-ai-enabled', 'enabled', 'AI Assistant');
  bindSwitch('#toggle-ai-voice-reply', 'voice_reply', 'Voice talkback');
  bindSwitch('#toggle-ai-dictation', 'dictation_enabled', 'Voice dictation');

  $<HTMLSelectElement>('#ai-voice-style')?.addEventListener('change', ev => {
    const sel = ev.target as HTMLSelectElement;
    aiState.voice_style = sel.value || 'google_natural';
    void saveSettings('Voice character updated');
  });
  bindSwitch('#toggle-ai-screen', 'screen_context', 'Screen OCR context');
  bindSwitch('#toggle-ai-local', 'local_llm_enabled', 'Local LLM');

  $$<HTMLButtonElement>('#seg-ai-route .sg').forEach(btn => {
    btn.addEventListener('click', () => {
      aiState.routing_mode = btn.dataset.v || 'hybrid';
      syncAiControls();
      void saveSettings('AI routing updated');
    });
  });

  $<HTMLInputElement>('#ai-local-model')?.addEventListener('change', ev => {
    const inp = ev.target as HTMLInputElement;
    aiState.local_model = inp.value.trim() || 'llama3.2:3b';
    void saveSettings('Local LLM model saved');
  });

  $<HTMLButtonElement>('#btn-ai-install-local')?.addEventListener('click', async ev => {
    const btn = ev.currentTarget as HTMLButtonElement;
    btn.disabled = true;
    setText('ai-local-progress', 'Checking / installing Ollama...');
    try {
      const msg = (await invoke<string>('ai_install_local_llm')) || '';
      setText('ai-local-progress', msg);
      showToast(msg);
      void refreshLocalStatus();
    } catch (err) {
      setText('ai-local-progress', String(err || 'Install failed'));
      showToast(String(err || 'Install failed'));
    } finally {
      btn.disabled = false;
    }
  });

  $<HTMLButtonElement>('#btn-ai-pull-model')?.addEventListener('click', async ev => {
    const btn = ev.currentTarget as HTMLButtonElement;
    const model = $<HTMLInputElement>('#ai-local-model')?.value?.trim() || 'llama3.2:3b';
    aiState.local_model = model;
    btn.disabled = true;
    setText('ai-local-progress', `Pulling ${model}...`);
    try {
      const msg = (await invoke<string>('ai_pull_local_model', { model })) || '';
      setText('ai-local-progress', msg);
      showToast(msg);
      void saveSettings(`Local model ${model} saved`);
      void refreshLocalStatus();
    } catch (err) {
      setText('ai-local-progress', String(err || 'Model download failed'));
      showToast(String(err || 'Download failed'));
    } finally {
      btn.disabled = false;
    }
  });

  const toggleTerm = $<HTMLInputElement>('#toggle-ai-terminal');
  const termModal = $('#ai-term-modal');
  const termAck = $<HTMLInputElement>('#ai-term-ack');
  const btnCancel = $('#btn-ai-term-cancel');
  const btnGrant = $<HTMLButtonElement>('#btn-ai-term-grant');

  const closeModal = (): void => {
    if (termModal) termModal.hidden = true;
    if (termAck) termAck.checked = false;
    if (btnGrant) btnGrant.disabled = true;
    if (toggleTerm) toggleTerm.checked = Boolean(aiState.allow_ai_terminal);
  };

  toggleTerm?.addEventListener('change', () => {
    if (toggleTerm.checked) {
      toggleTerm.checked = false;
      if (termAck) termAck.checked = false;
      if (btnGrant) btnGrant.disabled = true;
      if (termModal) termModal.hidden = false;
    } else {
      aiState.allow_ai_terminal = false;
      void saveSettings('AI terminal proposals disabled');
    }
  });

  termAck?.addEventListener('change', () => {
    if (btnGrant) btnGrant.disabled = !termAck.checked;
  });
  btnCancel?.addEventListener('click', closeModal);
  termModal?.addEventListener('click', ev => {
    if (ev.target === termModal) closeModal();
  });
  btnGrant?.addEventListener('click', () => {
    if (!termAck?.checked) return;
    aiState.allow_ai_terminal = true;
    if (toggleTerm) toggleTerm.checked = true;
    if (termModal) termModal.hidden = true;
    void saveSettings('AI terminal proposals enabled');
  });

  const runPrompt = async (): Promise<void> => {
    const inp = $<HTMLInputElement>('#ai-chat-input');
    const prompt = inp?.value?.trim();
    if (!prompt) return;
    const btn = $<HTMLButtonElement>('#btn-ai-ask');
    if (btn) btn.disabled = true;
    setText('ai-chat-out', 'Thinking...');
    setText('ai-chat-meta', '');
    try {
      const res = await invoke<{ reply?: string; meta?: string }>('ai_ask_text', { prompt });
      setText('ai-chat-out', res?.reply || '(No reply)');
      setText('ai-chat-meta', res?.meta || '');
      if (inp) inp.value = '';
      void refreshProposals();
    } catch (err) {
      setText('ai-chat-out', `Error: ${err}`);
    } finally {
      if (btn) btn.disabled = false;
    }
  };

  $('#btn-ai-ask')?.addEventListener('click', () => {
    void runPrompt();
  });
  $('#ai-chat-input')?.addEventListener('keydown', ev => {
    if ((ev as KeyboardEvent).key === 'Enter') void runPrompt();
  });
  $('#btn-ai-clear-chat')?.addEventListener('click', async () => {
    await invoke('ai_clear_chat').catch(() => {});
    setText('ai-chat-out', 'Ask Nexus AI anything, check screen errors, or chat naturally.');
    setText('ai-chat-meta', '');
  });

  void listen<{ message?: string }>('ai-local-progress', ev => {
    if (ev?.payload?.message) setText('ai-local-progress', ev.payload.message);
  });
  void listen<{ reply?: string; meta?: string }>('ai-reply', ev => {
    const p = ev?.payload;
    if (!p?.reply) return;
    setText('ai-chat-out', p.reply);
    if (p.meta) setText('ai-chat-meta', p.meta);
  });
  void listen('ai-terminal-proposal', () => {
    void refreshProposals();
  });

  on('settings:sync', () => syncAiControls());
  on('tab:changed', ev => {
    if (ev?.module === 'voice' && ev?.tab === 'ai') {
      void refreshKeys();
      void refreshLocalStatus();
      void refreshProposals();
    } else {
      stopCooldownTicker();
    }
  });

  $$<HTMLButtonElement>('#ai-subnav .ai-subtab').forEach(tabBtn => {
    tabBtn.addEventListener('click', () => {
      const target = tabBtn.dataset.aiTab || 'assistant';
      $$<HTMLButtonElement>('#ai-subnav .ai-subtab').forEach(b =>
        b.classList.toggle('on', b.dataset.aiTab === target)
      );
      $$<HTMLElement>('#page-voice-ai .ai-subcard').forEach(card =>
        card.classList.toggle('on', card.dataset.aiPanel === target)
      );
    });
  });

  syncAiControls();
  void refreshKeys();
  void refreshLocalStatus();
  void refreshProposals();
}
