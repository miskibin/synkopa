import { beforeAll, describe, expect, it } from 'vitest';
import type { AudioStatus } from '../api/types';
import { MockBackend } from '../api/mock';
import { audioNotice, audioStatus, audioToast, defaultAudioConfig, dismissAudioToast, watchAudio } from './audioWatch';

beforeAll(() => { (globalThis as { window?: unknown }).window ??= {}; });

const tick = () => new Promise(r => setTimeout(r, 0));
const base = { running: true, error: null, note: null, restarts: 0 } as AudioStatus;

describe('audio that stops or returns by itself', () => {
  it('is quiet while nothing changes', () => {
    expect(audioNotice(base, { ...base })).toBeNull();
    expect(audioNotice(null, base)).toBeNull();
  });
  it('says when the output was reopened, with the reason', () => {
    const m = audioNotice(base, { ...base, restarts: 1, note: 'Audio restarted by itself: device gone.' });
    expect(m).toContain('device gone');
  });
  it('says when audio could not come back, once per error', () => {
    const down = { ...base, running: false, error: 'Audio stopped (reopening failed). Retrying…' };
    expect(audioNotice(base, down)).toContain('Retrying');
    expect(audioNotice(down, { ...down })).toBeNull();
  });
  it('follows the mock backend like the app', async () => {
    const be = new MockBackend(); const said: string[] = [];
    await watchAudio(() => be.audioGetConfig(), cb => be.on('audio-status', cb), m => said.push(m));
    be.audioDeviceLost();
    await tick();
    expect(said).toHaveLength(1);
    expect(said[0]).toContain('no longer available');
    // a settings change the user made is not a "restarted by itself"
    await be.audioSetConfig({ ...(await be.audioGetConfig()), bufferSize: 256 });
    await tick();
    expect(said).toHaveLength(1);
  });
  it('shows a missing saved ASIO device at startup without repeating the toast', async () => {
    const be = new MockBackend();
    const cfg = { ...defaultAudioConfig('asio'), output: 'Focusrite USB ASIO' };
    await be.setFocusritePresent(false);
    await be.audioSetConfig(cfg);
    const said: string[] = [];
    await watchAudio(() => be.audioGetConfig(), cb => be.on('audio-status', cb), m => said.push(m));
    expect(audioToast()?.unavailableOutput).toBe(cfg.output);
    dismissAudioToast();
    await be.audioSetConfig(cfg);
    await tick();
    expect(said).toEqual(['Focusrite USB ASIO is unavailable.']);
    expect(audioToast()).toBeNull();
  });
  it('stops recording and lets the user recover on WASAPI or retry a reconnected ASIO device', async () => {
    const be = new MockBackend();
    const cfg = { ...defaultAudioConfig('asio'), output: 'Focusrite USB ASIO' };
    await be.audioSetConfig(cfg);
    await watchAudio(() => be.audioGetConfig(), cb => be.on('audio-status', cb), () => {});
    await be.transportSet({ rec: true, monitor: true });
    await be.transportPlay(0);
    await be.setFocusritePresent(false);
    await tick();
    expect((await be.getState()).transport).toMatchObject({ playing: false, rec: false, monitor: false });
    expect(audioStatus()).toMatchObject({ running: false, unavailableOutput: cfg.output });
    expect(audioToast()?.unavailableOutput).toBe(cfg.output);
    expect((await be.audioSetConfig(defaultAudioConfig('wasapi'))).running).toBe(true);
    await tick();
    expect(audioToast()).toBeNull();
    await be.setFocusritePresent(true);
    expect((await be.audioSetConfig(cfg)).running).toBe(true);
    await be.setFocusritePresent(false);
    await tick();
    expect(audioToast()?.unavailableOutput).toBe(cfg.output);
    be.setWasapiOutputPresent(false);
    expect(await be.audioSetConfig(defaultAudioConfig('wasapi'))).toMatchObject({ running: false, error: 'no output device' });
  });
  it('keeps a device event received while the initial snapshot is in flight', async () => {
    const initial = { ...base, host: 'asio' };
    const disconnected = { ...initial, running: false, unavailableOutput: 'Focusrite USB ASIO' };
    let emit = (_: AudioStatus) => {};
    await watchAudio(async () => {
      emit(disconnected);
      return initial;
    }, async cb => { emit = cb; }, () => {});
    expect(audioStatus()).toBe(disconnected);
  });
});

describe('play while the output is not open', () => {
  it('names the reason only when audio is known to be down', async () => {
    const { noAudioReason } = await import('./audioWatch');
    expect(noAudioReason(null)).toBeNull();
    expect(noAudioReason(base)).toBeNull();
    const err = 'Headphones: Failed to get audio client';
    expect(noAudioReason({ ...base, running: false, error: err })).toBe(err);
    expect(noAudioReason({ ...base, running: false, unavailableOutput: 'Focusrite USB ASIO' })).toBe('Focusrite USB ASIO');
    // starting up, nothing failed: no warning
    expect(noAudioReason({ ...base, running: false, unavailableOutput: null })).toBeNull();
  });
  it('shows a notice with a retry and the audio settings', async () => {
    const { warnIfNoAudio } = await import('./audioWatch');
    const { notice } = await import('./notice');
    const be = new MockBackend();
    await watchAudio(() => be.audioGetConfig(), cb => be.on('audio-status', cb), () => {});
    expect(warnIfNoAudio()).toBe(false);
    be.audioOpenFailed('Headphones: Failed to get audio client');
    await tick();
    expect(warnIfNoAudio()).toBe(true);
    expect(notice()?.msg).toContain('Failed to get audio client');
    expect(notice()?.actions.map(a => a.label)).toEqual(['Try again', 'Audio settings']);
  });
});
