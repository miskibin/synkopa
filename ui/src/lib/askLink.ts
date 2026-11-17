// "Ask AI": the prompt sent to ChatGPT and the link that carries it. The engine builds the
// same link (app/src/links.rs) and refuses one past MAX_ASK_URL, so the UI decides with the
// same arithmetic whether the question fits in a link or goes through the clipboard.

export type AskService = 'chatgpt' | 'deepwiki';

export const REPO_URL = 'https://github.com/miskibin/wave-DAW';
export const MAX_ASK_URL = 2000;
const CHATGPT_URL = 'https://chatgpt.com/?q=';
const CHATGPT_HOME = 'https://chatgpt.com/';
export const DEEPWIKI_URL = 'https://deepwiki.com/miskibin/wave-DAW';

/** Percent-encodes everything but RFC 3986 unreserved characters, like `links::encode_component`. */
export function encodeComponent(s: string): string {
  let out = '';
  for (const b of new TextEncoder().encode(s)) {
    const c = String.fromCharCode(b);
    out += /[A-Za-z0-9\-._~]/.test(c) ? c : '%' + b.toString(16).toUpperCase().padStart(2, '0');
  }
  return out;
}

/**
 * What ChatGPT gets: which app, where its code is, how to answer, then the question. The
 * instructions are English (the model follows them best) and ask for the question's language.
 */
export function askPrompt(question: string, ctx: { version: string | null; view: string | null }): string {
  const app = ctx.version ? `Synkopa ${ctx.version}` : 'Synkopa';
  const lines = [
    `I use ${app}, an open-source DAW: ${REPO_URL}`,
    'Check its source code before answering. Give concrete steps in its interface (menus, buttons, right-click menus, shortcuts).',
    "If Synkopa can't do it yet, say so plainly. Answer in the language of my question.",
  ];
  if (ctx.view) lines.push(`Open in Synkopa right now: ${ctx.view}.`);
  return `${lines.join(' ')}\n\nQuestion: ${question.trim()}`;
}

/** The link for `service`, or null when the prompt is too long for one (ChatGPT only). No
 *  prompt opens a new ChatGPT chat (the question was copied to the clipboard instead). */
export function askUrl(service: AskService, prompt: string | null): string | null {
  if (service === 'deepwiki') return DEEPWIKI_URL;
  if (!prompt?.trim()) return CHATGPT_HOME;
  const url = CHATGPT_URL + encodeComponent(prompt.trim());
  return url.length <= MAX_ASK_URL ? url : null;
}
