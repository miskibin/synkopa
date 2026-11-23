//! Builders and the base projects the fixture cases run on. Built with the real model
//! constructors and handed to serde, so the fixture's JSON is exactly what the engine
//! writes — the UI replays it without any hand-written JSON in between.

use std::collections::BTreeMap;

use wave_engine::model::*;

pub fn fx(id: Id, kind: FxKind) -> Fx {
    Fx { id, kind, bypass: false, mix: 1.0, preset: None, params: BTreeMap::new(), bands: None, plugin: None, maps: None }
}

pub fn eq_band(f: f64, g: f64) -> EqBand {
    EqBand { on: true, kind: BandType::Peak, f, g, q: 1.0, slope: 12, place: Placement::Stereo, dynamic: None }
}

pub fn delay_fx(id: Id, params: &[(&str, f64)]) -> Fx {
    let mut f = fx(id, FxKind::Delay);
    f.params = params.iter().map(|(k, v)| ((*k).into(), *v)).collect();
    f
}

pub fn plugin_fx(id: Id, path: &str) -> Fx {
    let mut f = fx(id, FxKind::Plugin);
    f.plugin = Some(PluginRef {
        format: PluginRef::VST3.into(),
        path: path.into(),
        cid: "2ebf41a7aae34b6f9f39e2ff2d6b0c41".into(),
        name: "Serum".into(),
        vendor: "Xfer Records".into(),
        state: None,
    });
    f
}

pub fn marker(id: Id, at: f64, name: &str) -> Marker {
    Marker { id, at, name: name.into() }
}

pub fn eq_fx(id: Id, bands: Vec<EqBand>) -> Fx {
    let mut f = fx(id, FxKind::Eq);
    f.bands = Some(bands);
    f
}

pub fn warp_maps() -> WarpMaps {
    WarpMaps::default()
}

pub fn pat(id: Id, name: &str, len: f64, min_len: f64, notes: Vec<Note>) -> Pattern {
    Pattern { id, name: name.into(), color: "#4f8ef7".into(), len, min_len, notes, swing: 0.0 }
}

#[allow(clippy::too_many_arguments)]
pub fn clip(id: Id, track: u32, start: f64, len: f64, kind: ClipKind, source: Id) -> Clip {
    Clip {
        id,
        track,
        start,
        len,
        offset: 0.0,
        kind,
        source,
        muted: false,
        gain: 0.0,
        fade_in: 0.0,
        fade_out: 0.0,
        pan: 0.0,
        norm: false,
        rev: false,
        tune: None,
        stretch: 1.0,
        resample: false,
        pitch: 0.0,
        insert: None,
        root: SAMPLE_ROOT,
    }
}

pub fn source(id: Id, name: &str) -> Source {
    Source {
        id,
        name: name.into(),
        color: "#d9678f".into(),
        path: format!("C:\\Music\\{name}.wav"),
        dur: 2.5,
        sr: 44100,
        ch: 1,
        peak_db: -3.0,
    }
}

pub fn page(id: Id, name: &str, start_bar: u32, text: &str) -> LyricPage {
    LyricPage { id, name: name.into(), start_bar, text: text.into() }
}

/// The rich project every case starts from unless it says otherwise: two routed tracks,
/// master + two inserts (with a delay/EQ/plugin slot), a two-bar pattern with notes on a
/// sampler and a keys channel, an audio and a comp and an automation clip, a take group,
/// a loop, key A minor, a notebook page and two markers.
pub fn base() -> Project {
    Project {
        version: 2,
        bpm: 120.0,
        tracks: vec![track(1, "Drums", 1), track(2, "Vocal", 2)],
        inserts: vec![
            Insert {
                id: 0,
                name: "Master".into(),
                color: "#d5dae2".into(),
                vol: VOL_UNITY,
                pan: 0.0,
                width: 1.0,
                muted: false,
                solo: false,
                fx: vec![fx(900, FxKind::Limit)],
                sends: vec![],
                master: true,
            },
            Insert {
                id: 1,
                name: "Drums".into(),
                color: "#4f8ef7".into(),
                vol: VOL_UNITY,
                pan: 0.0,
                width: 1.0,
                muted: false,
                solo: false,
                fx: vec![delay_fx(902, &[("fb", 0.3)]), eq_fx(901, vec![eq_band(100.0, 3.0), eq_band(8000.0, -2.0)])],
                sends: vec![],
                master: true,
            },
            Insert {
                id: 2,
                name: "Vocal".into(),
                color: "#d9678f".into(),
                vol: VOL_UNITY,
                pan: 0.0,
                width: 1.0,
                muted: false,
                solo: false,
                fx: vec![plugin_fx(903, "C:\\Program Files\\Common Files\\VST3\\Serum.vst3")],
                sends: vec![],
                master: true,
            },
        ],
        patterns: vec![
            pat(
                20,
                "Verse",
                8.0,
                4.0,
                vec![
                    Note::new(30, 0.0, 1.0, None),
                    Note { ch: 30, step: 4.0, len: 1.0, p: None, v: 0.8, trig: Some(true), pan: 0.0, slide: false },
                    Note::new(31, 4.0, 4.0, Some(57)),
                    Note::new(31, 8.0, 4.0, Some(60)),
                    Note::new(31, 12.0, 4.0, Some(62)),
                    Note::new(30, 28.0, 4.0, None),
                ],
            ),
            pat(21, "Hook", 8.0, 8.0, vec![]),
        ],
        sources: vec![source(10, "kick"), source(11, "take-a")],
        clips: vec![
            clip(40, 0, 0.0, 8.0, ClipKind::Pat, 20),
            Clip { insert: Some(2), gain: -3.0, fade_in: 0.1, ..clip(41, 1, 2.0, 4.0, ClipKind::Audio, 10) },
            clip(42, 1, 8.0, 4.0, ClipKind::Comp, 60),
            clip(43, 0, 16.0, 4.0, ClipKind::Auto, 50),
        ],
        loop_range: Some(LoopRange { a: 0.0, b: 8.0 }),
        automations: vec![Automation {
            id: 50,
            name: "Drums vol".into(),
            color: "#4f8ef7".into(),
            target: AutoTarget::InsertVol { id: 1 },
            points: vec![
                AutoPoint { t: 0.0, v: 1.0, tension: 0.0, shape: CurveShape::Curve },
                AutoPoint { t: 4.0, v: 0.5, tension: 0.0, shape: CurveShape::Curve },
            ],
        }],
        comps: vec![Comp {
            id: 60,
            name: "Vocal Comp".into(),
            takes: vec![take(61, "take-a", 10, 0.0, 2.0), take(62, "take-b", 11, 2.0, 2.0)],
            regions: vec![
                CompRegion { take: 61, from: 0.0, to: 2.0, xfade: None },
                CompRegion { take: 62, from: 2.0, to: 4.0, xfade: Some(0.05) },
            ],
            xfade: 0.010,
        }],
        channels: vec![
            Channel::sampler(30, "Kick", "#d9678f", 10),
            Channel::new(31, "Keys", "#7c8cf8", ChannelSrc::Keys { keys: KeysState::default() }),
        ],
        notebook: Notebook { pages: vec![page(70, "Verse", 1, "[Verse]\nfirst line\n\nsecond line")] },
        markers: vec![marker(80, 0.0, "Intro"), marker(81, 16.0, "Hook")],
        key: Some(9),
        scale: 2,
        next_id: 100,
    }
}

/// One track only: for the rejections that need a project at its last lane.
pub fn one_track() -> Project {
    let mut p = base();
    p.tracks.truncate(1);
    p.clips.retain(|c| c.track == 0);
    p
}

pub fn track(id: Id, name: &str, insert: Id) -> Track {
    Track {
        id,
        name: name.into(),
        color: "#4f8ef7".into(),
        muted: false,
        solo: false,
        arm: false,
        vol: VOL_UNITY,
        h: 56.0,
        insert,
        icon: None,
    }
}

pub fn take(id: Id, name: &str, source: Id, start: f64, len: f64) -> Take {
    Take { id, name: name.into(), source, start, len, offset: 0.0, color: None }
}
