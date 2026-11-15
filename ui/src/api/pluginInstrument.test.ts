import { beforeAll, describe, expect, it, vi } from 'vitest';
import { MockBackend } from './mock';
import type { Channel, PluginInfo } from './types';

beforeAll(() => { vi.stubGlobal('window', {}); });

type PluginChannel = Extract<Channel, { kind: 'plugin' }>;
const channelOf = (id: number, p: PluginInfo): PluginChannel => ({
  id, name: p.name, color: '#9b7cf8', kind: 'plugin',
  plugin: { format: 'vst3', path: p.path, cid: p.cid, name: p.name, vendor: p.vendor },
  vol: 0.7937, pan: 0, mute: false, solo: false, insert: null, root: 60,
});

// the mock must behave like the engine for plugin-instrument channels (crates/engine/tests/plugin_instruments.rs)
describe('plugin instruments in the mock', () => {
  it('lists VST3 instruments apart from the effects', async () => {
    const be = new MockBackend();
    const r = await be.pluginScan(false);
    expect(r.instruments?.length).toBeGreaterThan(0);
    expect(r.instruments!.every(i => i.format === 'vst3' && i.category.startsWith('Instrument'))).toBe(true);
    expect(r.plugins.some(p => p.category.startsWith('Instrument'))).toBe(false);
  });

  it('adds a plugin channel whose parameters, editor and state answer by channel id', async () => {
    const be = new MockBackend();
    await be.projectNew();
    const inst = (await be.pluginScan(false)).instruments![0];
    await be.applyOps([{ op: 'putChannel', channel: channelOf(7001, inst) }]);
    const st = await be.getState();
    expect(st.project.channels?.find(c => c.id === 7001)?.kind).toBe('plugin');
    expect(st.fxErrors?.[7001]).toBeUndefined();
    const ps = await be.pluginParams(7001);
    expect(ps.length).toBeGreaterThan(0);
    await be.pluginSetParam(7001, ps[0].id, 0.25);
    expect((await be.pluginParams(7001))[0].value).toBe(0.25);
    await expect(be.pluginEditor(7001, true)).resolves.toBeUndefined();
    expect(JSON.parse(atob(await be.pluginState(7001)))[0]).toEqual([ps[0].id, 0.25]);
  });

  it('reports a plugin that is not installed under the channel id and keeps the channel', async () => {
    const be = new MockBackend();
    await be.projectNew();
    const missing: PluginInfo = { format: 'vst3', path: 'C:\\VST3\\Gone.vst3', cid: 'ff'.repeat(16), name: 'Gone', vendor: 'Nobody', category: 'Instrument' };
    await be.applyOps([{ op: 'putChannel', channel: { ...channelOf(7002, missing), plugin: { ...channelOf(7002, missing).plugin, state: 'c2F2ZWQ=' } } }]);
    const st = await be.getState();
    expect(st.fxErrors?.[7002]).toMatch(/Gone/);
    const ch = st.project.channels?.find(c => c.id === 7002);
    expect(ch?.kind === 'plugin' && ch.plugin.state).toBe('c2F2ZWQ=');
    await expect(be.pluginParams(7002)).rejects.toThrow(/not a loaded plugin/);
    await be.applyOps([{ op: 'removeChannel', id: 7002 }]);
    expect((await be.getState()).fxErrors?.[7002]).toBeUndefined();
  });

  it('refuses instruments the engine cannot host', async () => {
    const be = new MockBackend();
    await be.projectNew();
    const inst = (await be.pluginScan(false)).instruments![0];
    const lv2 = { ...channelOf(7003, inst), plugin: { ...channelOf(7003, inst).plugin, format: 'lv2' as const } };
    await expect(be.applyOps([{ op: 'putChannel', channel: lv2 }])).rejects.toThrow(/VST3 only/);
    const blank = { ...channelOf(7003, inst), plugin: { ...channelOf(7003, inst).plugin, cid: ' ' } };
    await expect(be.applyOps([{ op: 'putChannel', channel: blank }])).rejects.toThrow(/class id/);
  });
});
