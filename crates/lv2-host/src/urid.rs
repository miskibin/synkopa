//! `urid:map` / `urid:unmap`: one process-wide URI ⇄ integer table shared by every
//! instance (URIDs must agree between a plugin, its state and the host).
//!
//! Plugins map their URIs at instantiation; a lookup of a known URI takes only the read
//! lock, so a plugin that maps in `run()` against the specification's advice does not
//! wait behind another reader.

use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::OnceLock;

use parking_lot::RwLock;

use crate::sys::{LV2_URID_Map, LV2_URID_Unmap, LV2_URID};

#[derive(Default)]
struct Table {
    ids: HashMap<CString, LV2_URID>,
    /// `uris[id - 1]`; a `CString`'s bytes never move, so `unmap` can hand them out.
    uris: Vec<CString>,
}

fn table() -> &'static RwLock<Table> {
    static TABLE: OnceLock<RwLock<Table>> = OnceLock::new();
    TABLE.get_or_init(Default::default)
}

/// The URID of `uri` (from 1; 0 is never a valid URID).
pub fn map(uri: &CStr) -> LV2_URID {
    if let Some(id) = table().read().ids.get(uri) {
        return *id;
    }
    let mut t = table().write();
    if let Some(id) = t.ids.get(uri) {
        return *id;
    }
    let id = t.uris.len() as LV2_URID + 1;
    t.uris.push(uri.to_owned());
    t.ids.insert(uri.to_owned(), id);
    id
}

/// [`map`] of a NUL-terminated constant from [`crate::sys`].
pub fn map_str(uri_nul: &str) -> LV2_URID {
    map(CStr::from_bytes_with_nul(uri_nul.as_bytes()).expect("a NUL-terminated URI constant"))
}

/// The URI of `id`, or `None` for one never handed out.
pub fn unmap(id: LV2_URID) -> Option<&'static CStr> {
    let t = table().read();
    let s = t.uris.get((id as usize).checked_sub(1)?)?;
    // SAFETY: entries are never removed or modified and a CString's buffer does not
    // move when the Vec grows, so the bytes live (unchanged) as long as the process.
    Some(unsafe { CStr::from_ptr(s.as_ptr()) })
}

unsafe extern "C" fn map_cb(_handle: *mut c_void, uri: *const c_char) -> LV2_URID {
    if uri.is_null() {
        return 0;
    }
    // SAFETY: LV2 passes a NUL-terminated string that lives for the call.
    map(unsafe { CStr::from_ptr(uri) })
}

unsafe extern "C" fn unmap_cb(_handle: *mut c_void, id: LV2_URID) -> *const c_char {
    unmap(id).map_or(std::ptr::null(), CStr::as_ptr)
}

/// A `Sync` home for a feature struct that holds a (null) raw handle.
pub(crate) struct Static<T>(pub T);
// SAFETY: the structs wrapped here are never written after construction and hold only
// function pointers and a null handle.
unsafe impl<T> Sync for Static<T> {}

static MAP: Static<LV2_URID_Map> = Static(LV2_URID_Map { handle: std::ptr::null_mut(), map: map_cb });
static UNMAP: Static<LV2_URID_Unmap> = Static(LV2_URID_Unmap { handle: std::ptr::null_mut(), unmap: unmap_cb });

/// The `urid:map` feature data (lilv's state functions take it too).
pub fn map_feature() -> *mut LV2_URID_Map {
    std::ptr::addr_of!(MAP.0).cast_mut()
}

/// The `urid:unmap` feature data.
pub fn unmap_feature() -> *mut LV2_URID_Unmap {
    std::ptr::addr_of!(UNMAP.0).cast_mut()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_is_stable_and_reversible_through_the_c_callbacks() {
        let a = map(c"urn:wave:test:a");
        let b = map(c"urn:wave:test:b");
        assert_ne!(a, 0);
        assert_ne!(a, b);
        assert_eq!(map(c"urn:wave:test:a"), a, "referentially transparent");
        assert_eq!(map_str("urn:wave:test:b\0"), b);
        assert_eq!(unmap(a).unwrap(), c"urn:wave:test:a");
        assert!(unmap(0).is_none());
        assert!(unmap(u32::MAX).is_none());
        // SAFETY: the feature structs are valid for the process; strings outlive the calls.
        unsafe {
            let m = &*map_feature();
            assert_eq!((m.map)(m.handle, c"urn:wave:test:a".as_ptr()), a);
            assert_eq!((m.map)(m.handle, std::ptr::null()), 0);
            let u = &*unmap_feature();
            assert_eq!(CStr::from_ptr((u.unmap)(u.handle, b)), c"urn:wave:test:b");
            assert!((u.unmap)(u.handle, 0).is_null());
        }
    }
}
