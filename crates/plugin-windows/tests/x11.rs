//! The X11 backend against a real X server (CI: Xvfb; set `WAVE_REQUIRE_X11=1` to fail
//! instead of skipping without one): the window, its embed child, a "plugin" window that
//! appears in it, the header's gear and close button clicked through XTEST, and the window
//! manager's close message.
#![cfg(target_os = "linux")]

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use wave_plugin_windows::event_loop::{self, PollLoop};
use wave_plugin_windows::{header, Label, Service, WindowEvent, WindowOptions, WindowSize};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, WindowClass};
use x11rb::protocol::xtest::ConnectionExt as _;

fn display() -> bool {
    let required = std::env::var_os("WAVE_REQUIRE_X11").is_some_and(|v| v == "1");
    match wave_plugin_windows::available() {
        Ok(()) => true,
        Err(e) => {
            assert!(!required, "WAVE_REQUIRE_X11=1 but: {e}");
            eprintln!("skipping: {e}");
            false
        }
    }
}

/// Turn the loop until `done` or 3 s.
fn until(lp: &PollLoop, mut done: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(3);
    while Instant::now() < end {
        lp.run_once(Duration::from_millis(5));
        if done() {
            return true;
        }
    }
    false
}

#[test]
fn a_plugin_window_hosts_a_child_and_its_header_buttons_work() {
    if !display() {
        return;
    }
    let lp = PollLoop::new();
    event_loop::install(lp.clone());
    let service = Service::new();
    let asked = Rc::new(Cell::new(0_u64));
    let a = Rc::clone(&asked);
    service.set_settings_handler(Rc::new(move |tag| a.set(tag)));
    let label = Label { title: "Test plugin".into(), subtitle: "VST3 effect".into(), tag: 4242 };
    let window = service.open_window(&WindowOptions::labelled(&label, WindowSize::new(320, 200))).expect("window");
    window.show().unwrap();
    let embed = window.native_handle() as u32;
    assert_ne!(embed, 0);
    assert!(until(&lp, || window.client_size().ok() == Some(WindowSize::new(320, 200))), "the plugin area is the size asked");
    assert!(!window.has_child_window().unwrap());
    assert_eq!(window.embedded_size().unwrap(), None);

    // a "plugin" on its own connection embeds a window
    let (other, screen) = x11rb::connect(None).unwrap();
    let root = other.setup().roots[screen].root;
    let child = other.generate_id().unwrap();
    other
        .create_window(
            0,
            child,
            embed,
            0,
            0,
            300,
            180,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().event_mask(EventMask::BUTTON_PRESS),
        )
        .unwrap();
    other.map_window(child).unwrap();
    other.flush().unwrap();
    assert!(until(&lp, || window.has_child_window().unwrap_or(false)));
    assert_eq!(window.embedded_size().unwrap(), Some(WindowSize::new(300, 180)));

    // resizing keeps the header above the plugin area
    window.set_client_size(WindowSize::new(400, 260)).unwrap();
    assert!(until(&lp, || window.client_size().ok() == Some(WindowSize::new(400, 260))));

    // the header's gear and close button, clicked like a user (XTEST)
    let at = other.translate_coordinates(embed, root, 0, 0).unwrap().reply().unwrap();
    let (x0, y0) = (at.dst_x, at.dst_y - header::HEIGHT as i16);
    let w = 400_i16;
    let click = |x: i16, y: i16| {
        other.xtest_fake_input(6, 0, 0, root, x, y, 0).unwrap(); // motion
        other.xtest_fake_input(4, 1, 0, root, 0, 0, 0).unwrap(); // press
        other.xtest_fake_input(5, 1, 0, root, 0, 0, 0).unwrap(); // release
        other.flush().unwrap();
    };
    click(x0 + w - 45, y0 + 14);
    assert!(until(&lp, || asked.get() == 4242), "the gear reports the window's tag");
    window.drain_events();
    click(x0 + w - 15, y0 + 14);
    assert!(until(&lp, || window.drain_events().contains(&WindowEvent::Closed)), "the close button asks the owner to close");

    // a press in the plugin's area is the plugin's: no implicit grab by WAVE's frame
    click(x0 + 50, y0 + header::HEIGHT as i16 + 50);
    let got = Instant::now() + Duration::from_secs(2);
    let mut pressed = false;
    while Instant::now() < got && !pressed {
        lp.run_once(Duration::from_millis(5));
        while let Ok(Some(ev)) = other.poll_for_event() {
            pressed |= matches!(ev, x11rb::protocol::Event::ButtonPress(e) if e.event == child);
        }
    }
    assert!(pressed, "the plugin's window gets its clicks");

    // the window manager's close message
    let a = other.intern_atom(false, b"WM_PROTOCOLS").unwrap().reply().unwrap().atom;
    let d = other.intern_atom(false, b"WM_DELETE_WINDOW").unwrap().reply().unwrap().atom;
    let frame = other.query_tree(embed).unwrap().reply().unwrap().parent;
    other.send_event(false, frame, EventMask::NO_EVENT, ClientMessageEvent::new(32, frame, a, [d, 0, 0, 0, 0])).unwrap();
    other.flush().unwrap();
    assert!(until(&lp, || window.drain_events().contains(&WindowEvent::Closed)));

    window.close().unwrap();
    assert!(!window.is_open());
    assert!(until(&lp, || lp.is_empty()), "the connection's loop sources go with the last window");
    event_loop::uninstall();
}

#[test]
fn without_an_installed_loop_no_window_opens() {
    if !display() {
        return;
    }
    event_loop::uninstall();
    let err = Service::new().open_window(&WindowOptions::new("x", WindowSize::new(200, 100))).unwrap_err();
    assert!(matches!(err, wave_plugin_windows::WindowError::NoEventLoop), "{err:?}");
}
