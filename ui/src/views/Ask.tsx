// "Ask AI": a question about Synkopa goes to an AI assistant that reads Synkopa's open-source code.
// ChatGPT gets the whole prompt in its link; DeepWiki's links take no question, so the question
// is copied and the user pastes it there (as with a ChatGPT question too long for a link).
import { createSignal, For, Show } from 'solid-js';
import { be } from '../api/backend';
import { askPrompt, askUrl, type AskService } from '../lib/askLink';
import { t } from '../lib/i18n';
import { say, tucked, U } from '../state/ui';
import { rackFloating, rollFloating } from '../state/pattern';
import { APP_GLYPH } from '../ui/menuIcons';

const SERVICE_KEY = 'wave.ask.service';
const loadService = (): AskService => {
  try { return localStorage.getItem(SERVICE_KEY) === 'deepwiki' ? 'deepwiki' : 'chatgpt'; } catch { return 'chatgpt'; }
};

const [open, setOpen] = createSignal(false);
const [service, setServiceSig] = createSignal<AskService>(loadService());
const [question, setQuestion] = createSignal('');
const [error, setError] = createSignal<string | null>(null);
const [busy, setBusy] = createSignal(false);
let version: string | null = null;

export const openAsk = () => {
  setError(null);
  setOpen(true);
  if (version == null) be().appInfo().then(i => { version = i.version; }).catch(() => {});
};
/** Opens the Ask dialog with `text` as the question (the command palette's "Ask AI"): the user reads where it goes and sends it there. */
export const askAbout = (text: string) => { setQuestion(text); openAsk(); };
export const toggleAsk = () => (open() ? setOpen(false) : openAsk());
const close = () => { if (!busy()) setOpen(false); };

function setService(s: AskService) {
  setServiceSig(s);
  setError(null);
  try { localStorage.setItem(SERVICE_KEY, s); } catch { /* private window */ }
}

/** What the user is looking at, in English for the prompt (names as Synkopa's menus show them). */
function currentView(): string {
  const parts = [U.mixView ? 'Mix view (F9)' : 'Playlist'];
  if (U.rackWin || rackFloating()) parts.push('Channel rack');
  if (U.rollWin || rollFloating()) parts.push('Piano roll');
  if (U.plugs.some(p => !tucked(p.z))) parts.push('an effect window');
  return parts.join(', ');
}

const prompt = () => askPrompt(question(), { version, view: currentView() });
/** The question has to be pasted by hand: DeepWiki, or a ChatGPT link that would be too long. */
const viaClipboard = () => service() === 'deepwiki' || (question().trim() !== '' && askUrl('chatgpt', prompt()) == null);

async function send() {
  const q = question().trim();
  if (!q || busy()) return;
  setBusy(true); setError(null);
  try {
    const paste = viaClipboard();
    if (paste) {
      // DeepWiki reads the code itself: the bare question; ChatGPT needs the context too
      try { await navigator.clipboard.writeText(service() === 'deepwiki' ? q : prompt()); }
      catch (e) { setError(t('d.ask.copyFailed', { error: String(e) })); return; }
    }
    await be().openAsk(service(), paste ? null : prompt());
    say(paste ? t('d.ask.sentPaste') : t('d.ask.sent'));
    setQuestion('');
    setOpen(false);
  } catch (e) {
    setError(t('d.ask.openFailed', { error: String(e) }));
  } finally {
    setBusy(false);
  }
}

/** The menu-row button: always visible, labelled, next to the application menus. */
export function AskButton() {
  return (
    <button class="askbtn" classList={{ on: open() }} title={t('d.ask.tip')} aria-haspopup="dialog" aria-expanded={open()} onClick={toggleAsk}>
      <svg width="13" height="13" viewBox={APP_GLYPH.ask.vb} aria-hidden="true"><path d={APP_GLYPH.ask.d} fill="currentColor" /></svg>
      <span>{t('d.ask.button')}</span>
    </button>
  );
}

export function AskDialog() {
  const services: [AskService, string][] = [['chatgpt', 'ChatGPT'], ['deepwiki', 'DeepWiki']];
  return (
    <Show when={open()}>
      <div class="overlay" style={{ 'z-index': 250 }} onPointerDown={close}>
        <div class="dialog askdlg" role="dialog" aria-modal="true" aria-labelledby="askhd" onPointerDown={e => e.stopPropagation()}
          onKeyDown={e => { e.stopPropagation(); if (e.key === 'Escape') close(); }}>
          <div class="hd"><span id="askhd">{t('d.ask.title')}</span><button aria-label={t('d.upd.btn.close')} onClick={close}>×</button></div>
          <p class="askintro">{t('d.ask.intro')}</p>
          <textarea class="askq" rows={4} value={question()} placeholder={t('d.ask.placeholder')} aria-label={t('d.ask.title')}
            ref={el => setTimeout(() => el.focus())}
            onInput={e => { setQuestion(e.currentTarget.value); setError(null); }}
            onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); void send(); } }} />
          <div class="askto">
            <span class="cl">{t('d.ask.to')}</span>
            <div class="seg asksvc" role="radiogroup" aria-label={t('d.ask.to')}>
              <For each={services}>{([id, name]) => (
                <button role="radio" aria-checked={service() === id} classList={{ on: service() === id }} onClick={() => setService(id)}>{name}</button>
              )}</For>
            </div>
          </div>
          <div class="asknote" classList={{ warn: viaClipboard() }}>
            {service() === 'deepwiki' ? t('d.ask.noteDeepwiki') : viaClipboard() ? t('d.ask.noteLong') : t('d.ask.noteChatgpt')}
          </div>
          <Show when={error()}><div class="askerr" role="alert">{error()}</div></Show>
          <div class="askfoot">
            <span class="askpriv">{t('d.ask.privacy')}</span>
            <button class="smallbtn" onClick={close}>{t('d.ask.cancel')}</button>
            <button class="smallbtn primary" disabled={!question().trim() || busy()} onClick={() => void send()}
              title={t('d.ask.sendTip')}>{viaClipboard() ? t('d.ask.sendPaste') : t('d.ask.send')}</button>
          </div>
        </div>
      </div>
    </Show>
  );
}
