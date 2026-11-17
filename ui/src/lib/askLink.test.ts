import { describe, expect, it } from 'vitest';
import { askPrompt, askUrl, DEEPWIKI_URL, encodeComponent, MAX_ASK_URL } from './askLink';

describe('encodeComponent', () => {
  // the same vectors as app/src/links.rs: both sides must agree on a link's length
  it('matches the engine', () => {
    expect(encodeComponent('a-z_0.9~')).toBe('a-z_0.9~');
    expect(encodeComponent('jak dodać efekt?')).toBe('jak%20doda%C4%87%20efekt%3F');
    expect(encodeComponent('a&b=c/d#e\n')).toBe('a%26b%3Dc%2Fd%23e%0A');
    expect(encodeComponent("(x)!*'")).toBe('%28x%29%21%2A%27');
  });
  it('decodes back to the text', () => {
    const s = 'Czy da się przestawić efekty w mikserze? 🎛️ "x" & y';
    expect(decodeURIComponent(encodeComponent(s))).toBe(s);
  });
});

describe('askPrompt', () => {
  it('names the version, the repository, the view and the question', () => {
    const p = askPrompt('  jak dodać kanał?  ', { version: '0.4.0', view: 'Mixer' });
    expect(p).toContain('Synkopa 0.4.0');
    expect(p).toContain('https://github.com/miskibin/synkopa');
    expect(p).toContain('Open in Synkopa right now: Mixer.');
    expect(p.endsWith('Question: jak dodać kanał?')).toBe(true);
  });
  it('leaves out what it does not know', () => {
    const p = askPrompt('x', { version: null, view: null });
    expect(p).toContain('I use Synkopa, ');
    expect(p).not.toContain('right now');
  });
});

describe('askUrl', () => {
  it('puts the prompt in a ChatGPT link', () => {
    expect(askUrl('chatgpt', 'how?')).toBe('https://chatgpt.com/?q=how%3F');
  });
  it('gives up on a prompt past the link limit', () => {
    const base = 'https://chatgpt.com/?q='.length;
    expect(askUrl('chatgpt', 'a'.repeat(MAX_ASK_URL - base))!.length).toBe(MAX_ASK_URL);
    expect(askUrl('chatgpt', 'a'.repeat(MAX_ASK_URL - base + 1))).toBeNull();
    expect(askUrl('chatgpt', 'ą'.repeat(400))).toBeNull(); // 2400 characters encoded
  });
  it('a real question with the full prompt fits', () => {
    const p = askPrompt('Czy da się przestawić kolejność efektów w mikserze i jak to zrobić myszką albo klawiaturą?', { version: '0.4.0', view: 'Mixer' });
    expect(askUrl('chatgpt', p)).not.toBeNull();
  });
  it('no prompt opens a new ChatGPT chat', () => {
    expect(askUrl('chatgpt', null)).toBe('https://chatgpt.com/');
    expect(askUrl('chatgpt', '  ')).toBe('https://chatgpt.com/');
  });
  it('DeepWiki always opens the repository', () => {
    expect(askUrl('deepwiki', 'x'.repeat(5000))).toBe(DEEPWIKI_URL);
  });
});
