//! "Extract stems": the region a clip plays is copied out with context ([`Engine::stem_region`]),
//! and separated stems come back as one track and clip each right below it, playing exactly what
//! the clip played, with the clip muted, in one undo step ([`Engine::place_stems`]). The separation
//! itself (HTDemucs) is `app/src/stems_actions.rs`; here the "stems" are known splits of the region,
//! so their sum must render exactly like the clip did.

use std::path::PathBuf;
use std::sync::Arc;

use wave_engine::sources::Audio;
use wave_engine::*;

const SR: u32 = 48000;

/// 10 s stereo file whose every sample says where it is (left ramps up, right is a 220 Hz sine).
fn song(tag: &str) -> (PathBuf, Vec<f32>) {
    let dir = std::env::temp_dir().join(format!("wave-stems-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("song.wav");
    let n = 10 * SR as usize;
    let data: Vec<f32> =
        (0..n).flat_map(|i| [0.5 * i as f32 / n as f32, 0.3 * (i as f32 * 220.0 * std::f32::consts::TAU / SR as f32).sin()]).collect();
    wave_engine::sources::write_wav(&wav, &data, SR).unwrap();
    (wav, data)
}

fn dry(mut p: Project) -> Project {
    for m in p.inserts.iter_mut() {
        m.fx.clear();
    }
    p
}

/// The engine with the song as one clip on track 1 that plays `from_s..to_s` of it from beat 4.
fn setup(tag: &str, from_s: f64, to_s: f64, tweak: impl Fn(&mut Clip)) -> (Arc<Engine>, Id, Vec<f32>) {
    let (wav, data) = song(tag);
    let e = Engine::new(dry(Project::empty()), Arc::new(NoPlugins));
    let (_, clips, _) = e.import_audio(&[wav], Some((1, 4.0))).unwrap();
    let spb = 60.0 / e.project().bpm;
    let mut c = e.project().clips.iter().find(|c| c.id == clips[0]).cloned().unwrap();
    c.offset = from_s / spb;
    c.len = (to_s - from_s) / spb;
    tweak(&mut c);
    e.apply(&[Op::PutClips { clips: vec![c.clone()] }], false, None).unwrap();
    (e, c.id, data)
}

/// `region` split into `parts` that add up to it (fake stems at weights `w`).
fn fake_stems(region: &StemRegion, w: &[f32]) -> Vec<StemFile> {
    w.iter()
        .enumerate()
        .map(|(k, &g)| {
            let data: Vec<f32> = region.data.iter().map(|v| v * g).collect();
            let path = region.dir.join(format!("stem-test-{k}-{}.wav", std::process::id()));
            StemFile {
                label: ["Drums", "Bass", "Other", "Vocals"][k].into(),
                color: "#e8a33d".into(),
                path,
                audio: Audio::from_stereo(data, region.sr, 2),
            }
        })
        .collect()
}

#[test]
fn the_region_is_what_the_clip_plays_with_context_either_side() {
    let (e, id, data) = setup("region", 4.0, 6.0, |_| {});
    let r = e.stem_region(id, 3.0).unwrap();
    assert_eq!(r.sr, SR);
    assert!((r.from_s - 1.0).abs() < 1e-9, "{}", r.from_s);
    assert!((r.source_s - 10.0).abs() < 1e-9);
    assert_eq!(r.data.len(), 2 * 8 * SR as usize, "1 s .. 9 s");
    assert_eq!(&r.data[..], &data[2 * SR as usize..2 * 9 * SR as usize]);
    assert_eq!(r.name, "song");
    // at the edges of the file the context is cut, not padded
    let (e, id, _) = setup("edges", 0.5, 9.0, |_| {});
    let r = e.stem_region(id, 3.0).unwrap();
    assert_eq!(r.from_s, 0.0);
    assert_eq!(r.data.len(), 2 * 10 * SR as usize);
}

#[test]
fn a_reversed_clip_is_separated_as_it_plays() {
    let (e, id, data) = setup("rev", 4.0, 6.0, |c| c.rev = true);
    let r = e.stem_region(id, 0.0).unwrap();
    // the reversed file's 4 s .. 6 s are the original's 6 s .. 4 s backwards
    let n = data.len() / 2;
    let first = 4 * SR as usize;
    assert_eq!(r.data[0], data[2 * (n - 1 - first)]);
}

#[test]
fn stems_go_below_the_clip_play_what_it_played_and_undo_in_one_step() {
    // a resampled stretch and clip gain: the stems copy both and must still line up sample for sample
    let (e, id, _) = setup("place", 4.0, 7.0, |c| {
        c.stretch = 1.25;
        c.resample = true;
        c.gain = -4.0;
        c.offset *= 1.25;
        c.len *= 1.25;
    });
    let spb = 60.0 / e.project().bpm;
    let src = e.project().clips.iter().find(|c| c.id == id).cloned().unwrap();
    let span = (src.start, src.start + src.len);
    let want = e.render(span.0, span.1, 0.0, SR).unwrap();
    assert!(want.iter().any(|v| v.abs() > 0.05));

    let before = e.project();
    let region = e.stem_region(id, 3.0).unwrap();
    let r = e.place_stems(&region, fake_stems(&region, &[0.25, 0.5, 0.25])).unwrap();
    let p = e.project();
    assert_eq!(r.track, 2);
    assert_eq!(p.tracks.len(), before.tracks.len() + 3);
    assert_eq!(p.tracks[2].name, "song – Drums");
    assert_eq!(p.tracks[4].name, "song – Other");
    assert_eq!(p.tracks[2].insert, p.tracks[1].insert, "stems play where the clip's track does");
    assert!(p.clips.iter().find(|c| c.id == id).unwrap().muted);
    for (k, cid) in r.clip_ids.iter().enumerate() {
        let c = p.clips.iter().find(|c| c.id == *cid).unwrap();
        assert_eq!((c.track, c.start, c.len, c.stretch, c.gain, c.muted), (2 + k as u32, src.start, src.len, 1.25, -4.0, false));
        // the stem file starts at 1 s of the song, so the clip starts 3 s into it
        assert!((c.offset - 3.0 * 1.25 / spb).abs() < 1e-9, "{}", c.offset);
        let s = p.sources.iter().find(|s| s.id == c.source).unwrap();
        assert_eq!(s.name, p.tracks[2 + k].name);
    }
    // the sum of the stems renders like the clip did
    let got = e.render(span.0, span.1, 0.0, SR).unwrap();
    assert_eq!(got.len(), want.len());
    let err = got.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
    assert!(err < 1e-4, "max difference {err}");

    assert!(e.undo());
    assert_eq!(e.project(), before);
    assert!(e.redo());
    assert_eq!(e.project().tracks.len(), before.tracks.len() + 3);
}

#[test]
fn nothing_is_placed_when_the_clip_changed_or_went_away() {
    let (e, id, _) = setup("changed", 4.0, 6.0, |_| {});
    let region = e.stem_region(id, 3.0).unwrap();
    // the clip now plays past what was separated
    let mut c = e.project().clips.iter().find(|c| c.id == id).cloned().unwrap();
    let spb = 60.0 / e.project().bpm;
    c.len += 4.0 / spb;
    e.apply(&[Op::PutClips { clips: vec![c.clone()] }], false, None).unwrap();
    let before = e.project();
    assert!(e.place_stems(&region, fake_stems(&region, &[1.0])).is_err());
    assert_eq!(e.project(), before);
    // moving it on the timeline or to another track is fine: the stems follow it
    c.len -= 4.0 / spb;
    c.start = 12.0;
    c.track = 0;
    e.apply(&[Op::PutClips { clips: vec![c] }], false, None).unwrap();
    let r = e.place_stems(&region, fake_stems(&region, &[1.0])).unwrap();
    assert_eq!(r.track, 1);
    assert_eq!(e.project().clips.iter().find(|x| x.id == r.clip_ids[0]).unwrap().start, 12.0);
    // deleted while separating
    let region = e.stem_region(id, 3.0).unwrap();
    e.apply(&[Op::RemoveClips { ids: vec![id] }], false, None).unwrap();
    let before = e.project();
    assert!(e.place_stems(&region, fake_stems(&region, &[1.0])).unwrap_err().to_string().contains("deleted"));
    assert_eq!(e.project(), before);
}

#[test]
fn refuses_what_cannot_be_separated() {
    let (e, id, _) = setup("refuse", 4.0, 6.0, |_| {});
    assert!(e.stem_region(999_999, 3.0).is_err());
    let mut c = e.project().clips.iter().find(|c| c.id == id).cloned().unwrap();
    c.offset = 20.0 / (60.0 / e.project().bpm);
    e.apply(&[Op::PutClips { clips: vec![c] }], false, None).unwrap();
    let err = e.stem_region(id, 3.0).err().map(|x| x.to_string()).unwrap_or_default();
    assert!(err.contains("nothing to separate"), "a clip past the end of its file: {err}");
    let pat = e.project().clips.iter().find(|c| c.kind == ClipKind::Pat).map(|c| c.id);
    if let Some(pat) = pat {
        assert!(e.stem_region(pat, 3.0).is_err());
    }
}
