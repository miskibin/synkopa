//! Decoded audio stays bounded: a source no part of the history can reach any more loses
//! its decoded audio when the store is garbage-collected; one still referenced keeps it.

use std::path::PathBuf;
use std::sync::Arc;

use wave_engine::*;

const SR: u32 = 48000;

fn vol(e: &Engine, v: f32) -> Op {
    let mut t = e.project().tracks[0].clone();
    t.vol = v;
    Op::PutTrack { track: t }
}

#[test]
fn sources_the_history_aged_out_lose_their_audio() {
    let dir = std::env::temp_dir().join(format!("wave-gc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut paths: Vec<PathBuf> = vec![];
    for k in 0..2 {
        let path = dir.join(format!("take{k}.wav"));
        let data: Vec<f32> = (0..SR as usize).flat_map(|_| [0.2f32, 0.2]).collect();
        wave_engine::sources::write_wav(&path, &data, SR).unwrap();
        paths.push(path);
    }
    let e = Engine::new(Project::empty(), Arc::new(NoPlugins));
    let (srcs, ..) = e.import_audio(&paths, None).unwrap();
    let (gone, stays) = (srcs[0], srcs[1]);
    assert!(e.source_peaks(gone, false, false).is_some());
    e.apply(&[Op::RemoveSource { id: gone }], false, None).unwrap();
    assert!(e.source_peaks(stays, false, false).is_some(), "a source still in the project keeps its audio");
    // age every undo entry that still holds the removed source out of the undo limit; the
    // collection happens when the history drops the entry, not only at the next undo
    for k in 0..210 {
        e.apply(&[vol(&e, if k % 2 == 0 { 0.3 } else { 0.4 })], false, None).unwrap();
    }
    assert!(e.source_peaks(gone, false, false).is_none(), "history eviction collects the audio itself");
    e.undo(); // the store gc runs with the undo too
    assert!(e.source_peaks(gone, false, false).is_none(), "audio of a source no history entry references goes");
    assert!(e.source_peaks(stays, false, false).is_some(), "reachable audio stays");
    // and the project is none the wiser: undo and redo still work
    assert!(e.redo());
    let _ = std::fs::remove_dir_all(dir);
}
