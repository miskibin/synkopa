//! The case table: `(base, name, ops)` triples the runner replays through `ops::apply_all`.
//! Happy rows pin the resulting project; reject rows pin the engine's error string, which
//! the UI port must match exactly (its messages are user-visible toasts).

use serde_json::json;

use super::common::*;
use wave_engine::model::*;
use wave_engine::ops::TransformSelect;
use wave_engine::transform::TransformVerb;
use wave_engine::{tune_edit::TuneEdit, Id, Op};

pub type Case = (&'static str, &'static str, Vec<Op>);

fn tune_edit(v: serde_json::Value) -> TuneEdit {
    serde_json::from_value(v).unwrap()
}

pub fn cases() -> Vec<Case> {
    let mut c: Vec<Case> = vec![];
    let edit = |v: serde_json::Value| tune_edit(v);

    // ---- clips
    let good_audio = |id: Id| clip(id, 1, 20.0, 4.0, ClipKind::Audio, 10);
    c.push((
        "main",
        "putClips adds a clip and replaces one",
        vec![Op::PutClips {
            clips: vec![
                clip(46, 0, 8.0, 8.0, ClipKind::Pat, 21),
                Clip { len: 6.0, gain: -6.0, ..clip(41, 1, 2.0, 4.0, ClipKind::Audio, 10) },
            ],
        }],
    ));
    c.push((
        "main",
        "putClips keeps non-default audio fields and drops defaults",
        vec![Op::PutClips {
            clips: vec![Clip { stretch: 2.0, pitch: -3.5, fade_in: 0.2, insert: Some(2), ..good_audio(44) }, good_audio(45)],
        }],
    ));
    c.push(("main", "putClips refuses an invalid range", vec![Op::PutClips { clips: vec![Clip { len: 0.0, ..good_audio(44) }] }]));
    c.push(("main", "putClips refuses a missing track", vec![Op::PutClips { clips: vec![clip(44, 9, 0.0, 4.0, ClipKind::Audio, 10)] }]));
    c.push((
        "main",
        "putClips refuses a stretch out of range",
        vec![Op::PutClips { clips: vec![Clip { stretch: 5.0, ..good_audio(44) }] }],
    ));
    c.push((
        "main",
        "putClips refuses to stretch a pattern clip",
        vec![Op::PutClips { clips: vec![Clip { stretch: 2.0, ..clip(46, 0, 0.0, 8.0, ClipKind::Pat, 20) }] }],
    ));
    c.push((
        "main",
        "putClips refuses to resample a pattern clip",
        vec![Op::PutClips { clips: vec![Clip { resample: true, ..clip(46, 0, 0.0, 8.0, ClipKind::Pat, 20) }] }],
    ));
    c.push(("main", "putClips refuses a pitch out of range", vec![Op::PutClips { clips: vec![Clip { pitch: 13.0, ..good_audio(44) }] }]));
    c.push((
        "main",
        "putClips refuses to transpose a pattern clip",
        vec![Op::PutClips { clips: vec![Clip { pitch: 3.0, ..clip(46, 0, 0.0, 8.0, ClipKind::Pat, 20) }] }],
    ));
    c.push((
        "main",
        "putClips refuses an unknown mixer insert",
        vec![Op::PutClips { clips: vec![Clip { insert: Some(9), ..good_audio(44) }] }],
    ));
    c.push(("main", "putClips refuses an unknown pattern", vec![Op::PutClips { clips: vec![clip(46, 0, 0.0, 8.0, ClipKind::Pat, 99)] }]));
    c.push((
        "main",
        "putClips refuses an unknown automation",
        vec![Op::PutClips { clips: vec![clip(44, 0, 0.0, 4.0, ClipKind::Auto, 99)] }],
    ));
    let tune_ref = |melody: Option<Melody>| ClipTune {
        notes: vec![],
        pencil: vec![],
        reference: Some(RefTune {
            source: 10,
            correction: 0.5,
            vibrato: 0.5,
            influence: 0.5,
            octave: None,
            key: 0,
            scale: 1,
            speed: None,
            melody,
        }),
    };
    c.push((
        "main",
        "putClips refuses an unknown reference source",
        vec![Op::PutClips {
            clips: vec![Clip {
                tune: Some(ClipTune {
                    notes: vec![],
                    pencil: vec![],
                    reference: Some(RefTune {
                        source: 99,
                        correction: 0.5,
                        vibrato: 0.5,
                        influence: 0.5,
                        octave: None,
                        key: 0,
                        scale: 1,
                        speed: None,
                        melody: None,
                    }),
                }),
                ..good_audio(44)
            }],
        }],
    ));
    c.push((
        "main",
        "putClips refuses melody notes out of range",
        vec![Op::PutClips {
            clips: vec![Clip {
                tune: Some(tune_ref(Some(Melody { channel: 31, span: None, notes: vec![MelodyNote { start: 0.0, end: 1.0, key: 200 }] }))),
                ..good_audio(44)
            }],
        }],
    ));
    let tune_ref_field = |speed: Option<f32>, correction: f32, octave: Option<i32>| RefTune {
        source: 10,
        correction,
        vibrato: 0.5,
        influence: 0.5,
        octave,
        key: 0,
        scale: 1,
        speed,
        melody: Some(Melody { channel: 31, span: None, notes: vec![MelodyNote { start: 0.0, end: 1.0, key: 60 }] }),
    };
    let with_ref = |r: RefTune| ClipTune { notes: vec![], pencil: vec![], reference: Some(r) };
    c.push((
        "main",
        "putClips refuses a reference speed out of range",
        vec![Op::PutClips { clips: vec![Clip { tune: Some(with_ref(tune_ref_field(Some(2000.0), 0.5, None))), ..good_audio(44) }] }],
    ));
    c.push((
        "main",
        "putClips refuses a reference amount out of range",
        vec![Op::PutClips { clips: vec![Clip { tune: Some(with_ref(tune_ref_field(None, 1.5, None))), ..good_audio(44) }] }],
    ));
    c.push((
        "main",
        "putClips refuses a reference octave off the octave grid",
        vec![Op::PutClips { clips: vec![Clip { tune: Some(with_ref(tune_ref_field(None, 0.5, Some(7)))), ..good_audio(44) }] }],
    ));
    c.push(("main", "removeClips drops the ones asked for", vec![Op::RemoveClips { ids: vec![41, 40] }]));

    // ---- tracks and inserts
    c.push((
        "main",
        "putTrack appends and replaces",
        vec![
            Op::PutTrack { track: track(3, "Bass", 2) },
            Op::PutTrack { track: Track { name: "Voice".into(), insert: 1, ..track(2, "Vocal", 2) } },
        ],
    ));
    c.push(("main", "putTrack refuses an unknown insert", vec![Op::PutTrack { track: track(3, "Bass", 9) }]));
    c.push(("main", "removeTrack drops the lane and reindexes the clips", vec![Op::RemoveTrack { id: 1 }]));
    c.push(("one", "removeTrack refuses the last track", vec![Op::RemoveTrack { id: 1 }]));
    c.push(("one", "removeTrack refuses an unknown track", vec![Op::RemoveTrack { id: 99 }]));
    c.push((
        "main",
        "putInsert replaces and keeps its effects; a new one drops plugin slots",
        vec![
            Op::PutInsert {
                insert: Insert {
                    name: "Bus".into(),
                    vol: 0.5,
                    ..Insert {
                        id: 2,
                        name: "Vocal".into(),
                        color: "#d9678f".into(),
                        vol: VOL_UNITY,
                        pan: 0.0,
                        width: 1.0,
                        muted: false,
                        solo: false,
                        fx: vec![],
                        sends: vec![],
                        master: true,
                    }
                },
            },
            Op::PutInsert {
                insert: Insert {
                    id: 3,
                    name: "Extra".into(),
                    color: "#5fbf8a".into(),
                    vol: VOL_UNITY,
                    pan: 0.0,
                    width: 1.0,
                    muted: false,
                    solo: false,
                    fx: vec![delay_fx(950, &[("fb", 0.4)]), plugin_fx(951, "C:\\VST\\Serum.vst3")],
                    sends: vec![],
                    master: true,
                },
            },
        ],
    ));
    c.push(("main", "removeInsert re-routes tracks and clips to where they came from", vec![Op::RemoveInsert { id: 2 }]));
    c.push(("main", "removeInsert refuses the master", vec![Op::RemoveInsert { id: 0 }]));
    c.push(("main", "removeInsert refuses an unknown insert", vec![Op::RemoveInsert { id: 9 }]));

    // ---- effects
    c.push((
        "main",
        "putFx adds at an index and replaces in place",
        vec![
            Op::PutFx { insert_id: 2, fx: delay_fx(904, &[("fb", 0.2)]), index: Some(0) },
            Op::PutFx { insert_id: 1, fx: eq_fx(901, vec![eq_band(150.0, 6.0)]), index: None },
        ],
    ));
    c.push((
        "main",
        "putFx refuses an EQ with too many bands",
        vec![Op::PutFx { insert_id: 1, fx: eq_fx(904, vec![eq_band(100.0, 0.0); 25]), index: None }],
    ));
    c.push(("main", "putFx refuses an EQ without bands", vec![Op::PutFx { insert_id: 1, fx: fx(904, FxKind::Eq), index: None }]));
    c.push((
        "main",
        "putFx refuses a plugin slot without a plugin",
        vec![Op::PutFx { insert_id: 2, fx: fx(904, FxKind::Plugin), index: None }],
    ));
    c.push(("main", "putFx refuses a Warp without maps", vec![Op::PutFx { insert_id: 2, fx: fx(904, FxKind::Warp), index: None }]));
    c.push((
        "main",
        "putFx refuses maps outside a Warp",
        vec![Op::PutFx { insert_id: 2, fx: Fx { maps: Some(warp_maps()), ..delay_fx(904, &[]) }, index: None }],
    ));
    c.push((
        "main",
        "putFx refuses to move an effect to another insert",
        vec![Op::PutFx { insert_id: 2, fx: eq_fx(901, vec![eq_band(100.0, 3.0)]), index: None }],
    ));
    c.push(("main", "putFx refuses to change an effect's type", vec![Op::PutFx { insert_id: 1, fx: delay_fx(901, &[]), index: None }]));
    c.push((
        "main",
        "putFx refuses to swap a slot's plugin",
        vec![Op::PutFx { insert_id: 2, fx: plugin_fx(903, "C:\\Program Files\\Common Files\\VST3\\Vital.vst3"), index: None }],
    ));
    c.push((
        "main",
        "putFx refuses an unknown insert for a new effect",
        vec![Op::PutFx { insert_id: 9, fx: delay_fx(904, &[]), index: None }],
    ));
    c.push(("main", "removeFx drops the slot", vec![Op::RemoveFx { fx_id: 900 }]));
    c.push(("main", "removeFx refuses an unknown effect", vec![Op::RemoveFx { fx_id: 99 }]));
    c.push(("main", "moveFx reorders and clamps", vec![Op::MoveFx { fx_id: 901, to: 0 }, Op::MoveFx { fx_id: 903, to: 9 }]));
    c.push(("main", "moveFx refuses an unknown effect", vec![Op::MoveFx { fx_id: 99, to: 0 }]));

    // ---- patterns and channels
    c.push((
        "main",
        "putPattern writes notes and whole-pass clips follow the length",
        vec![Op::PutPattern {
            pattern: pat(
                20,
                "Verse",
                8.0,
                4.0,
                vec![Note::new(30, 0.0, 1.0, None), Note::new(31, 32.0, 8.0, Some(64)), Note::new(30, 28.0, 4.0, None)],
            ),
        }],
    ));
    c.push((
        "main",
        "putPattern keeps the user's minimum length",
        vec![Op::PutPattern { pattern: pat(21, "Hook", 4.0, 8.0, vec![Note::new(30, 0.0, 4.0, None)]) }],
    ));
    c.push((
        "main",
        "putPattern refuses a negative step",
        vec![Op::PutPattern { pattern: pat(20, "Verse", 8.0, 4.0, vec![Note::new(30, -1.0, 1.0, None)]) }],
    ));
    c.push((
        "main",
        "putPattern refuses a zero length",
        vec![Op::PutPattern { pattern: pat(20, "Verse", 8.0, 4.0, vec![Note::new(30, 0.0, 0.0, None)]) }],
    ));
    c.push((
        "main",
        "putPattern refuses a velocity out of range",
        vec![Op::PutPattern { pattern: pat(20, "Verse", 8.0, 4.0, vec![Note { v: 1.5, ..Note::new(30, 0.0, 1.0, None) }]) }],
    ));
    c.push((
        "main",
        "putPattern refuses a key out of range",
        vec![Op::PutPattern { pattern: pat(20, "Verse", 8.0, 4.0, vec![Note::new(30, 0.0, 1.0, Some(200))]) }],
    ));
    c.push((
        "main",
        "putPattern refuses notes of a missing channel",
        vec![Op::PutPattern { pattern: pat(20, "Verse", 8.0, 4.0, vec![Note::new(99, 0.0, 1.0, None)]) }],
    ));
    c.push((
        "main",
        "putPattern refuses notes past the arrangement's end",
        vec![Op::PutPattern { pattern: pat(20, "Verse", 8.0, 4.0, vec![Note::new(30, 8188.0, 8.0, None)]) }],
    ));
    c.push((
        "main",
        "putChannel appends and moves",
        vec![
            Op::PutChannel {
                channel: Channel::new(33, "Keys 2", "#7c8cf8", ChannelSrc::Keys { keys: KeysState::default() }),
                index: Some(0),
            },
            Op::PutChannel { channel: Channel { vol: 0.5, ..Channel::sampler(30, "Kick", "#d9678f", 10) }, index: Some(2) },
        ],
    ));
    let chan = |mut ch: Channel| -> Channel {
        ch.vol = VOL_UNITY;
        ch
    };
    c.push((
        "main",
        "putChannel refuses a fader position out of range",
        vec![Op::PutChannel { channel: Channel { vol: 1.5, ..chan(Channel::sampler(33, "Kick", "#d9678f", 10)) }, index: None }],
    ));
    c.push((
        "main",
        "putChannel refuses a pan out of range",
        vec![Op::PutChannel { channel: Channel { pan: 1.5, ..chan(Channel::sampler(33, "Kick", "#d9678f", 10)) }, index: None }],
    ));
    c.push((
        "main",
        "putChannel refuses a root out of range",
        vec![Op::PutChannel { channel: Channel { root: 200, ..chan(Channel::sampler(33, "Kick", "#d9678f", 10)) }, index: None }],
    ));
    c.push((
        "main",
        "putChannel refuses an unknown sample",
        vec![Op::PutChannel { channel: chan(Channel::sampler(33, "Kick", "#d9678f", 99)), index: None }],
    ));
    c.push((
        "main",
        "putChannel refuses an unknown insert",
        vec![Op::PutChannel { channel: Channel { insert: Some(9), ..chan(Channel::sampler(33, "Kick", "#d9678f", 10)) }, index: None }],
    ));
    c.push((
        "main",
        "putChannel refuses a broken sampler range",
        vec![Op::PutChannel {
            channel: chan(Channel::new(
                33,
                "Kick",
                "#d9678f",
                ChannelSrc::Sample { source: 10, sampler: SamplerState { end: 0.0, ..SamplerState::default() }, slices: vec![] },
            )),
            index: None,
        }],
    ));
    c.push((
        "main",
        "putChannel refuses an unknown synth parameter",
        vec![Op::PutChannel {
            channel: chan(Channel::new(
                33,
                "Synth",
                "#7c8cf8",
                ChannelSrc::Synth { synth: SynthState { preset: None, params: [("nope".into(), 0.5)].into_iter().collect() } },
            )),
            index: None,
        }],
    ));
    c.push((
        "main",
        "putChannel refuses a keys release out of range",
        vec![Op::PutChannel {
            channel: chan(Channel::new(33, "Keys", "#7c8cf8", ChannelSrc::Keys { keys: KeysState { release: 10.0, dynamics: 100.0 } })),
            index: None,
        }],
    ));
    c.push(("main", "removeChannel drops the channel and its notes", vec![Op::RemoveChannel { id: 30 }]));
    c.push(("main", "removeChannel refuses an unknown channel", vec![Op::RemoveChannel { id: 99 }]));

    // ---- sources and their dependents
    c.push((
        "main",
        "putSource renames and recolours",
        vec![Op::PutSource { source: Source { name: "Kick 808".into(), color: "#e0a44a".into(), ..source(10, "kick") } }],
    ));
    c.push(("main", "putSource refuses an unknown source", vec![Op::PutSource { source: source(99, "gone") }]));
    c.push((
        "main",
        "removeSource drops clips, sampler channels with their notes, comp takes and references",
        vec![Op::RemoveSource { id: 10 }],
    ));
    c.push(("main", "removeSource ignores an unknown source", vec![Op::RemoveSource { id: 99 }])); // the engine is a silent no-op

    // ---- automation
    c.push((
        "main",
        "putAutomation sorts, clamps and normalises the points",
        vec![Op::PutAutomation {
            automation: Automation {
                id: 51,
                name: "EQ thr".into(),
                color: "#e0a44a".into(),
                target: AutoTarget::Fx { fx_id: 901, param: "thr".into() },
                points: vec![
                    AutoPoint { t: 4.0, v: 2.0, tension: 3.0, shape: CurveShape::Hold },
                    AutoPoint { t: 0.0, v: -1.0, tension: 0.0, shape: CurveShape::Curve },
                    AutoPoint { t: 8.0, v: 0.5, tension: -1.0, shape: CurveShape::Stairs },
                ],
            },
        }],
    ));
    c.push(("main", "removeAutomation drops the envelope and its clips", vec![Op::RemoveAutomation { id: 50 }]));

    // ---- pattern and comp removal, tune edits
    c.push(("main", "removePattern drops its clips", vec![Op::RemovePattern { id: 20 }]));
    c.push(("main", "removePattern refuses an unknown pattern", vec![Op::RemovePattern { id: 99 }]));
    c.push((
        "main",
        "editTune writes the clip's tune",
        vec![Op::EditTune { clip: 41, edit: edit(json!({"init": [[0.5, 1.5], [1.5, 2.5]]})) }],
    ));
    c.push(("main", "editTune refuses an unknown clip", vec![Op::EditTune { clip: 99, edit: edit(json!({})) }]));
    c.push(("main", "editTune refuses a non-audio clip", vec![Op::EditTune { clip: 40, edit: edit(json!({})) }]));

    // ---- note transforms
    c.push((
        "main",
        "strum spreads the selected same-start notes",
        vec![Op::TransformNotes {
            pattern: 20,
            verb: TransformVerb::Strum { width: 1.0, ramp: 0.5 },
            seed: None,
            select: Some(TransformSelect::Range { from: 0.0, to: 32.0, ch: Some(31) }),
        }],
    ));
    c.push((
        "main",
        "euclidFill replaces the channel's notes and grows the pattern",
        vec![Op::TransformNotes {
            pattern: 20,
            verb: TransformVerb::EuclidFill { ch: 30, k: 5, n: 8, rotate: 2, vel: 0.8 },
            seed: None,
            select: None,
        }],
    ));
    c.push((
        "main",
        "euclidFill at full velocity writes no velocities",
        vec![Op::TransformNotes {
            pattern: 20,
            verb: TransformVerb::EuclidFill { ch: 30, k: 4, n: 8, rotate: 0, vel: 1.0 },
            seed: None,
            select: None,
        }],
    ));
    c.push((
        "main",
        "melody generates in the project's key and scale",
        vec![Op::TransformNotes {
            pattern: 20,
            verb: TransformVerb::Melody { ch: 31, bars: 2, density: 0.5, repeat: 1 },
            seed: Some(7),
            select: None,
        }],
    ));
    c.push((
        "main",
        "transformNotes refuses melody bars out of range",
        vec![Op::TransformNotes {
            pattern: 20,
            verb: TransformVerb::Melody { ch: 31, bars: 0, density: 0.5, repeat: 1 },
            seed: None,
            select: None,
        }],
    ));
    c.push((
        "main",
        "transformNotes refuses euclid sizes out of range",
        vec![Op::TransformNotes {
            pattern: 20,
            verb: TransformVerb::EuclidFill { ch: 30, k: 0, n: 8, rotate: 0, vel: 0.7 },
            seed: None,
            select: None,
        }],
    ));
    c.push((
        "main",
        "transformNotes refuses drum voices out of range",
        vec![Op::TransformNotes {
            pattern: 20,
            verb: TransformVerb::EuclidDrums {
                voices: vec![wave_engine::transform::DrumVoice { ch: 30, k: 513, n: 8, rotate: 0, vel: 0.7 }],
                bars: 2,
            },
            seed: None,
            select: None,
        }],
    ));
    c.push((
        "main",
        "transformNotes refuses an unknown target channel",
        vec![Op::TransformNotes {
            pattern: 20,
            verb: TransformVerb::EuclidFill { ch: 99, k: 4, n: 8, rotate: 0, vel: 0.7 },
            seed: None,
            select: None,
        }],
    ));
    c.push((
        "main",
        "transformNotes refuses an unknown pattern",
        vec![Op::TransformNotes { pattern: 99, verb: TransformVerb::Strum { width: 0.5, ramp: 0.0 }, seed: None, select: None }],
    ));
    c.push((
        "main",
        "transformNotes refuses a selection index out of range",
        vec![Op::TransformNotes {
            pattern: 20,
            verb: TransformVerb::Strum { width: 0.5, ramp: 0.0 },
            seed: None,
            select: Some(TransformSelect::Indices { indices: vec![99] }),
        }],
    ));
    c.push((
        "main",
        "transformNotes refuses an unknown chord harmony",
        vec![Op::TransformNotes { pattern: 20, verb: TransformVerb::Chord { harmony: "nope".into() }, seed: None, select: None }],
    ));
    c.push((
        "main",
        "chord diatonic needs a key and scale",
        vec![
            Op::SetKey { key: None, scale: 1 },
            Op::TransformNotes { pattern: 20, verb: TransformVerb::Chord { harmony: "diatonic".into() }, seed: None, select: None },
        ],
    ));
    c.push((
        "main",
        "progression refuses an unknown template",
        vec![Op::TransformNotes {
            pattern: 20,
            verb: TransformVerb::Progression { ch: 31, bars: 4, template: "nope".into(), voice_lead: true, degrees: vec![] },
            seed: None,
            select: None,
        }],
    ));
    c.push((
        "main",
        "progression refuses degrees out of the scale",
        vec![Op::TransformNotes {
            pattern: 20,
            verb: TransformVerb::Progression { ch: 31, bars: 4, template: "pop".into(), voice_lead: true, degrees: vec![9] },
            seed: None,
            select: None,
        }],
    ));

    // ---- the rest
    c.push(("main", "setKey sets and clears", vec![Op::SetKey { key: Some(4), scale: 3 }, Op::SetKey { key: None, scale: 1 }]));
    c.push(("main", "setKey refuses a key out of range", vec![Op::SetKey { key: Some(12), scale: 1 }]));
    c.push(("main", "setKey refuses a scale out of range", vec![Op::SetKey { key: Some(0), scale: 5 }]));
    c.push(("main", "setBpm moves the tempo", vec![Op::SetBpm { bpm: 140.0 }]));
    c.push(("main", "setBpm refuses a tempo out of range", vec![Op::SetBpm { bpm: 10.0 }]));
    c.push(("main", "setLoop sets the loop", vec![Op::SetLoop { range: Some(LoopRange { a: 4.0, b: 16.0 }) }]));
    c.push(("main", "setLoop clears the loop", vec![Op::SetLoop { range: None }]));
    c.push(("main", "setLoop refuses an empty loop", vec![Op::SetLoop { range: Some(LoopRange { a: 16.0, b: 4.0 }) }]));
    c.push((
        "main",
        "putComp replaces and normalises the regions",
        vec![Op::PutComp {
            comp: Comp {
                id: 60,
                name: "Vocal Comp".into(),
                takes: vec![take(61, "take-a", 10, 0.0, 2.0), take(62, "take-b", 11, 2.0, 2.0)],
                regions: vec![
                    CompRegion { take: 62, from: 2.0, to: 4.0, xfade: None },
                    CompRegion { take: 61, from: 0.0, to: 2.0, xfade: None },
                ],
                xfade: 0.010,
            },
        }],
    ));
    c.push((
        "main",
        "putComp refuses an empty take group",
        vec![Op::PutComp {
            comp: Comp {
                takes: vec![],
                ..Comp { id: 60, name: "Vocal Comp".into(), takes: vec![take(61, "take-a", 10, 0.0, 2.0)], regions: vec![], xfade: 0.010 }
            },
        }],
    ));
    c.push((
        "main",
        "putComp refuses a take with an invalid range",
        vec![Op::PutComp {
            comp: Comp {
                takes: vec![take(61, "bad", 10, -1.0, 0.0)],
                ..Comp { id: 60, name: "Vocal Comp".into(), takes: vec![], regions: vec![], xfade: 0.010 }
            },
        }],
    ));
    c.push((
        "main",
        "putComp refuses overlapping regions",
        vec![Op::PutComp {
            comp: Comp {
                id: 60,
                name: "Vocal Comp".into(),
                takes: vec![take(61, "take-a", 10, 0.0, 2.0), take(62, "take-b", 11, 2.0, 2.0)],
                regions: vec![
                    CompRegion { take: 61, from: 0.0, to: 3.0, xfade: None },
                    CompRegion { take: 62, from: 2.5, to: 4.0, xfade: None },
                ],
                xfade: 0.010,
            },
        }],
    ));
    c.push((
        "main",
        "putComp refuses a region of an unknown take",
        vec![Op::PutComp {
            comp: Comp {
                regions: vec![CompRegion { take: 99, from: 0.0, to: 2.0, xfade: None }],
                ..Comp { id: 60, name: "Vocal Comp".into(), takes: vec![take(61, "take-a", 10, 0.0, 2.0)], regions: vec![], xfade: 0.010 }
            },
        }],
    ));
    c.push((
        "main",
        "putComp refuses a take of an unknown source",
        vec![Op::PutComp {
            comp: Comp {
                takes: vec![take(61, "gone", 99, 0.0, 2.0)],
                ..Comp { id: 60, name: "Vocal Comp".into(), takes: vec![], regions: vec![], xfade: 0.010 }
            },
        }],
    ));
    c.push((
        "main",
        "putComp refuses a crossfade out of range",
        vec![Op::PutComp {
            comp: Comp {
                xfade: 0.9,
                ..Comp { id: 60, name: "Vocal Comp".into(), takes: vec![take(61, "take-a", 10, 0.0, 2.0)], regions: vec![], xfade: 0.010 }
            },
        }],
    ));
    c.push(("main", "removeComp drops the group and its clips", vec![Op::RemoveComp { id: 60 }]));
    c.push(("main", "removeComp refuses an unknown comp", vec![Op::RemoveComp { id: 99 }]));
    c.push((
        "main",
        "putNotebook writes the pages",
        vec![Op::PutNotebook {
            notebook: Notebook { pages: vec![page(70, "Verse", 1, "[Verse]\nfirst line"), page(71, "Hook", 5, "la la la")] },
        }],
    ));
    c.push((
        "main",
        "putNotebook refuses a page id used twice",
        vec![Op::PutNotebook { notebook: Notebook { pages: vec![page(70, "A", 1, "a"), page(70, "B", 2, "b")] } }],
    ));
    c.push((
        "main",
        "putNotebook refuses a start bar out of range",
        vec![Op::PutNotebook { notebook: Notebook { pages: vec![page(70, "A", 0, "a")] } }],
    ));
    c.push((
        "main",
        "putNotebook refuses a name longer than 80 characters",
        vec![Op::PutNotebook { notebook: Notebook { pages: vec![page(70, &"x".repeat(81), 1, "a")] } }],
    ));

    // ---- batches
    c.push((
        "main",
        "a failing batch changes nothing",
        vec![Op::SetBpm { bpm: 140.0 }, Op::PutClips { clips: vec![Clip { len: 0.0, ..good_audio(44) }] }],
    ));
    c.push(("main", "an empty batch changes nothing", vec![]));
    c
}
