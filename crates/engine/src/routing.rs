//! Insert-to-insert routing, as in FL Studio's mixer: besides (or instead of) the master, an
//! insert can send its post-fader output to other inserts, each at its own level. A bus that
//! sums a group (drums into one insert) and a shared effect (vocal and keys sending to one
//! reverb) are both sends. The routing must stay a DAG: a send that would close a loop is refused.
//!
//! Everything here works on a [`Project`] and insert *indices* (positions in `p.inserts`), the
//! way the audio graph addresses inserts. The master (index 0) receives the inserts whose
//! `master` flag is set; it never sends.
//!
//! Mirrored for the UI in `ui/src/lib/routing.ts` (loop checks, solo).

use crate::model::{Id, Insert, Project};

/// The highest send level (linear, +6 dB).
pub const SEND_MAX: f32 = 2.0;

/// The routing the audio graph runs: per insert (by index), its sends in the order kept and
/// the level it is processed at. An insert is processed after every insert sending into it.
#[derive(Clone, Debug, PartialEq)]
pub struct Order {
    /// `(target index, gain)` per insert. Sends to unknown inserts, to itself, to the master or
    /// closing a loop (a project edited by hand) are left out.
    pub sends: Vec<Vec<(usize, f32)>>,
    /// 0: nothing sends into it; otherwise one more than the deepest insert sending into it.
    /// The master's is 0 and means nothing: it is processed last.
    pub level: Vec<usize>,
    /// The deepest level of a non-master insert (0 when nothing is routed between inserts).
    pub depth: usize,
}

/// The sends of every insert as indices, without the invalid ones (see [`Order::sends`]),
/// and the processing levels.
pub fn order(p: &Project) -> Order {
    let n = p.inserts.len();
    let idx = |id: Id| p.inserts.iter().position(|m| m.id == id);
    let mut sends: Vec<Vec<(usize, f32)>> = p
        .inserts
        .iter()
        .enumerate()
        .map(|(k, m)| {
            if k == 0 {
                return vec![];
            }
            let mut out: Vec<(usize, f32)> = vec![];
            for s in &m.sends {
                if let Some(t) = idx(s.to).filter(|&t| t != 0 && t != k && !out.iter().any(|o| o.0 == t)) {
                    out.push((t, clamp_gain(s.gain)));
                }
            }
            out
        })
        .collect();
    // Kahn's algorithm; whatever is left in a loop loses the sends into its first insert
    let mut level = vec![0usize; n];
    loop {
        let mut indeg = vec![0usize; n];
        for v in &sends {
            for &(t, _) in v {
                indeg[t] += 1;
            }
        }
        let mut queue: Vec<usize> = (1..n).filter(|&k| indeg[k] == 0).collect();
        let mut seen = 0;
        level.iter_mut().for_each(|l| *l = 0);
        while let Some(k) = queue.pop() {
            seen += 1;
            for &(t, _) in &sends[k] {
                level[t] = level[t].max(level[k] + 1);
                indeg[t] -= 1;
                if indeg[t] == 0 {
                    queue.push(t);
                }
            }
        }
        if seen + 1 >= n {
            break;
        }
        let stuck = (1..n).find(|&k| indeg[k] > 0).expect("a node left in a loop");
        for v in sends.iter_mut() {
            v.retain(|&(t, _)| t != stuck);
        }
    }
    let depth = level.iter().skip(1).copied().max().unwrap_or(0);
    Order { sends, level, depth }
}

fn clamp_gain(g: f32) -> f32 {
    if g.is_finite() {
        g.clamp(0.0, SEND_MAX)
    } else {
        1.0
    }
}

/// Whether insert `to` is `from` or already reaches it through sends (so `from → to` would
/// close a loop).
pub fn would_loop(p: &Project, from: Id, to: Id) -> bool {
    reaches(p, to, from)
}

/// Whether `a` is `b` or reaches it through sends.
pub fn reaches(p: &Project, a: Id, b: Id) -> bool {
    let mut stack = vec![a];
    let mut seen: Vec<Id> = vec![];
    while let Some(x) = stack.pop() {
        if x == b {
            return true;
        }
        if seen.contains(&x) {
            continue;
        }
        seen.push(x);
        if let Some(m) = p.insert(x) {
            stack.extend(m.sends.iter().map(|s| s.to));
        }
    }
    false
}

/// Checks the routing of `m` (about to replace the insert with its id, or to be added).
pub fn validate(p: &Project, m: &Insert) -> Result<(), String> {
    if m.id == 0 {
        if !m.sends.is_empty() || !m.master {
            return Err("the master sends nowhere: it is the output".into());
        }
        return Ok(());
    }
    for (i, s) in m.sends.iter().enumerate() {
        if s.to == 0 {
            return Err("a send to the master: set `master` instead".into());
        }
        if s.to == m.id {
            return Err(format!("insert {} cannot send to itself", m.id));
        }
        if p.insert(s.to).is_none() {
            return Err(format!("insert {} sends to insert {}, which does not exist", m.id, s.to));
        }
        if m.sends[..i].iter().any(|o| o.to == s.to) {
            return Err(format!("insert {} sends to insert {} twice", m.id, s.to));
        }
        if !s.gain.is_finite() || s.gain < 0.0 || s.gain > SEND_MAX {
            return Err(format!("send level {} out of range 0..{SEND_MAX}", s.gain));
        }
        if reaches(p, s.to, m.id) {
            return Err(format!("insert {} → {} would make a loop: {} already sends into {}", m.id, s.to, s.to, m.id));
        }
    }
    Ok(())
}

/// Delay compensation along the routing. Every input of an insert is lined up with its
/// slowest one, and every path into the master with the slowest path, so parallel routes
/// (a dry insert to the master, the same sound through a bus with a look-ahead limiter) stay
/// in time. Frames throughout.
#[derive(Clone, Debug, PartialEq)]
pub struct Latency {
    /// Delay of each insert's own input (tracks, clips, channels) before its chain: the
    /// latency of the slowest insert sending into it.
    pub input: Vec<usize>,
    /// Delay of each send, aligned with [`Order::sends`].
    pub sends: Vec<Vec<usize>>,
    /// Delay of each insert's output into the master (0 for the master and inserts not
    /// routed to it).
    pub master: Vec<usize>,
    /// Where everything meets at the master's input.
    pub at_master: usize,
}

/// `lat`: each insert's chain latency, by index.
pub fn latency(p: &Project, o: &Order, lat: &[usize]) -> Latency {
    let n = p.inserts.len();
    let mut by_level: Vec<usize> = (1..n).collect();
    by_level.sort_by_key(|&k| o.level[k]);
    let mut input = vec![0usize; n];
    for &k in &by_level {
        let out = input[k] + lat[k];
        for &(t, _) in &o.sends[k] {
            input[t] = input[t].max(out);
        }
    }
    let out = |k: usize| input[k] + lat[k];
    let at_master = (1..n).filter(|&k| p.inserts[k].master).map(out).max().unwrap_or(0);
    let master = (0..n).map(|k| if k > 0 && p.inserts[k].master { at_master - out(k) } else { 0 }).collect();
    let sends = (0..n).map(|k| o.sends[k].iter().map(|&(t, _)| input[t] - out(k)).collect()).collect();
    input[0] = 0;
    Latency { input, sends, master, at_master }
}

/// Which inserts are heard (by index). Muted ones are not; with any non-master insert soloed,
/// only the soloed ones, the inserts feeding them and the inserts they feed (their way to the
/// master) are, as in FL Studio.
pub fn audible(p: &Project) -> Vec<bool> {
    let any_solo = p.inserts.iter().any(|m| m.id != 0 && m.solo);
    if !any_solo {
        return p.inserts.iter().map(|m| !m.muted).collect();
    }
    let soloed: Vec<Id> = p.inserts.iter().filter(|m| m.id != 0 && m.solo).map(|m| m.id).collect();
    p.inserts.iter().map(|m| !m.muted && (m.id == 0 || soloed.iter().any(|&s| reaches(p, m.id, s) || reaches(p, s, m.id)))).collect()
}

/// After insert `id` is removed: sends to it go, and an insert left with no output at all
/// (it fed only the removed one) goes to the master again rather than falling silent.
pub fn forget(p: &mut Project, id: Id) {
    for m in p.inserts.iter_mut() {
        let had = !m.sends.is_empty();
        m.sends.retain(|s| s.to != id);
        if had && m.sends.is_empty() && !m.master {
            m.master = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Insert, InsertSend};

    fn ins(id: Id, sends: &[(Id, f32)], master: bool) -> Insert {
        Insert {
            id,
            name: format!("I{id}"),
            color: "#fff".into(),
            vol: 0.8,
            pan: 0.0,
            width: 1.0,
            muted: false,
            solo: false,
            fx: vec![],
            sends: sends.iter().map(|&(to, gain)| InsertSend { to, gain }).collect(),
            master,
        }
    }
    fn proj(inserts: Vec<Insert>) -> Project {
        let mut p = crate::demo::empty_project();
        p.inserts = inserts;
        p
    }

    #[test]
    fn levels_follow_the_sends() {
        // 3 → 2 → 1 → master, 4 → master
        let p = proj(vec![ins(0, &[], true), ins(1, &[], true), ins(2, &[(1, 1.0)], false), ins(3, &[(2, 0.5)], false), ins(4, &[], true)]);
        let o = order(&p);
        assert_eq!(o.level, vec![0, 2, 1, 0, 0]);
        assert_eq!(o.depth, 2);
        assert_eq!(o.sends[3], vec![(2, 0.5)]);
    }

    #[test]
    fn a_loop_in_a_hand_edited_file_is_broken_not_followed() {
        let p = proj(vec![ins(0, &[], true), ins(1, &[(2, 1.0)], true), ins(2, &[(1, 1.0)], true)]);
        let o = order(&p);
        let edges: usize = o.sends.iter().map(|v| v.len()).sum();
        assert_eq!(edges, 1, "one send of the loop is dropped");
    }

    #[test]
    fn validate_refuses_loops_self_master_and_unknown_targets() {
        let p = proj(vec![ins(0, &[], true), ins(1, &[(2, 1.0)], true), ins(2, &[], true), ins(3, &[], true)]);
        assert!(validate(&p, &ins(2, &[(1, 1.0)], true)).unwrap_err().contains("loop"));
        assert!(validate(&p, &ins(2, &[(2, 1.0)], true)).is_err());
        assert!(validate(&p, &ins(2, &[(0, 1.0)], true)).is_err());
        assert!(validate(&p, &ins(2, &[(9, 1.0)], true)).is_err());
        assert!(validate(&p, &ins(2, &[(3, 3.0)], true)).is_err());
        assert!(validate(&p, &ins(2, &[(3, 1.0), (3, 0.5)], true)).is_err());
        assert!(validate(&p, &ins(0, &[], false)).is_err());
        assert!(validate(&p, &ins(3, &[(1, 0.5), (2, 1.0)], false)).is_ok());
        assert!(would_loop(&p, 2, 1) && !would_loop(&p, 1, 3) && would_loop(&p, 3, 3));
    }

    #[test]
    fn latency_lines_up_parallel_paths() {
        // 2 (lat 100) → bus 1 (lat 50) → master; 3 (lat 10) → master and → bus 1
        let p = proj(vec![ins(0, &[], true), ins(1, &[], true), ins(2, &[(1, 1.0)], false), ins(3, &[(1, 0.3)], true)]);
        let o = order(&p);
        let l = latency(&p, &o, &[0, 50, 100, 10]);
        assert_eq!(l.input, vec![0, 100, 0, 0], "the bus waits for its slowest source");
        assert_eq!(l.sends[2], vec![0]);
        assert_eq!(l.sends[3], vec![90], "3's send arrives with 2's");
        assert_eq!(l.at_master, 150);
        assert_eq!(l.master, vec![0, 0, 0, 140], "3 straight to the master meets the bus path");
    }

    #[test]
    fn without_routing_latency_is_the_old_flat_compensation() {
        let p = proj(vec![ins(0, &[], true), ins(1, &[], true), ins(2, &[], true)]);
        let l = latency(&p, &order(&p), &[7, 30, 5]);
        assert_eq!((l.at_master, l.master.clone()), (30, vec![0, 0, 25]));
    }

    #[test]
    fn solo_keeps_the_sources_and_the_way_to_the_master() {
        let mut p =
            proj(vec![ins(0, &[], true), ins(1, &[], true), ins(2, &[(1, 1.0)], false), ins(3, &[], true), ins(4, &[(2, 1.0)], false)]);
        p.inserts[2].solo = true;
        assert_eq!(audible(&p), vec![true, true, true, false, true]);
        p.inserts[4].muted = true;
        assert_eq!(audible(&p), vec![true, true, true, false, false]);
    }

    #[test]
    fn removing_a_bus_sends_its_only_feeders_to_the_master() {
        let mut p = proj(vec![ins(0, &[], true), ins(1, &[], true), ins(2, &[(1, 1.0)], false), ins(3, &[(1, 0.5), (2, 1.0)], false)]);
        p.inserts.retain(|m| m.id != 1);
        forget(&mut p, 1);
        assert!(p.inserts[1].master && p.inserts[1].sends.is_empty(), "2 fed only the bus: back to the master");
        assert!(!p.inserts[2].master && p.inserts[2].sends.len() == 1, "3 still feeds 2");
    }
}
