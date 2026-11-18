//! Slow work (copying and writing audio files) must not hold the engine lock: the UI keeps
//! reading state and the transport keeps serving commands while a file is written.

use std::sync::Arc;
use std::time::{Duration, Instant};

use wave_engine::*;

const SR: u32 = 48000;

#[test]
fn trimming_a_long_clip_does_not_block_the_engine() {
    let dir = std::env::temp_dir().join(format!("wave-slow-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("long.wav");
    // five minutes of audio: the copy and the wav write take at least tens of ms — long
    // enough that a lock held across them would stall every state() call in the window
    let data: Vec<f32> = (0..300 * SR as usize).flat_map(|_| [0.3f32, 0.3]).collect();
    wave_engine::sources::write_wav(&path, &data, SR).unwrap();
    drop(data);
    let mut p = Project::empty();
    for m in p.inserts.iter_mut() {
        m.fx.clear();
    }
    let e = Engine::new(p, Arc::new(NoPlugins));
    let clip_id = e.import_audio(std::slice::from_ref(&path), Some((0, 0.0))).unwrap().1[0];
    e.save(&dir.join("song.synkopa")).unwrap();

    let e2 = e.clone();
    let h = std::thread::spawn(move || e2.trim_clip(clip_id));
    // sample the engine while the trim copies and writes; the median keeps one scheduler
    // hiccup from failing the test
    let mut lats: Vec<Duration> = vec![];
    while !h.is_finished() {
        let t0 = Instant::now();
        let _ = e.state();
        lats.push(t0.elapsed());
        std::thread::sleep(Duration::from_millis(2));
    }
    let id = h.join().unwrap().unwrap();
    lats.sort();
    let med = lats[lats.len() / 2];
    eprintln!("state() median over {} samples while trimming: {:.2?}", lats.len(), med);
    assert!(med < Duration::from_millis(16), "the engine stalled {:.2?} during a trim", med);
    let p = e.project();
    let c = p.clips.iter().find(|c| c.id == clip_id).unwrap();
    assert_eq!(c.source, id, "the clip now plays the trimmed source");
    assert_eq!(c.offset, 0.0);
    let _ = std::fs::remove_dir_all(dir);
}
