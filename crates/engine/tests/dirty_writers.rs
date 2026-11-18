//! Every path that writes into a track or insert buffer must mark it (`dirty`): the mixer
//! skips buffers nobody wrote, and `gate` skips scanning them. A missed writer is not a
//! subtle regression — the audio silently disappears (a missed *un*-mark would click).

use std::sync::Arc;

use wave_engine::*;

const SR: u32 = 48000;

fn peak(out: &[f32], from_s: f32, to_s: f32) -> f32 {
    let a = (from_s * SR as f32) as usize..(to_s * SR as f32) as usize;
    out[a].iter().fold(0.0f32, |acc, &v| acc.max(v.abs()))
}

fn clip(id: Id, track: u32, start: f64, len: f64, pat: Id) -> Clip {
    Clip {
        id,
        track,
        start,
        len,
        offset: 0.0,
        kind: ClipKind::Pat,
        source: pat,
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
        root: 60,
    }
}

/// Pattern notes on a track: the notes render through the synth bank, the track buffer is
/// mixed into the insert only while its writer marked it.
#[test]
fn pattern_notes_mark_their_track() {
    let mut p = Project::routed();
    p.inserts[0].fx.clear();
    let kick = p.channels.iter().find(|c| c.name == "Kick").unwrap().id;
    p.patterns = vec![Pattern {
        id: 1,
        name: "P".into(),
        color: "#4f8ef7".into(),
        len: 4.0,
        min_len: 4.0,
        swing: 0.0,
        notes: vec![Note::new(kick, 0.0, 0.5, Some(60)), Note::new(kick, 2.0, 0.5, Some(60))],
    }];
    p.clips = vec![clip(10, 1, 0.0, 4.0, 1)];
    let out = Engine::new(p, Arc::new(NoPlugins)).render(0.0, 4.0, 0.0, SR).unwrap();
    let hit = peak(&out, 0.0, 0.5);
    assert!(hit > 0.05, "pattern notes on track 1 are silent (peak {hit}) — the note writer does not mark the track buffer");
}

/// A channel routed past its track straight into a bus insert: the note writer must mark the
/// insert buffer (the track mix never touches it).
#[test]
fn routed_notes_mark_their_insert() {
    let mut p = Project::routed();
    p.inserts[0].fx.clear();
    // playlist lanes play into the master (insert 0), so a bus is a genuinely different sink:
    // if the direct-to-insert writer is missed, the notes die at the bus and the test fails
    let bus = p.inserts[1].id;
    assert_ne!(p.tracks[1].insert, bus, "the fixture must not route track 1 through the bus under test");
    let kick = p.channels.iter().find(|c| c.name == "Kick").unwrap().id;
    p.channels.iter_mut().find(|c| c.id == kick).unwrap().insert = Some(bus);
    p.patterns = vec![Pattern {
        id: 1,
        name: "P".into(),
        color: "#4f8ef7".into(),
        len: 4.0,
        min_len: 4.0,
        swing: 0.0,
        notes: vec![Note::new(kick, 0.0, 0.5, Some(60))],
    }];
    p.clips = vec![clip(10, 1, 0.0, 4.0, 1)];
    let out = Engine::new(p, Arc::new(NoPlugins)).render(0.0, 4.0, 0.0, SR).unwrap();
    let hit = peak(&out, 0.0, 0.5);
    assert!(hit > 0.05, "routed notes are silent (peak {hit}) — the note writer does not mark the insert buffer");
}

/// An audio clip is mixed into its track buffer by the clip mixer itself (not by a renderer).
#[test]
fn audio_clips_mark_their_track() {
    // a directory the test owns: cleaned up on success and on failure alike
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("tone.wav");
    let x: Vec<f32> = (0..SR as usize * 5)
        .flat_map(|i| {
            let v = (i as f32 * 220.0 * std::f32::consts::TAU / SR as f32).sin() * 0.5;
            [v, v]
        })
        .collect();
    sources::write_wav(&wav, &x, SR).unwrap();
    let mut p = Project::empty();
    p.inserts[0].fx.clear();
    let e = Engine::new(p, Arc::new(NoPlugins));
    e.import_audio(std::slice::from_ref(&wav), Some((1, 0.0))).unwrap();
    let out = e.render(0.0, 4.0, 0.0, SR).unwrap();
    let hit = peak(&out, 0.5, 1.5);
    assert!(hit > 0.3, "audio clip on track 1 is silent (peak {hit}) — the clip mixer does not mark the track buffer");
}
