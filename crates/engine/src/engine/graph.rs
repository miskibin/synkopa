//! Turns the project into the structures the audio thread runs: graph and timeline
//! builders, plugin/synth sync, and playback latency (PDC) math.

use super::*;

impl Engine {
    /// Drops every plugin-instrument instance (the next sync makes new ones from the saved
    /// states). The audio thread's are removed by command (a rebuilt stream has none).
    pub(crate) fn forget_plugs(&self, c: &mut Core) {
        c.plugin_gen += 1; // every hosted instance goes away; in-flight latency reports are stale
        for id in c.plugs.drain().map(|(id, _)| id).collect::<Vec<_>>() {
            self.factory.release(id);
            c.fx_errors.remove(&id);
            send(c, Cmd::PlugRemove(id));
        }
    }

    pub(crate) fn snapshot_plugin_states(&self, project: &mut Project) {
        let factory = &self.factory;
        for m in project.inserts.iter_mut() {
            for f in m.fx.iter_mut() {
                if let Some(pl) = f.plugin.as_mut() {
                    if let Some(st) = factory.state(f.id) {
                        pl.state = Some(st);
                    }
                }
            }
        }
        for ch in project.channels.iter_mut() {
            if let ChannelSrc::Plugin { plugin } = &mut ch.src {
                if let Some(st) = factory.state(ch.id) {
                    plugin.state = Some(st);
                }
            }
        }
    }

    /// Pushes the project to the audio thread.
    pub(crate) fn sync(&self, c: &mut Core, d: Dirty) {
        self.refresh_midi(c);
        let Some(w) = c.wires.as_ref() else { return };
        let (sr, max_block) = (w.sr, w.max_block);
        // synth instances first, so a new timeline never plays a channel without one
        sync_synths(c, sr);
        sync_plugs(c, sr, max_block, &*self.factory);
        // a take's alignment assumes the latency at its start: hold latency changes back
        let defer = d.mixer && c.take.as_ref().is_some_and(|t| t.pdc != playback_latency_with(&c.project, sr, &c.plugin_lat));
        c.graph_deferred |= defer;
        if d.mixer && !defer {
            c.graph_deferred = false;
            // the rebuild can replace hosted instances: a latency report that was being
            // collected while this runs belongs to the old instances (refresh_plugin_latency)
            c.plugin_gen += 1;
            // by id and kind: an id whose kind changed is a different effect, whose processor
            // must be rebuilt, not carried (the builder only skips ids it holds as live)
            let now: HashMap<Id, FxKind> = c.project.inserts.iter().flat_map(|m| m.fx.iter().map(|f| (f.id, f.kind))).collect();
            let stale: Vec<Id> = c.live_fx.iter().filter_map(|(id, k)| (now.get(id) != Some(k)).then_some(*id)).collect();
            for gone in stale {
                self.factory.release(gone);
                c.fx_errors.remove(&gone);
            }
            let mut errs = std::mem::take(&mut c.fx_errors);
            let mut hosted = std::mem::take(&mut c.plugin_lat);
            let mut graph = build_graph(&c.project, &c.live_fx, sr, max_block, &*self.factory, true, &mut errs, &mut hosted);
            graph.monitor_track = monitor_of(c);
            c.fx_errors = errs;
            c.plugin_lat = hosted;
            c.live_fx = graph.inserts.iter().flat_map(|i| i.fx.iter().map(|s| (s.id, s.kind))).collect();
            c.sent_params = c.project.inserts.iter().flat_map(|m| m.fx.iter()).map(|f| (f.id, fx_send(f))).collect();
            // fresh processors got their maps in `build_graph`; carried ones keep what they had
            let fresh: HashSet<Id> = graph.inserts.iter().flat_map(|i| i.fx.iter()).filter(|s| s.proc.is_some()).map(|s| s.id).collect();
            c.sent_maps.retain(|id, _| now.contains_key(id) && !fresh.contains(id));
            for f in c.project.inserts.iter().flat_map(|m| m.fx.iter()) {
                if let (true, Some(m)) = (fresh.contains(&f.id), f.maps.as_ref()) {
                    c.sent_maps.insert(f.id, m.clone());
                }
            }
            send(c, Cmd::Graph(Box::new(graph)));
        } else if d.params || defer {
            let mut m = mix_state(&c.project);
            m.monitor_track = monitor_of(c);
            send(c, Cmd::Mix(Box::new(m)));
            let changed: Vec<(Id, (Option<ParamBlock>, f32))> = c
                .project
                .inserts
                .iter()
                .flat_map(|m| m.fx.iter())
                .map(|f| (f.id, fx_send(f)))
                .filter(|(id, pb)| c.sent_params.get(id) != Some(pb))
                .collect();
            for (id, (params, mix)) in changed {
                c.sent_params.insert(id, (params, mix));
                send(c, Cmd::Params { fx: id, params, mix });
            }
        }
        if d.mixer || d.params || defer {
            let changed: Vec<(Id, crate::model::WarpMaps)> = c
                .project
                .inserts
                .iter()
                .flat_map(|m| m.fx.iter())
                .filter_map(|f| f.maps.as_ref().map(|m| (f.id, m)))
                .filter(|(id, m)| c.sent_maps.get(id) != Some(*m))
                .map(|(id, m)| (id, m.clone()))
                .collect();
            for (id, m) in changed {
                send(c, Cmd::FxData { fx: id, data: Box::new(m.clone()) });
                c.sent_maps.insert(id, m);
            }
        }
        if d.timeline {
            let gesture_open = c.gesture.is_some();
            let audition = live_audition(c);
            let Core { project, sources, keys, .. } = c;
            let stale = gesture_open;
            let mut takes = comp_audio(sources, &project.comps);
            takes.extend(sample_audio(sources, &project.channels));
            let bpm = project.bpm;
            let tl = build_timeline(project, &mut |clip| sources.clip_play(clip, stale, bpm), &takes, &audition, keys.as_ref());
            send(c, Cmd::Timeline(Box::new(tl)));
        }
    }

    /// Asks the live hosted plugins for their latency again; when one changed, the
    /// mixer graph is rebuilt (running instances are kept) so the compensation follows.
    /// The factory is asked without the engine lock: it may wait for the UI thread.
    /// Returns whether anything changed.
    pub fn refresh_plugin_latency(&self) -> bool {
        let (ids, gen): (Vec<Id>, u64) = {
            let c = self.core.lock();
            if c.wires.is_none() {
                return false;
            }
            (c.plugin_lat.keys().copied().collect(), c.plugin_gen)
        };
        if ids.is_empty() {
            return false;
        }
        let now: Vec<(Id, usize)> = ids.into_iter().filter_map(|id| self.factory.latency(id).map(|l| (id, l))).collect();
        let mut c = self.core.lock();
        // the query ran without the lock: if the instances were replaced meanwhile, the
        // reports answer for the old ones — drop the whole batch, the next poll re-asks
        if c.plugin_gen != gen {
            return false;
        }
        let mut changed = false;
        for (id, l) in now {
            if let Some(old) = c.plugin_lat.get_mut(&id) {
                if *old != l {
                    log::info!("effect {id}: plugin latency {old} -> {l} samples; re-planning delay compensation");
                    *old = l;
                    changed = true;
                }
            }
        }
        if changed {
            self.sync(&mut c, Dirty { mixer: true, ..Default::default() });
        }
        changed
    }

    /// Tries every hosted plugin that did not load once more (an effect or a plugin
    /// instrument with an `fx_errors` entry): after the user chose to load one that Synkopa's
    /// safety check refused, or installed what was missing. Running instances are kept.
    pub fn retry_plugins(&self) {
        let mut c = self.core.lock();
        let failed: Vec<Id> = c.fx_errors.keys().copied().collect();
        if failed.is_empty() {
            return;
        }
        for id in failed {
            // an instrument channel is made again by `sync_plugs`; an effect by the graph build,
            // which makes only the slots it does not already hold (a failed one is held empty)
            c.plugs.remove(&id);
            c.live_fx.remove(&id);
        }
        self.sync(&mut c, Dirty { mixer: true, ..Default::default() });
        self.changed(c);
    }

    /// Frames between the timeline and the speakers inside the mixer of the running audio
    /// half, hosted plugins included (`playback_latency` counts built-in effects only).
    pub fn playback_latency(&self) -> usize {
        let c = self.core.lock();
        let sr = c.wires.as_ref().map_or(48_000.0, |w| w.sr);
        playback_latency_with(&c.project, sr, &c.plugin_lat)
    }
}

/// Loaded audio of every take source of `comps`.
pub(crate) fn comp_audio(store: &SourceStore, comps: &[Comp]) -> HashMap<Id, Arc<Audio>> {
    comps.iter().flat_map(|cp| cp.takes.iter()).filter_map(|t| store.get(t.source).map(|a| (t.source, a.clone()))).collect()
}

/// Loaded audio of every sampler channel.
pub(crate) fn sample_audio(store: &SourceStore, chans: &[Channel]) -> HashMap<Id, Arc<Audio>> {
    chans
        .iter()
        .filter_map(|c| match c.src {
            ChannelSrc::Sample { source, .. } => store.get(source).map(|a| (source, a.clone())),
            ChannelSrc::Synth { .. } | ChannelSrc::Keys { .. } | ChannelSrc::Plugin { .. } => None,
        })
        .collect()
}

/// Auditions that still point at an existing take.
pub(crate) fn live_audition(c: &Core) -> HashMap<Id, Id> {
    c.audition.iter().filter(|(cp, t)| c.project.comp(**cp).is_some_and(|x| x.take(**t).is_some())).map(|(a, b)| (*a, *b)).collect()
}

fn fx_send(f: &Fx) -> (Option<ParamBlock>, f32) {
    (if f.kind == FxKind::Plugin { None } else { Some(param_block(f)) }, f.mix.clamp(0.0, 1.0))
}

pub(crate) fn c_watch(w: &[Id]) -> Vec<Id> {
    w.to_vec()
}

/// Brings the audio thread's synth instances in line with the project's synth channels:
/// new channels get an instance (built here, off the audio thread), changed ones their new
/// patch, removed ones are dropped.
fn sync_synths(c: &mut Core, sr: f32) {
    let want: Vec<(Id, crate::poly::Patch)> = c
        .project
        .channels
        .iter()
        .filter_map(|ch| match &ch.src {
            ChannelSrc::Synth { synth } => Some((ch.id, crate::poly::params::resolve(synth))),
            _ => None,
        })
        .collect();
    let gone: Vec<Id> = c.synths.keys().filter(|id| !want.iter().any(|(w, _)| w == *id)).copied().collect();
    for id in gone {
        c.synths.remove(&id);
        send(c, Cmd::SynthRemove(id));
    }
    for (id, patch) in want {
        match c.synths.get(&id) {
            None => {
                c.synths.insert(id, patch);
                send(c, Cmd::SynthAdd(Box::new(crate::poly::SynthInst::new(id, patch, sr))));
            }
            Some(old) if *old != patch => {
                c.synths.insert(id, patch);
                send(c, Cmd::SynthPatch { chan: id, patch: Box::new(patch) });
            }
            _ => {}
        }
    }
}

/// Brings the audio thread's plugin instruments in line with the project's plugin channels:
/// a new channel (or one whose plugin changed) gets an instance made here, off the audio
/// thread, from its saved state; a removed one is released. A plugin that does not load
/// leaves its channel silent with an `fx_errors` entry under the channel id; the channel and
/// its saved state stay in the project.
fn sync_plugs(c: &mut Core, sr: f32, max_block: usize, factory: &dyn PluginFactory) {
    let gone: Vec<Id> = c
        .plugs
        .iter()
        .filter(|(id, had)| !c.project.channel(**id).and_then(Channel::plugin).is_some_and(|p| p.same_class(had)))
        .map(|(id, _)| *id)
        .collect();
    for id in gone {
        c.plugs.remove(&id);
        c.fx_errors.remove(&id);
        factory.release(id);
        send(c, Cmd::PlugRemove(id));
    }
    let want: Vec<(Id, PluginRef)> = c
        .project
        .channels
        .iter()
        .filter_map(|ch| ch.plugin().filter(|_| !c.plugs.contains_key(&ch.id)).map(|p| (ch.id, p.clone())))
        .collect();
    for (id, plugin) in want {
        match factory.create_instrument(id, &plugin, sr, max_block, true) {
            Ok(inst) => {
                c.fx_errors.remove(&id);
                send(c, Cmd::PlugAdd(Box::new(crate::plug::PlugInst::new(id, inst, max_block))));
            }
            Err(e) => {
                log::warn!("plugin instrument '{}' did not load: {e}", plugin.name);
                c.fx_errors.insert(id, e);
            }
        }
        c.plugs.insert(id, plugin);
    }
}

fn audible_tracks(p: &Project) -> Vec<(f32, bool)> {
    let any_solo = p.tracks.iter().any(|t| t.solo);
    p.tracks.iter().map(|t| (fader_gain(t.vol), !t.muted && (!any_solo || t.solo))).collect()
}

fn audible_inserts(p: &Project) -> Vec<(Id, f32, f32, f32, bool)> {
    // solo follows the routing: a soloed bus keeps what feeds it, a soloed source its way out
    let heard = crate::routing::audible(p);
    p.inserts
        .iter()
        .zip(heard)
        .map(|(m, heard)| {
            (m.id, fader_gain(m.vol), m.pan.clamp(-1.0, 1.0), if m.width.is_finite() { m.width.clamp(0.0, 2.0) } else { 1.0 }, heard)
        })
        .collect()
}

/// Frames between the timeline and the speakers inside the mixer: the slowest path through the
/// inserts to the master (plugin delay compensation lines every path up with it, see
/// `crate::routing::latency`) plus the master chain.
/// Built-in effects only: hosted plugins count as 0 here (see [`Engine::playback_latency`]).
pub fn playback_latency(p: &Project, sr: f32) -> usize {
    playback_latency_with(p, sr, &HashMap::new())
}

/// [`playback_latency`] with the latency each hosted plugin reports (`hosted`, by effect id).
pub(crate) fn playback_latency_with(p: &Project, sr: f32, hosted: &HashMap<Id, usize>) -> usize {
    let lat: Vec<usize> = p.inserts.iter().map(|m| m.fx.iter().map(|f| fx_latency(f, sr, hosted)).sum()).collect();
    let routes = crate::routing::latency(p, &crate::routing::order(p), &lat);
    lat.first().copied().unwrap_or(0) + routes.at_master
}

/// The delay one effect adds: `catalog::latency` for built-in effects, the reported value
/// for hosted plugins (0 when bypassed or not loaded).
fn fx_latency(f: &crate::model::Fx, sr: f32, hosted: &HashMap<Id, usize>) -> usize {
    match f.kind {
        FxKind::Plugin if f.bypass => 0,
        FxKind::Plugin => hosted.get(&f.id).copied().unwrap_or(0),
        _ => crate::catalog::latency(f, sr),
    }
}

fn monitor_track(p: &Project) -> Option<usize> {
    p.tracks.iter().position(|t| t.arm)
}

/// The track the live input is monitored through: the armed one, else the one being recorded
/// on. `None`: the input is heard dry on the master.
pub(crate) fn monitor_of(c: &Core) -> Option<usize> {
    monitor_track(&c.project).or_else(|| c.take.as_ref().map(|t| t.track))
}

pub(crate) fn mix_state(p: &Project) -> MixState {
    let sends = p
        .inserts
        .iter()
        .flat_map(|m| {
            m.sends.iter().map(move |s| (m.id, s.to, if s.gain.is_finite() { s.gain.clamp(0.0, crate::routing::SEND_MAX) } else { 1.0 }))
        })
        .collect();
    MixState { inserts: audible_inserts(p), sends, tracks: audible_tracks(p), monitor_track: monitor_track(p), chans: audible_channels(p) }
}

/// (gain, pan, audible) of every channel, in project order. Solo works like the mixer's:
/// with any channel soloed, only soloed channels play.
fn audible_channels(p: &Project) -> Vec<(f32, f32, bool)> {
    let any_solo = p.channels.iter().any(|c| c.solo);
    p.channels.iter().map(|c| (c.gain(), c.pan.clamp(-1.0, 1.0), !c.mute && (!any_solo || c.solo))).collect()
}

/// The channel table and the patterns for the audio thread. Swing is applied here (when
/// the timeline is built, i.e. at schedule time, never written into the notes); notes of
/// unknown channels and notes past the pattern end are left out.
fn build_patterns(p: &Project, audio: &HashMap<Id, Arc<Audio>>, keys: Option<&Arc<crate::keys::KeysBank>>) -> (Vec<ChanRt>, Vec<PatRt>) {
    let ins_idx = |id: Id| p.inserts.iter().position(|m| m.id == id);
    let track_ins = |track: u32| p.tracks.get(track as usize).and_then(|t| ins_idx(t.insert));
    // first clip (by time) of each pattern → its track's insert
    let mut first: HashMap<Id, (f64, u32)> = HashMap::new();
    for c in p.clips.iter().filter(|c| c.kind == ClipKind::Pat) {
        let e = first.entry(c.source).or_insert((c.start, c.track));
        if c.start < e.0 {
            *e = (c.start, c.track);
        }
    }
    let pat_home = |pid: Id| first.get(&pid).and_then(|&(_, t)| track_ins(t));
    let gains = audible_channels(p);
    let chans: Vec<ChanRt> = p
        .channels
        .iter()
        .zip(gains)
        .map(|(c, (gain, pan, on))| {
            let insert = c.insert.and_then(ins_idx);
            let heard = p.patterns.iter().filter(|pt| pt.notes.iter().any(|n| n.ch == c.id)).find_map(|pt| pat_home(pt.id));
            let (kind, sample) = match c.src {
                ChannelSrc::Sample { source, .. } => (crate::synth::VoiceKind::Sample, audio.get(&source).cloned()),
                ChannelSrc::Synth { .. } => (crate::synth::VoiceKind::Synth, None),
                ChannelSrc::Keys { .. } => (crate::synth::VoiceKind::Keys, None),
                ChannelSrc::Plugin { .. } => (crate::synth::VoiceKind::Plugin, None),
            };
            let ks = match c.src {
                ChannelSrc::Keys { keys: k } => Some(k),
                _ => None,
            };
            let (keys_release, keys_dynamics) = ks.map(|k| (k.release / 1000.0, k.dynamics / 100.0)).unwrap_or((0.35, 1.0));
            let (smp, slices) = match &c.src {
                ChannelSrc::Sample { source, sampler, slices } => (
                    SmpRt::new(sampler, p.sources.iter().find(|s| s.id == *source).map(|s| s.peak_db)),
                    if slices.is_empty() { None } else { Some(Arc::<[f32]>::from(slices.as_slice())) },
                ),
                _ => (SmpRt::default(), None),
            };
            ChanRt {
                id: c.id,
                kind,
                sample,
                smp,
                slices,
                keys: ks.and(keys.cloned()),
                keys_release,
                keys_dynamics,
                root: c.root,
                gain,
                pan,
                gain0: gain,
                pan0: pan,
                on,
                insert,
                heard,
                home: insert.or(heard).unwrap_or(0),
            }
        })
        .collect();
    let pats = p
        .patterns
        .iter()
        .map(|pt| {
            let end = (pt.len * 4.0) as f32;
            let notes = pt
                .notes
                .iter()
                .filter(|n| n.step < end - 1e-4)
                .filter_map(|n| {
                    let ci = p.channels.iter().position(|c| c.id == n.ch)?;
                    let step = n.step + swing_offset(n.step, pt.swing);
                    Some(NoteRt {
                        beat: step as f64 / 4.0,
                        len: n.len as f64 / 4.0,
                        sampler_trigger: if chans[ci].kind == crate::synth::VoiceKind::Sample { n.trig } else { None },
                        chan: ci,
                        key: n.p.unwrap_or(p.channels[ci].root),
                        vel: n.v.clamp(0.0, 1.0),
                        pan: n.pan.clamp(-1.0, 1.0),
                    })
                })
                .collect();
            PatRt { id: pt.id, len: pt.len, notes, home: pat_home(pt.id) }
        })
        .collect();
    (chans, pats)
}

/// Builds the mixer graph. Effects in `live` are left as `proc: None` so the audio
/// thread keeps the running instance; everything else is created and prepared here.
///
/// `hosted`: the latency of every hosted plugin by effect id. Read for the instances in
/// `live` (they keep running, and so keep the latency they had), written for the ones
/// created here (what the new processor reports), pruned of effects that are gone. Plugin
/// delay compensation uses it the way it uses `catalog::latency` for built-in effects.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_graph(
    p: &Project,
    live: &HashMap<Id, FxKind>,
    sr: f32,
    max_block: usize,
    factory: &dyn PluginFactory,
    live_instances: bool,
    errs: &mut HashMap<Id, String>,
    hosted: &mut HashMap<Id, usize>,
) -> Graph {
    let ins_state = audible_inserts(p);
    // processors first: a hosted plugin's latency is only known once it exists
    let procs: Vec<Vec<Option<Box<dyn Processor>>>> = p
        .inserts
        .iter()
        .map(|m| {
            m.fx.iter()
                .map(|f| {
                    // live by id *and* kind: the audio thread carries the running processor
                    // over only for the same kind (see `Rt::apply`), so a kind change must
                    // build fresh here, or the slot would sit without a processor
                    if live.get(&f.id) == Some(&f.kind) {
                        None
                    } else if f.kind == FxKind::Plugin {
                        match factory.create(f.id, f.plugin.as_ref().unwrap(), sr, max_block, live_instances) {
                            Ok(p) => {
                                errs.remove(&f.id);
                                hosted.insert(f.id, p.latency_samples());
                                Some(p)
                            }
                            Err(e) => {
                                errs.insert(f.id, e);
                                hosted.remove(&f.id);
                                None
                            }
                        }
                    } else {
                        let mut pr = crate::dsp::create(f.kind).unwrap();
                        pr.prepare(sr, max_block);
                        pr.set_params(&param_block(f));
                        if let Some(m) = &f.maps {
                            let _ = pr.swap_data(Box::new(m.clone()));
                        }
                        Some(pr)
                    }
                })
                .collect()
        })
        .collect();
    hosted.retain(|id, _| p.find_fx(*id).is_some_and(|(_, f)| f.kind == FxKind::Plugin));
    let lat: Vec<usize> = p.inserts.iter().map(|m| m.fx.iter().map(|f| fx_latency(f, sr, hosted)).sum()).collect();
    let order = crate::routing::order(p);
    let routes = crate::routing::latency(p, &order, &lat);
    let fed: Vec<bool> = (0..p.inserts.len()).map(|k| order.sends.iter().any(|v| v.iter().any(|s| s.0 == k))).collect();
    let inserts = p
        .inserts
        .iter()
        .zip(ins_state)
        .zip(procs)
        .map(|((m, (id, gain, pan, width, on)), procs)| {
            let fx =
                m.fx.iter()
                    .zip(procs)
                    .map(|(f, proc)| {
                        let params = if f.kind == FxKind::Plugin { None } else { Some(param_block(f)) };
                        FxSlot::new(f.id, f.kind, f.bypass, f.mix.clamp(0.0, 1.0), proc, params, fx_latency(f, sr, hosted))
                    })
                    .collect();
            let k = p.inserts.iter().position(|x| x.id == id).unwrap_or(0);
            let sends = order.sends[k].iter().zip(&routes.sends[k]).map(|(&(t, g), &d)| SendRt::new(t, p.inserts[t].id, g, d)).collect();
            InsertRt::new(id, gain, pan, width, on, fx, max_block).with_delay(routes.master[k]).with_routing(
                order.level[k],
                k == 0 || m.master,
                sends,
                fed[k],
                routes.input[k],
                max_block,
            )
        })
        .collect();
    let track_state = audible_tracks(p);
    let tracks = p
        .tracks
        .iter()
        .zip(track_state)
        .map(|(t, (gain, on))| TrackRt { insert: p.inserts.iter().position(|m| m.id == t.insert).unwrap_or(0), gain, on })
        .collect();
    Graph::new(inserts, tracks, monitor_track(p), max_block).with_latency(routes.at_master, lat.first().copied().unwrap_or(0))
}

/// `takes`: loaded audio by source id of every comp take and sampler channel.
/// What an audio clip plays and the rate it reads it at (`SourceStore::clip_play`).
type ClipAudioFn<'a> = dyn FnMut(&Clip) -> Option<(Arc<Audio>, f64)> + 'a;

pub(crate) fn build_timeline(
    p: &Project,
    audio: &mut ClipAudioFn<'_>,
    takes: &HashMap<Id, Arc<Audio>>,
    audition: &HashMap<Id, Id>,
    keys: Option<&Arc<crate::keys::KeysBank>>,
) -> Timeline {
    // comps are resolved once each, shared by every clip that plays them
    let spb = 60.0 / p.bpm;
    let comps: HashMap<Id, Arc<crate::comp::CompRt>> =
        p.comps.iter().map(|cp| (cp.id, Arc::new(crate::comp::build(cp, spb, audition.get(&cp.id).copied(), takes)))).collect();
    let (chans, pats) = build_patterns(p, takes, keys);
    // automation envelopes whose target still exists
    let mut autos = vec![];
    let mut auto_idx = HashMap::new();
    for a in &p.automations {
        if let Some(target) = resolve_target(p, &a.target) {
            auto_idx.insert(a.id, autos.len());
            autos.push(AutoRt { target, env: a.clone() });
        }
    }
    let mut clips = vec![];
    for c in p.clips.iter().filter(|c| !c.muted) {
        if c.track as usize >= p.tracks.len() {
            continue;
        }
        let mut rate = 1.0;
        let src = match c.kind {
            ClipKind::Pat => match p.patterns.iter().position(|x| x.id == c.source) {
                Some(i) => ClipSrc::Pat(i),
                None => continue,
            },
            ClipKind::Auto => match auto_idx.get(&c.source) {
                Some(&i) => ClipSrc::Auto(i),
                None => continue,
            },
            ClipKind::Audio => match audio(c) {
                Some((a, r)) => {
                    rate = r;
                    ClipSrc::Audio(a)
                }
                None => continue,
            },
            ClipKind::Comp => match comps.get(&c.source) {
                Some(cp) => ClipSrc::Comp(cp.clone()),
                None => continue,
            },
        };
        clips.push(ClipRt {
            id: c.id,
            track: c.track as usize,
            start: c.start,
            end: c.start + c.len,
            offset: c.offset,
            rate,
            gain: c.gain,
            pan: c.pan,
            fade_in: c.fade_in,
            fade_out: c.fade_out,
            insert: if c.kind == ClipKind::Audio { c.insert.and_then(|id| p.inserts.iter().position(|m| m.id == id)) } else { None },
            src,
            gain0: c.gain,
            pan0: c.pan,
        });
    }
    clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    // clip targets point at the sorted list (a muted clip is not in it: its automation rests)
    for a in autos.iter_mut() {
        if let TargetRt::ClipGain(k) | TargetRt::ClipPan(k) = &mut a.target {
            let id = *k as Id;
            *k = clips.iter().position(|c| c.id == id && matches!(c.src, ClipSrc::Audio(_))).unwrap_or(usize::MAX);
        }
    }
    // the clips of each automated target, by start (automations of the same target share one)
    let mut drives: Vec<Drive> = vec![];
    for (ci, c) in clips.iter().enumerate() {
        let ClipSrc::Auto(ai) = c.src else { continue };
        let target = autos[ai].target;
        match drives.iter_mut().find(|d| d.target == target) {
            Some(d) => d.clips.push(ci),
            None => drives.push(Drive { target, clips: vec![ci] }),
        }
    }
    Timeline { bpm: p.bpm, clips, pats, chans, autos, drives, loop_range: p.loop_range.map(|l| (l.a, l.b)), song_end: p.song_end() }
}

/// Turns an automation target into graph indices; `None` if it no longer exists.
fn resolve_target(p: &Project, t: &AutoTarget) -> Option<TargetRt> {
    Some(match t {
        AutoTarget::Fx { fx_id, param } => {
            let (_, f) = p.find_fx(*fx_id)?;
            if let Some(rest) = param.strip_prefix("band:") {
                let mut it = rest.split(':');
                let band: usize = it.next()?.parse().ok()?;
                let field = match it.next()? {
                    "f" => 0,
                    "g" => 1,
                    "q" => 2,
                    _ => return None,
                };
                if band >= MAX_BANDS {
                    return None;
                }
                TargetRt::Band { fx: *fx_id, band, field }
            } else {
                let defs = crate::catalog::params(f.kind);
                let idx = defs.iter().position(|d| d.id == param)?;
                let d = &defs[idx];
                TargetRt::Param { fx: *fx_id, idx, min: d.min as f32, max: d.max as f32, log: d.log }
            }
        }
        AutoTarget::Mix { fx_id } => {
            p.find_fx(*fx_id)?;
            TargetRt::Mix { fx: *fx_id }
        }
        AutoTarget::InsertVol { id } => TargetRt::InsVol(p.inserts.iter().position(|m| m.id == *id)?),
        AutoTarget::InsertPan { id } => TargetRt::InsPan(p.inserts.iter().position(|m| m.id == *id)?),
        AutoTarget::TrackVol { id } => TargetRt::TrackVol(p.tracks.iter().position(|x| x.id == *id)?),
        AutoTarget::ChanVol { id } => TargetRt::ChanVol(p.channels.iter().position(|c| c.id == *id)?),
        AutoTarget::ChanPan { id } => TargetRt::ChanPan(p.channels.iter().position(|c| c.id == *id)?),
        AutoTarget::Synth { chan_id, param } => {
            let c = p.channels.iter().find(|c| c.id == *chan_id)?;
            if !matches!(c.src, ChannelSrc::Synth { .. }) {
                return None;
            }
            let idx = crate::poly::params::index_of(param)?;
            let d = &crate::poly::params::PARAMS[idx];
            TargetRt::Synth { chan: *chan_id, idx, min: d.min as f32, max: d.max as f32, log: d.log, choice: d.options.is_some() }
        }
        AutoTarget::Keys { chan_id, param } => {
            let i = p.channels.iter().position(|c| c.id == *chan_id && matches!(c.src, ChannelSrc::Keys { .. }))?;
            match param.as_str() {
                "release" => TargetRt::KeysRelease(i),
                "dynamics" => TargetRt::KeysDynamics(i),
                _ => return None,
            }
        }
        AutoTarget::Plugin { fx_id, param_id } => {
            p.find_fx(*fx_id).filter(|(_, f)| f.kind == FxKind::Plugin)?;
            TargetRt::PluginParam { fx: *fx_id, param_id: *param_id }
        }
        // the clip id for now: `build_timeline` turns it into the clip's index once sorted
        AutoTarget::ClipGain { id } => TargetRt::ClipGain(p.clips.iter().find(|c| c.id == *id && c.kind == ClipKind::Audio)?.id as usize),
        AutoTarget::ClipPan { id } => TargetRt::ClipPan(p.clips.iter().find(|c| c.id == *id && c.kind == ClipKind::Audio)?.id as usize),
    })
}
