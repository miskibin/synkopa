//! `IMessage` and `IAttributeList`: what `IHostApplication::createInstance` makes.
//!
//! A VST3 plugin whose processor and controller are separate objects talks between them
//! with messages over `IConnectionPoint` (which Synkopa already connects), but the message
//! objects come from the host. A host that cannot make them silently cuts that channel:
//! DPF-built plugins (Vitalium, the DISTRHO and ZAM ones) then assert and their interface
//! hears less from the processor. These are plain property bags.

use std::collections::HashMap;
use std::ffi::{c_void, CStr, CString};

use parking_lot::Mutex;
use vst3::Steinberg::Vst::{IAttributeList, IAttributeListTrait, IAttributeList_iid, IMessage, IMessageTrait, IMessage_iid, TChar};
use vst3::Steinberg::{int64, kInvalidArgument, kResultFalse, kResultOk, tresult, uint32, FIDString, TUID};
use vst3::{Class, ComWrapper};

/// Largest binary attribute accepted (a plugin streaming a sample through a message).
const MAX_BINARY: usize = 64 * 1024 * 1024;

#[derive(Clone)]
enum Value {
    Int(i64),
    Float(f64),
    /// UTF-16 with its NUL.
    String(Vec<u16>),
    Binary(Vec<u8>),
}

#[derive(Default)]
pub(crate) struct AttributeList {
    values: Mutex<HashMap<CString, Value>>,
}

impl Class for AttributeList {
    type Interfaces = (IAttributeList,);
}

/// # Safety
/// `id` is null or a NUL-terminated string.
unsafe fn key(id: *const std::ffi::c_char) -> Option<CString> {
    (!id.is_null()).then(|| CStr::from_ptr(id).to_owned())
}

impl AttributeList {
    fn set(&self, id: *const std::ffi::c_char, value: Value) -> tresult {
        // SAFETY: the plugin passes a C string (or null, refused).
        match unsafe { key(id) } {
            Some(k) => {
                self.values.lock().insert(k, value);
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    fn get(&self, id: *const std::ffi::c_char) -> Option<Value> {
        // SAFETY: as in `set`.
        unsafe { key(id) }.and_then(|k| self.values.lock().get(&k).cloned())
    }
}

impl IAttributeListTrait for AttributeList {
    unsafe fn setInt(&self, id: *const std::ffi::c_char, value: int64) -> tresult {
        self.set(id, Value::Int(value))
    }

    unsafe fn getInt(&self, id: *const std::ffi::c_char, value: *mut int64) -> tresult {
        match (self.get(id), value.is_null()) {
            (Some(Value::Int(v)), false) => {
                *value = v;
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setFloat(&self, id: *const std::ffi::c_char, value: f64) -> tresult {
        self.set(id, Value::Float(value))
    }

    unsafe fn getFloat(&self, id: *const std::ffi::c_char, value: *mut f64) -> tresult {
        match (self.get(id), value.is_null()) {
            (Some(Value::Float(v)), false) => {
                *value = v;
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setString(&self, id: *const std::ffi::c_char, string: *const TChar) -> tresult {
        if string.is_null() {
            return kInvalidArgument;
        }
        let mut s = Vec::new();
        let mut p = string;
        while *p != 0 && s.len() < 1 << 20 {
            s.push(*p);
            p = p.add(1);
        }
        s.push(0);
        self.set(id, Value::String(s))
    }

    unsafe fn getString(&self, id: *const std::ffi::c_char, string: *mut TChar, size_in_bytes: uint32) -> tresult {
        let Some(Value::String(s)) = self.get(id) else { return kResultFalse };
        let room = size_in_bytes as usize / 2;
        if string.is_null() || room == 0 {
            return kInvalidArgument;
        }
        let n = s.len().min(room);
        for (i, c) in s.iter().take(n).enumerate() {
            *string.add(i) = *c;
        }
        // always terminated, also when cut
        *string.add(n - 1) = 0;
        kResultOk
    }

    unsafe fn setBinary(&self, id: *const std::ffi::c_char, data: *const c_void, size_in_bytes: uint32) -> tresult {
        let n = size_in_bytes as usize;
        if (data.is_null() && n > 0) || n > MAX_BINARY {
            return kInvalidArgument;
        }
        let bytes = if n == 0 { Vec::new() } else { std::slice::from_raw_parts(data.cast::<u8>(), n).to_vec() };
        self.set(id, Value::Binary(bytes))
    }

    /// The pointer stays valid while the list lives and the attribute is not replaced (the
    /// VST3 contract).
    unsafe fn getBinary(&self, id: *const std::ffi::c_char, data: *mut *const c_void, size_in_bytes: *mut uint32) -> tresult {
        if data.is_null() || size_in_bytes.is_null() {
            return kInvalidArgument;
        }
        let Some(k) = key(id) else { return kInvalidArgument };
        let values = self.values.lock();
        match values.get(&k) {
            Some(Value::Binary(b)) => {
                *data = b.as_ptr().cast();
                *size_in_bytes = b.len() as uint32;
                kResultOk
            }
            _ => kResultFalse,
        }
    }
}

pub(crate) struct Message {
    id: Mutex<Option<CString>>,
    attributes: ComWrapper<AttributeList>,
}

impl Class for Message {
    type Interfaces = (IMessage,);
}

impl IMessageTrait for Message {
    unsafe fn getMessageID(&self) -> FIDString {
        self.id.lock().as_ref().map_or(std::ptr::null(), |s| s.as_ptr())
    }

    unsafe fn setMessageID(&self, id: FIDString) {
        *self.id.lock() = key(id);
    }

    /// Not add-ref'ed: owned by the message (VST3).
    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        self.attributes.as_com_ref::<IAttributeList>().map_or(std::ptr::null_mut(), |a| a.as_ptr())
    }
}

/// `IHostApplication::createInstance`: a new message or attribute list in `*obj` (one
/// reference, the caller's), or `kResultFalse` for anything else.
///
/// # Safety
/// `cid` / `iid` are null or live TUIDs; `obj` is null or writable.
pub(crate) unsafe fn create_instance(cid: *mut TUID, iid: *mut TUID, obj: *mut *mut c_void) -> tresult {
    if obj.is_null() {
        return kInvalidArgument;
    }
    *obj = std::ptr::null_mut();
    let (Some(cid), Some(iid)) = (cid.as_ref(), iid.as_ref()) else { return kInvalidArgument };
    if *cid == IMessage_iid && *iid == IMessage_iid {
        let m = ComWrapper::new(Message { id: Mutex::new(None), attributes: ComWrapper::new(AttributeList::default()) });
        if let Some(p) = m.to_com_ptr::<IMessage>() {
            *obj = p.into_raw().cast();
            return kResultOk;
        }
    } else if *cid == IAttributeList_iid && *iid == IAttributeList_iid {
        if let Some(p) = ComWrapper::new(AttributeList::default()).to_com_ptr::<IAttributeList>() {
            *obj = p.into_raw().cast();
            return kResultOk;
        }
    }
    kResultFalse
}

#[cfg(test)]
mod tests {
    use super::*;
    use vst3::ComPtr;

    #[test]
    fn the_host_makes_messages_whose_attributes_round_trip() {
        let mut raw: *mut c_void = std::ptr::null_mut();
        let (mut cid, mut iid) = (IMessage_iid, IMessage_iid);
        // SAFETY: live TUIDs and out-pointer; the object is adopted once below.
        let m = unsafe {
            assert_eq!(create_instance(&mut cid, &mut iid, &mut raw), kResultOk);
            ComPtr::<IMessage>::from_raw(raw.cast()).unwrap()
        };
        // SAFETY: plain values and live buffers throughout.
        unsafe {
            m.setMessageID(c"parameter-edit".as_ptr());
            assert_eq!(CStr::from_ptr(m.getMessageID()), c"parameter-edit");
            let a = vst3::ComRef::<IAttributeList>::from_raw(m.getAttributes()).unwrap();
            assert_eq!(a.setInt(c"index".as_ptr(), 7), kResultOk);
            assert_eq!(a.setFloat(c"value".as_ptr(), 0.25), kResultOk);
            let hi: Vec<TChar> = "hé".encode_utf16().chain([0]).collect();
            assert_eq!(a.setString(c"name".as_ptr(), hi.as_ptr()), kResultOk);
            assert_eq!(a.setBinary(c"blob".as_ptr(), b"abc".as_ptr().cast(), 3), kResultOk);
            let (mut i, mut f) = (0, 0.0);
            assert_eq!(a.getInt(c"index".as_ptr(), &mut i), kResultOk);
            assert_eq!(a.getFloat(c"value".as_ptr(), &mut f), kResultOk);
            assert_eq!((i, f), (7, 0.25));
            assert_eq!(a.getInt(c"value".as_ptr(), &mut i), kResultFalse, "the wrong type is not converted");
            let mut buf = [0 as TChar; 8];
            assert_eq!(a.getString(c"name".as_ptr(), buf.as_mut_ptr(), 16), kResultOk);
            assert_eq!(&buf[..3], &hi[..]);
            let mut short = [7 as TChar; 2];
            assert_eq!(a.getString(c"name".as_ptr(), short.as_mut_ptr(), 4), kResultOk);
            assert_eq!(short[1], 0, "a cut string is still terminated");
            let (mut p, mut n): (*const c_void, u32) = (std::ptr::null(), 0);
            assert_eq!(a.getBinary(c"blob".as_ptr(), &mut p, &mut n), kResultOk);
            assert_eq!(std::slice::from_raw_parts(p.cast::<u8>(), n as usize), b"abc");
            assert_eq!(a.getInt(c"missing".as_ptr(), &mut i), kResultFalse);
        }
        let (mut cid, mut iid) = ([1; 16], IMessage_iid);
        // SAFETY: as above.
        assert_eq!(unsafe { create_instance(&mut cid, &mut iid, &mut raw) }, kResultFalse);
        assert!(raw.is_null());
    }
}
