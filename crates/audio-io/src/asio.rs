//! Side-effect-free ASIO discovery. CPAL's ASIO device iterator starts each
//! driver to ask for capabilities, so it is only used to open a stream.

pub(crate) const FOCUSRITE_USB: &str = "Focusrite USB ASIO";

pub(crate) fn driver_names() -> Vec<String> {
    #[cfg(all(windows, feature = "asio"))]
    return asio_sys::Asio::new().driver_names();
    #[cfg(not(all(windows, feature = "asio")))]
    Vec::new()
}

/// `None` means Windows could not query the device tree. Never load a known
/// Focusrite USB driver when we cannot establish that its hardware is present.
pub(crate) fn focusrite_usb_present() -> Option<bool> {
    #[cfg(all(windows, feature = "asio"))]
    return windows::focusrite_usb_present().ok();
    #[cfg(not(all(windows, feature = "asio")))]
    None
}

#[cfg(all(windows, feature = "asio"))]
mod windows {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        CM_Get_DevNode_Status, CM_Get_Device_ID_ListW, CM_Get_Device_ID_List_SizeW, CM_Locate_DevNodeW, CM_GETIDLIST_DONOTGENERATE,
        CM_GETIDLIST_FILTER_PRESENT, CM_GETIDLIST_FILTER_SERVICE, CM_LOCATE_DEVNODE_NORMAL, CONFIGRET, CR_BUFFER_SMALL, CR_SUCCESS,
        DN_STARTED,
    };

    pub(super) fn focusrite_usb_present() -> Result<bool, CONFIGRET> {
        // Focusrite's USB INF binds FocusriteUsb to the physical USB devnode.
        // The software root and its audio endpoints can remain after unplugging.
        let service = windows_sys::w!("FocusriteUsb");
        let flags = CM_GETIDLIST_FILTER_SERVICE | CM_GETIDLIST_FILTER_PRESENT | CM_GETIDLIST_DONOTGENERATE;
        // Both calls take lengths in UTF-16 code units. The device list can
        // grow between calls, so retry a bounded number of times.
        for _ in 0..3 {
            let mut len = 0;
            // SAFETY: `service` is a static NUL-terminated UTF-16 string.
            let result = unsafe { CM_Get_Device_ID_List_SizeW(&mut len, service, flags) };
            if result != CR_SUCCESS {
                return Err(result);
            }
            let mut ids = vec![0u16; len as usize];
            // SAFETY: `ids` provides `len` writable UTF-16 code units.
            match unsafe { CM_Get_Device_ID_ListW(service, ids.as_mut_ptr(), len, flags) } {
                CR_BUFFER_SMALL => continue,
                CR_SUCCESS => {}
                result => return Err(result),
            }
            for id in ids.split_inclusive(|&c| c == 0).take_while(|id| id[0] != 0) {
                if !String::from_utf16_lossy(id).starts_with("USB\\VID_1235&PID_") || id.last() != Some(&0) {
                    continue;
                }
                let mut dev = 0;
                let (mut status, mut problem) = (0, 0);
                // SAFETY: the device ID is NUL-terminated and the out pointers are writable.
                if unsafe { CM_Locate_DevNodeW(&mut dev, id.as_ptr(), CM_LOCATE_DEVNODE_NORMAL) } == CR_SUCCESS
                    && unsafe { CM_Get_DevNode_Status(&mut status, &mut problem, dev, 0) } == CR_SUCCESS
                    && status & DN_STARTED != 0
                {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        Err(CR_BUFFER_SMALL)
    }
}
