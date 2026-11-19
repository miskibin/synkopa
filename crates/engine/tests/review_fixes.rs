//! The review-round fixes: a cancel that arrives with the last progress frame still cancels,
//! an open drag never leaks into a replaced project, stem context shrinks to the reopen
//! limit, and a saved scratch take never replaces audio that is already there.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use wave_engine::*;

/// 0.5 s of 440 Hz tone as a wav file, imported onto a fresh engine: one audio clip.
fn engine_with_tone(name: &str) -> (Arc<Engine>, Id, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("wave-fixes-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("tone.wav");
    let tone: Vec<f32> = (0..22050)
        .flat_map(|i| {
            let v = (i as f32 * 440.0 * std::f32::consts::TAU / 44100.0).sin() * 0.4;
            [v, v]
        })
        .collect();
    wave_engine::sources::write_wav(&wav, &tone, 44100).unwrap();
    let e = Engine::new(Project::sketch(), Arc::new(NoPlugins));
    let (_, clips, _) = e.import_audio(std::slice::from_ref(&wav), Some((0, 4.0))).unwrap();
    (e, clips[0], dir)
}

#[test]
fn a_cancel_at_the_final_progress_frame_still_cancels() {
    let (e, _clip, dir) = engine_with_tone("cancel");
    let cancel = AtomicBool::new(false);
    // a plain tail runs the loop to the very last frame, where the final progress callback
    // fires: cancelling from it must not hand back the finished audio as a success
    let r = e.render_export_with(None, 0.0, 8.0, RenderTail::Seconds(0.0), 48_000, &cancel, &mut |p| {
        if let RenderProgress::Rendering { done, total, .. } = p {
            if done >= total {
                cancel.store(true, Ordering::Relaxed);
            }
        }
    });
    assert!(matches!(r, Err(EngineError::Cancelled)), "a cancelled render must not succeed: {r:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_open_drag_never_leaks_into_a_replaced_project() {
    let e = Engine::new(Project::sketch(), Arc::new(NoPlugins));
    e.begin_gesture(None);
    let drag = Clip {
        id: 9100,
        track: 1,
        start: 20.0,
        len: 4.0,
        offset: 0.0,
        kind: ClipKind::Pat,
        source: 1,
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
    };
    e.apply(&[Op::PutClips { clips: vec![drag] }], true, None).unwrap();
    // replacing the project must forget the drag's snapshot (as undo/redo do)
    e.new_project(Project::sketch()).unwrap();
    e.end_gesture();
    assert!(!e.undo(), "the replaced project must not undo into the old one");
    assert!(!e.project().clips.iter().any(|c| c.id == 9100), "the dragged clip stayed behind");
}

#[test]
fn stem_context_shrinks_so_the_region_fits_the_reopen_limit() {
    let (e, clip, dir) = engine_with_tone("stems");
    // a context far past the limit must not grow the written region past it: the file is
    // what Audio::load decodes again when the project reopens (it stops at STEMS_MAX_S)
    let region = e.stem_region(clip, 100_000.0).unwrap();
    let secs = region.data.len() as f64 / 2.0 / f64::from(region.sr);
    assert!(secs <= STEMS_MAX_S, "the region is {secs:.0} s, past the {STEMS_MAX_S:.0} s reopen limit");
    assert!(secs > 0.3, "the clip's own span must survive the trim ({secs})");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_saved_scratch_take_never_replaces_audio_that_is_already_there() {
    let scratch = wave_engine::engine::scratch_dir();
    std::fs::create_dir_all(&scratch).unwrap();
    let take = scratch.join("fixes-take.wav");
    let tone: Vec<f32> = (0..22050).flat_map(|i| [(i as f32 * 0.01).sin(), -(i as f32 * 0.01).sin()]).collect();
    wave_engine::sources::write_wav(&take, &tone, 44100).unwrap();
    let dir_a = std::env::temp_dir().join(format!("wave-fixes-save-a-{}", std::process::id()));
    let dir_b = std::env::temp_dir().join(format!("wave-fixes-save-b-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir_a);
    let _ = std::fs::remove_dir_all(&dir_b);
    std::fs::create_dir_all(dir_b.join("Song Audio")).unwrap();
    // the audio project B already references: saving B must not overwrite it
    let occupied = dir_b.join("Song Audio").join("fixes-take.wav");
    let other: Vec<f32> = vec![0.9, -0.9];
    wave_engine::sources::write_wav(&occupied, &other, 44100).unwrap();

    let e_a = Engine::new(Project::sketch(), Arc::new(NoPlugins));
    e_a.import_audio(std::slice::from_ref(&take), None).unwrap();
    e_a.save(&dir_a.join("Song.synkopa")).unwrap();

    e_a.new_project(Project::sketch()).unwrap();
    e_a.import_audio(std::slice::from_ref(&take), None).unwrap();
    e_a.save(&dir_b.join("Song.synkopa")).unwrap();

    // B's existing audio survived, the take landed beside it under a fresh name
    let kept = wave_engine::sources::Audio::load(&occupied).unwrap();
    assert_eq!(kept.frames, 1, "the existing file was overwritten");
    let audio = dir_b.join("Song Audio").join("fixes-take (2).wav");
    assert!(audio.is_file(), "the moved take went to {} instead", audio.display());
    let _ = std::fs::remove_dir_all(dir_a);
    let _ = std::fs::remove_dir_all(dir_b);
    let _ = std::fs::remove_file(&take);
}

#[test]
fn overlapping_copies_of_the_same_stem_each_publish_their_own_file() {
    let e = Engine::new(Project::sketch(), Arc::new(NoPlugins));
    let dir = std::env::temp_dir().join(format!("wave-fixes-copy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("Song.synkopa");
    let b = dir.join("Song.wave");
    let t1 = e.save_copy(&a);
    let t2 = e.save_copy(&b);
    assert!(t1.is_ok() && t2.is_ok(), "{t1:?} / {t2:?}");
    assert!(a.is_file() && b.is_file());
    let _ = std::fs::remove_dir_all(dir);
}

// ------------------------------------------------------------- round 2

use std::sync::mpsc;
use std::time::Duration;

/// A hosted plugin whose latency report only answers after the test says so: the query is
/// paused mid-flight, the plugin replaced, and the stale report must be discarded.
struct BlockingLatency {
    entered: mpsc::Sender<()>,
    release: std::sync::Mutex<mpsc::Receiver<()>>,
}
impl PluginFactory for BlockingLatency {
    fn create(&self, _: Id, _: &PluginRef, _: f32, _: usize, _: bool) -> Result<Box<dyn Processor>, String> {
        Ok(Box::new(Fixed(128)))
    }
    fn latency(&self, _: Id) -> Option<usize> {
        self.entered.send(()).ok()?;
        let _ = self.release.lock().ok()?.recv_timeout(Duration::from_secs(5));
        Some(512)
    }
}

/// A processor with a fixed reported latency.
struct Fixed(usize);
impl Processor for Fixed {
    fn prepare(&mut self, _: f32, _: usize) {}
    fn process(&mut self, _: &mut [f32], _: &mut [f32], _: &ProcessCtx) {}
    fn latency_samples(&self) -> usize {
        self.0
    }
}

fn plugin_fx(id: Id) -> Fx {
    let mut f = catalog::new_fx(id, FxKind::Plugin, 120.0);
    f.plugin = Some(PluginRef {
        format: "lv2".into(),
        path: "/usr/lib/lv2/delay.lv2".into(),
        cid: "test".into(),
        name: "Look-ahead".into(),
        vendor: "Test".into(),
        state: None,
    });
    f
}

#[test]
fn a_plugin_replaced_during_a_latency_query_discards_the_stale_report() {
    let (entered_tx, entered) = mpsc::channel();
    let (release_tx, release) = mpsc::channel();
    let factory = Arc::new(BlockingLatency { entered: entered_tx, release: std::sync::Mutex::new(release) });
    let mut p = Project::sketch();
    p.inserts[1].fx.push(plugin_fx(7000));
    let e = Engine::new(p, factory.clone());
    e.create_rt(48_000.0, 256);
    let before = e.playback_latency();
    assert!(before >= 128, "the instance's own latency is in the mix ({before})");

    let e2 = e.clone();
    let query = std::thread::spawn(move || e2.refresh_plugin_latency());
    entered.recv_timeout(Duration::from_secs(5)).expect("the query reached the factory");
    // the plugin is replaced while the query is parked in the factory (it holds no engine
    // lock): the generation moves before the report can be collected, deterministically
    e.apply(&[Op::PutInsert { insert: plugin_insert(7001) }], false, None).unwrap();
    release_tx.send(()).ok();
    let changed = query.join().unwrap();
    assert!(!changed, "a report collected before the replacement must be discarded");
    assert_eq!(e.playback_latency(), before, "the stale 512 never landed");
}

/// A whole insert carrying one plugin effect, for mixer-dirty edits in these tests.
fn plugin_insert(id: Id) -> Insert {
    let mut ins = Insert { id, name: format!("Bus {id}"), ..Project::sketch().inserts[1].clone() };
    ins.fx = vec![plugin_fx(id)];
    ins
}

#[test]
fn relinking_during_extraction_rejects_the_placement_without_mutating() {
    let (e, clip, dir) = engine_with_tone("relink-stems");
    let region = e.stem_region(clip, 2.0).unwrap();
    // swap the audio under the same source id while the separator "runs"
    let other = dir.join("other.wav");
    let tone: Vec<f32> = (0..22050).flat_map(|i| [(i as f32 * 0.02).sin(), 0.0]).collect();
    wave_engine::sources::write_wav(&other, &tone, 44100).unwrap();
    let src = e.project().clips.iter().find(|c| c.id == clip).unwrap().source;
    e.relink_sources(&[Relink { source_id: src, path: other.display().to_string() }]).unwrap();
    let stems = vec![StemFile {
        label: "Drums".into(),
        color: "#123456".into(),
        path: dir.join("stem.wav"),
        audio: wave_engine::sources::Audio::from_stereo(region.data[..4000].to_vec(), region.sr, 2),
    }];
    assert!(e.place_stems(&region, stems).is_err(), "stems of the old audio must not be placed");
    assert!(!e.project().clips.iter().any(|c| c.id == clip && c.muted), "the clip stayed unmuted");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn apply_with_library_decodes_what_the_project_does_not_have() {
    let e = Engine::new(Project::empty(), Arc::new(NoPlugins));
    let requested = |ids: &std::collections::HashMap<String, Id>| {
        assert!(ids.contains_key("kick-808.flac"), "build must always see the requested sample");
        Ok(vec![])
    };
    e.apply_with_library(&["kick-808.flac"], None, |_, ids| requested(ids)).unwrap();
    assert!(e.project().sources.iter().any(|s| s.path.ends_with("kick-808.flac")));
    // the sample leaves the project again (a watch would do the same): the next apply decodes
    // it fresh and build still gets its id — the path a concurrent replacement takes
    e.set_project(Project::empty(), None, None).unwrap();
    e.apply_with_library(&["kick-808.flac"], None, |_, ids| requested(ids)).unwrap();
    assert!(e.project().sources.iter().any(|s| s.path.ends_with("kick-808.flac")));
}

#[test]
fn a_bare_relative_name_opens_against_the_working_directory() {
    let e = Engine::new(Project::sketch(), Arc::new(NoPlugins));
    let name = "wave-fixes-bare.synkopa";
    e.save(Path::new(name)).unwrap();
    let e2 = Engine::new(Project::empty(), Arc::new(NoPlugins));
    // a bare file name has an empty parent: media still resolves against "."
    e2.open(Path::new(name)).unwrap();
    assert!(e2.state().path.as_ref().is_some_and(|p| p.ends_with(name)));
    let _ = std::fs::remove_file(name);
}

#[test]
fn overlapping_copies_each_publish_their_own_snapshot() {
    let e_a = Engine::new(Project::sketch(), Arc::new(NoPlugins));
    let mut rich = Project::sketch();
    rich.tracks[0].name = "MARKER-A".into();
    let e_b = Engine::new(rich, Arc::new(NoPlugins));
    let dir = std::env::temp_dir().join(format!("wave-fixes-copy2-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("Song.synkopa");
    let b = dir.join("Song.wave");
    let (ra, rb) = (e_a.save_copy(&a), e_b.save_copy(&b));
    assert!(ra.is_ok() && rb.is_ok(), "{ra:?} / {rb:?}");
    // each call published its own project, not the other's snapshot
    let doc_a = std::fs::read_to_string(&a).unwrap();
    let doc_b = std::fs::read_to_string(&b).unwrap();
    assert!(doc_a.contains("Drums") && !doc_a.contains("MARKER-A"), "A published A");
    assert!(doc_b.contains("MARKER-A"), "B published B");
    let _ = std::fs::remove_dir_all(dir);
}
