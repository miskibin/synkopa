// "Unsaved changes?" before New / Open / closing, and the crash-recovery offer.
import { fireEvent, render, screen } from '@solidjs/testing-library';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { t } from '../lib/i18n';
import { setS } from '../state/store';
import { confirmRecover, confirmUnsaved, ConfirmDialog } from './Confirm';
import { modalOwnsKey } from './modal';

beforeEach(() => { vi.useFakeTimers({ toFake: ['setTimeout'] }); setS({ dirty: true, path: 'C:\\Music\\Song.synkopa' }); render(() => <ConfirmDialog />); });
afterEach(() => {
  // answer whatever a failed test left open: the question is module state
  for (const d of document.querySelectorAll('.dialog')) fireEvent.keyDown(d, { key: 'Escape' });
  vi.useRealTimers();
});

const btn = (key: Parameters<typeof t>[0]) => screen.getByRole('button', { name: t(key) });

describe('confirmUnsaved', () => {
  it('answers "discard" at once when nothing is unsaved, without asking', async () => {
    setS('dirty', false);
    await expect(confirmUnsaved('opening another project')).resolves.toBe('discard');
    expect(screen.queryByText(t('d.ui.unsaved'))).toBeNull();
  });

  it('asks, naming the project and what is about to happen, and focuses Save', async () => {
    const answer = confirmUnsaved('opening another project');
    expect(await screen.findByText(t('d.ui.unsaved'))).toBeTruthy();
    expect(screen.getByText(/Song\.synkopa/).textContent).toContain('opening another project');
    vi.runAllTimers();
    expect(document.activeElement).toBe(btn('d.ui.save'));
    fireEvent.click(btn('d.ui.save'));
    await expect(answer).resolves.toBe('save');
    expect(screen.queryByText(t('d.ui.unsaved'))).toBeNull();
  });

  it.each([
    ['d.ui.discard', 'discard'],
    ['d.ui.cancel', 'cancel'],
  ] as const)('the %s button answers %s', async (key, want) => {
    const answer = confirmUnsaved('closing');
    fireEvent.click(await screen.findByRole('button', { name: t(key) }));
    await expect(answer).resolves.toBe(want);
  });

  it('Enter saves, Escape cancels, a press outside cancels', async () => {
    let a = confirmUnsaved('closing');
    fireEvent.keyDown(await screen.findByText(t('d.ui.unsaved')), { key: 'Enter' });
    await expect(a).resolves.toBe('save');
    a = confirmUnsaved('closing');
    fireEvent.keyDown(await screen.findByText(t('d.ui.unsaved')), { key: 'Escape' });
    await expect(a).resolves.toBe('cancel');
    a = confirmUnsaved('closing');
    fireEvent.pointerDown((await screen.findByText(t('d.ui.unsaved'))).closest('.overlay')!);
    await expect(a).resolves.toBe('cancel');
  });

  it('keys typed in the dialog do not reach the app shortcuts', async () => {
    void confirmUnsaved('closing');
    await screen.findByText(t('d.ui.unsaved'));
    await new Promise(r => queueMicrotask(() => r(null)));
    // App.tsx's key handler asks this first
    expect(modalOwnsKey(new KeyboardEvent('keydown', { key: 'Delete' }))).toBe(true);
    expect(modalOwnsKey(new KeyboardEvent('keydown', { key: ' ' }))).toBe(true);
    fireEvent.click(btn('d.ui.cancel'));
  });

  it('is a modal alert dialog whose Tab stays in it', async () => {
    void confirmUnsaved('closing');
    const dlg = await screen.findByRole('alertdialog', { name: t('d.ui.unsaved') });
    expect(dlg.getAttribute('aria-modal')).toBe('true');
    vi.runAllTimers();
    btn('d.ui.save').focus();
    fireEvent.keyDown(btn('d.ui.save'), { key: 'Tab' });
    expect(dlg.contains(document.activeElement)).toBe(true);
    fireEvent.click(btn('d.ui.cancel'));
  });

  it('a second question while one is open answers "busy" and leaves the first actionable', async () => {
    const first = confirmUnsaved('closing');
    await expect(confirmUnsaved('opening')).resolves.toBe('busy');
    fireEvent.click(await screen.findByRole('button', { name: t('d.ui.discard') }));
    await expect(first).resolves.toBe('discard');
  });
});

describe('confirmRecover', () => {
  it('offers Recover (focused, Enter), Delete copy and Not now (Escape)', async () => {
    let a = confirmRecover(Date.UTC(2026, 0, 2, 10, 0), null, 'C:\\Music\\Song.synkopa');
    const dlg = await screen.findByRole('alertdialog');
    expect(dlg.textContent).toContain('Song.synkopa');
    vi.runAllTimers();
    expect(document.activeElement).toBe(btn('d.ui.recover.restore'));
    fireEvent.keyDown(dlg, { key: 'Enter' });
    await expect(a).resolves.toBe('restore');
    a = confirmRecover(0);
    fireEvent.keyDown(await screen.findByRole('alertdialog'), { key: 'Escape' });
    await expect(a).resolves.toBe('later');
    a = confirmRecover(0);
    fireEvent.click(await screen.findByRole('button', { name: t('d.ui.recover.discard') }));
    await expect(a).resolves.toBe('discard');
    expect(screen.queryByRole('alertdialog')).toBeNull();
  });

  it('says that recovering replaces the project open now', async () => {
    void confirmRecover(0, 'D:\\Other.synkopa');
    expect((await screen.findByRole('alertdialog')).textContent).toContain('Other.synkopa');
    fireEvent.click(btn('d.ui.recover.later'));
  });
});
