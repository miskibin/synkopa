//! The X11 backend (Linux; XWayland under Wayland).
//!
//! Synkopa opens its **own** connection to the X server (`x11rb`, pure Rust: no libX11 at
//! build or run time, and no process-wide Xlib error handler to fight the plugins over),
//! creates the windows there, and reads the connection from the UI thread's installed event
//! loop ([`crate::event_loop`]): a watch on its socket plus a 16 ms tick, which also runs the
//! owners' tick handlers like `WM_TIMER` does on Windows. The plugin draws through its own
//! connection; all it gets from us is a window id to embed into.
//!
//! # Layout
//! A top-level *frame* without WM decorations (`_MOTIF_WM_HINTS`), holding:
//! - a *header* child (Synkopa's title bar). Pressing on its free part hands the move to the
//!   window manager (`_NET_WM_MOVERESIZE`, so snapping and multi-monitor moves behave
//!   natively);
//! - an *embed* child below it, the plugin's parent (VST3 `X11EmbedWindowID`, LV2
//!   `ui:parent`);
//! - for a resizable plugin, thin input-only *border* strips round the plugin that resize it
//!   the same way.
//!
//! Only the header and the borders ask for pointer events; the frame and the embed child
//! never do. A pointer press is delivered to the deepest window that asked for it, and a
//! press taken by an ancestor starts that client's implicit pointer grab: a frame that
//! listened for presses would steal every drag from a plugin whose window does not select
//! core button events (toolkits that read XInput2).
//!
//! # XEmbed
//! VST3 says the parent "supports XEmbed". What plugins rely on in practice, and what this
//! does: tell an embedded client it is embedded (`XEMBED_EMBEDDED_NOTIFY`), map it when its
//! `_XEMBED_INFO` asks to be mapped, tell it when the window is (de)activated, and give it
//! the keyboard focus when the window gets it, so text fields in plugin UIs can be typed in.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use x11rb::connection::{Connection, RequestConnection};
use x11rb::properties::WmSizeHints;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt as _, CreateGCAux, CreateWindowAux, EventMask, ImageFormat, ImageOrder,
    InputFocus, PropMode, StackMode, Window as XWindow, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::CURRENT_TIME;

use crate::event_loop::{self, SourceKey};
use crate::header::{self, Canvas, Part};
use crate::service::{WindowError, WindowEvent, WindowId, WindowOptions, WindowSize, WindowState, PUMP_INTERVAL_MS};

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        WM_PROTOCOLS,
        WM_DELETE_WINDOW,
        WM_TRANSIENT_FOR,
        _NET_WM_NAME,
        _NET_WM_PID,
        _NET_WM_MOVERESIZE,
        _NET_ACTIVE_WINDOW,
        _NET_WM_STATE,
        _NET_WM_STATE_ABOVE,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_NORMAL,
        _MOTIF_WM_HINTS,
        _XEMBED,
        _XEMBED_INFO,
        RESOURCE_MANAGER,
        UTF8_STRING,
    }
}

/// Header background, also the frame's (its resize border).
const FRAME_BG: u32 = 0x171b21;
/// Border round a resizable plugin, at 100 %.
const BORDER: f32 = 4.0;
/// Pixels at the top of a resizable window's header that resize instead of move.
const TOP_RESIZE: i32 = 4;

// _NET_WM_MOVERESIZE directions
const SIZE_TOPLEFT: u32 = 0;
const SIZE_TOP: u32 = 1;
const SIZE_TOPRIGHT: u32 = 2;
const SIZE_RIGHT: u32 = 3;
const SIZE_BOTTOMRIGHT: u32 = 4;
const SIZE_BOTTOM: u32 = 5;
const SIZE_BOTTOMLEFT: u32 = 6;
const SIZE_LEFT: u32 = 7;
const MOVE: u32 = 8;

// XEmbed messages
const XEMBED_EMBEDDED_NOTIFY: u32 = 0;
const XEMBED_WINDOW_ACTIVATE: u32 = 1;
const XEMBED_WINDOW_DEACTIVATE: u32 = 2;
const XEMBED_FOCUS_IN: u32 = 4;
const XEMBED_FOCUS_OUT: u32 = 5;
const XEMBED_MAPPED: u32 = 1;

struct Win {
    frame: XWindow,
    header: XWindow,
    embed: XWindow,
    /// Left, right, bottom resize strips (resizable plugins only).
    borders: Vec<XWindow>,
    state: WindowState,
    /// Frame size, device pixels.
    size: Cell<(u32, u32)>,
    border: Cell<u32>,
    /// The plugin's own window inside `embed`, once it appeared.
    client: Cell<XWindow>,
}

struct Conn {
    conn: RustConnection,
    root: XWindow,
    depth: u8,
    atoms: Atoms,
    dpi: Cell<u32>,
    /// `PutImage` of 32-bit little-endian pixels is right for this server.
    image_ok: bool,
    gc: u32,
    windows: RefCell<HashMap<XWindow, Rc<Win>>>,
    sources: RefCell<Vec<SourceKey>>,
}

thread_local! {
    static CONN: RefCell<Option<Rc<Conn>>> = const { RefCell::new(None) };
}

fn err(what: &'static str, e: impl std::fmt::Display) -> WindowError {
    WindowError::X11 { id: WindowId::none(), what, message: e.to_string() }
}

/// A connection to the X server named by `DISPLAY`. A refused or reset handshake is retried
/// a few times: a server that is starting (or a socket the system is handing over) answers a
/// moment later, and one failure must not cost the session its plugin windows.
fn connect() -> Result<(RustConnection, usize), String> {
    if std::env::var_os("DISPLAY").is_none() {
        return Err("DISPLAY is not set (a Wayland session without XWayland?)".to_owned());
    }
    let mut last = String::new();
    for attempt in 0..4 {
        match x11rb::connect(None) {
            Ok(c) => return Ok(c),
            Err(e) => last = e.to_string(),
        }
        std::thread::sleep(Duration::from_millis(25 << attempt));
    }
    Err(last)
}

/// Whether an X server can be reached. A success is remembered for the process; a failure
/// is asked again next time (the server may come up, XWayland may start on demand).
pub(crate) fn available() -> Result<(), WindowError> {
    static OK: OnceLock<()> = OnceLock::new();
    if OK.get().is_some() {
        return Ok(());
    }
    connect()
        .map(|_| {
            let _ = OK.set(());
        })
        .map_err(WindowError::NoDisplay)
}

/// `Xft.dpi` from the root window's resource database (what GTK, Qt and most plugins read
/// for their scale), else 96.
fn read_dpi(conn: &RustConnection, root: XWindow, atoms: &Atoms) -> u32 {
    let value = conn
        .get_property(false, root, atoms.RESOURCE_MANAGER, AtomEnum::STRING, 0, 1 << 16)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| r.value)
        .unwrap_or_default();
    parse_xft_dpi(&String::from_utf8_lossy(&value)).unwrap_or(96)
}

fn parse_xft_dpi(resources: &str) -> Option<u32> {
    resources
        .lines()
        .find_map(|l| l.strip_prefix("Xft.dpi:"))
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|d| (48.0..=480.0).contains(d))
        .map(|d| d.round() as u32)
}

fn scale_of(dpi: u32) -> f32 {
    dpi.max(1) as f32 / 96.0
}

fn connection() -> Result<Rc<Conn>, WindowError> {
    if let Some(c) = CONN.with(|c| c.borrow().clone()) {
        return Ok(c);
    }
    let lp = event_loop::current().ok_or(WindowError::NoEventLoop)?;
    let (conn, screen) = connect().map_err(WindowError::NoDisplay)?;
    let setup = conn.setup();
    let s = &setup.roots[screen];
    let (root, depth) = (s.root, s.root_depth);
    // our header is 32-bit 0x00RRGGBB pixels, least significant byte first
    let image_ok = setup.image_byte_order == ImageOrder::LSB_FIRST
        && depth >= 24
        && setup.pixmap_formats.iter().any(|f| f.depth == depth && f.bits_per_pixel == 32);
    let atoms = Atoms::new(&conn).map_err(|e| err("InternAtom", e))?.reply().map_err(|e| err("InternAtom", e))?;
    let gc = conn.generate_id().map_err(|e| err("generate_id", e))?;
    conn.create_gc(gc, root, &CreateGCAux::new()).map_err(|e| err("CreateGC", e))?;
    // follow the desktop's scale while windows are open
    let _ = conn
        .change_window_attributes(root, &x11rb::protocol::xproto::ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE));
    let dpi = read_dpi(&conn, root, &atoms);
    let c = Rc::new(Conn {
        conn,
        root,
        depth,
        atoms,
        dpi: Cell::new(dpi),
        image_ok,
        gc,
        windows: RefCell::new(HashMap::new()),
        sources: RefCell::new(Vec::new()),
    });
    let fd = c.conn.stream().as_raw_fd();
    let weak = Rc::downgrade(&c);
    let on_fd = lp.watch_fd(
        fd,
        Box::new(move || {
            if let Some(c) = weak.upgrade() {
                pump(&c);
            }
        }),
    );
    let weak = Rc::downgrade(&c);
    let on_tick = lp.add_timer(
        Duration::from_millis(u64::from(PUMP_INTERVAL_MS)),
        Box::new(move || {
            if let Some(c) = weak.upgrade() {
                pump(&c);
                tick(&c);
            }
        }),
    );
    c.sources.borrow_mut().extend(on_fd.into_iter().chain([on_tick]));
    log::info!("plugin windows: X11 display open (Xft.dpi {dpi}, depth {depth})");
    CONN.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&c)));
    Ok(c)
}

/// Drop the connection once its last window is gone (and stop its loop sources).
fn release_if_idle(c: &Rc<Conn>) {
    if !c.windows.borrow().is_empty() {
        return;
    }
    if let Some(lp) = event_loop::current() {
        for key in c.sources.borrow_mut().drain(..) {
            lp.remove(key);
        }
    }
    let _ = c.conn.flush();
    CONN.with(|slot| *slot.borrow_mut() = None);
}

fn win(c: &Conn, frame: isize) -> Option<Rc<Win>> {
    c.windows.borrow().get(&(frame as XWindow)).cloned()
}

fn with_win<R>(frame: isize, f: impl FnOnce(&Rc<Conn>, &Rc<Win>) -> R) -> Option<R> {
    let c = CONN.with(|c| c.borrow().clone())?;
    let w = win(&c, frame)?;
    Some(f(&c, &w))
}

fn header_height(c: &Conn) -> u32 {
    header::height(scale_of(c.dpi.get()))
}

fn border_for(c: &Conn, resizable: bool) -> u32 {
    if resizable {
        (BORDER * scale_of(c.dpi.get())).round() as u32
    } else {
        0
    }
}

/// Frame size for a plugin area of `size`.
fn frame_size(c: &Conn, size: WindowSize, border: u32) -> (u32, u32) {
    (size.width + 2 * border, size.height + header_height(c) + border)
}

fn set_size_hints(c: &Conn, w: &Win, plugin: Option<WindowSize>) {
    let border = w.border.get();
    let (min_w, min_h) = frame_size(c, w.state.min_size, border);
    let mut hints = WmSizeHints::new();
    match plugin.filter(|_| !w.state.resizable) {
        Some(size) => {
            let fixed = frame_size(c, size, border);
            let fixed = (fixed.0 as i32, fixed.1 as i32);
            hints.min_size = Some(fixed);
            hints.max_size = Some(fixed);
        }
        None => hints.min_size = Some((min_w as i32, min_h as i32)),
    }
    let _ = hints.set_normal_hints(&c.conn, w.frame);
}

pub(crate) fn create_window(
    options: &WindowOptions,
    size: WindowSize,
    events: Sender<WindowEvent>,
    ticks: Arc<AtomicU64>,
    owner: isize,
) -> Result<isize, WindowError> {
    let c = connection()?;
    let made = make_window(&c, options, size, events, ticks, owner);
    if made.is_err() {
        release_if_idle(&c);
    }
    made
}

fn make_window(
    c: &Rc<Conn>,
    options: &WindowOptions,
    size: WindowSize,
    events: Sender<WindowEvent>,
    ticks: Arc<AtomicU64>,
    owner: isize,
) -> Result<isize, WindowError> {
    let x = &c.conn;
    let a = &c.atoms;
    let border = border_for(c, options.resizable);
    let (fw, fh) = frame_size(c, size, border);
    let id = || x.generate_id().map_err(|e| err("generate_id", e));
    let (frame, header, embed) = (id()?, id()?, id()?);
    let hh = header_height(c);
    x.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        frame,
        c.root,
        0,
        0,
        fw as u16,
        fh as u16,
        0,
        WindowClass::INPUT_OUTPUT,
        x11rb::COPY_FROM_PARENT,
        // no pointer events here: see the module docs
        &CreateWindowAux::new().background_pixel(FRAME_BG).event_mask(EventMask::STRUCTURE_NOTIFY | EventMask::FOCUS_CHANGE),
    )
    .map_err(|e| err("CreateWindow", e))?;
    x.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        header,
        frame,
        0,
        0,
        fw as u16,
        hh as u16,
        0,
        WindowClass::INPUT_OUTPUT,
        x11rb::COPY_FROM_PARENT,
        &CreateWindowAux::new().background_pixel(FRAME_BG).event_mask(
            EventMask::EXPOSURE | EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION | EventMask::LEAVE_WINDOW,
        ),
    )
    .map_err(|e| err("CreateWindow", e))?;
    x.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        embed,
        frame,
        border as i16,
        hh as i16,
        size.width as u16,
        size.height as u16,
        0,
        WindowClass::INPUT_OUTPUT,
        x11rb::COPY_FROM_PARENT,
        // a plugin's window appearing, reparented in, mapping itself
        &CreateWindowAux::new().background_pixel(0).event_mask(EventMask::SUBSTRUCTURE_NOTIFY),
    )
    .map_err(|e| err("CreateWindow", e))?;
    let mut borders = Vec::new();
    if border > 0 {
        for _ in 0..3 {
            let strip = id()?;
            x.create_window(
                0,
                strip,
                frame,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                x11rb::COPY_FROM_PARENT,
                &CreateWindowAux::new().event_mask(EventMask::BUTTON_PRESS),
            )
            .map_err(|e| err("CreateWindow", e))?;
            borders.push(strip);
        }
    }
    let title = options.title.as_bytes();
    let _ = x.change_property8(PropMode::REPLACE, frame, AtomEnum::WM_NAME, AtomEnum::STRING, title);
    let _ = x.change_property8(PropMode::REPLACE, frame, a._NET_WM_NAME, a.UTF8_STRING, title);
    let _ = x.change_property8(PropMode::REPLACE, frame, AtomEnum::WM_CLASS, AtomEnum::STRING, b"wave-plugin\0WAVE\0");
    let _ = x.change_property32(PropMode::REPLACE, frame, a.WM_PROTOCOLS, AtomEnum::ATOM, &[a.WM_DELETE_WINDOW]);
    let _ = x.change_property32(PropMode::REPLACE, frame, a._NET_WM_PID, AtomEnum::CARDINAL, &[std::process::id()]);
    let _ = x.change_property32(PropMode::REPLACE, frame, a._NET_WM_WINDOW_TYPE, AtomEnum::ATOM, &[a._NET_WM_WINDOW_TYPE_NORMAL]);
    // no WM decorations: Synkopa's header is the title bar (flags = decorations, decorations = none)
    let _ = x.change_property32(PropMode::REPLACE, frame, a._MOTIF_WM_HINTS, a._MOTIF_WM_HINTS, &[2, 0, 0, 0, 0]);
    if owner > 0 {
        // above the app window and minimised with it (an X11 session's GTK window only)
        let _ = x.change_property32(PropMode::REPLACE, frame, a.WM_TRANSIENT_FOR, AtomEnum::WINDOW, &[owner as u32]);
    }
    x.map_subwindows(frame).map_err(|e| err("MapWindow", e))?;
    let w = Rc::new(Win {
        frame,
        header,
        embed,
        borders,
        state: WindowState::new(options, events, ticks),
        size: Cell::new((fw, fh)),
        border: Cell::new(border),
        client: Cell::new(x11rb::NONE),
    });
    w.state.embed.set(embed as isize);
    set_size_hints(c, &w, Some(size));
    layout(c, &w);
    c.windows.borrow_mut().insert(frame, w);
    x.flush().map_err(|e| err("flush", e))?;
    Ok(frame as isize)
}

pub(crate) fn embed_handle(frame: isize) -> isize {
    with_win(frame, |_, w| w.embed as isize).unwrap_or(0)
}

pub(crate) fn destroy_window(frame: isize) {
    let Some(c) = CONN.with(|c| c.borrow().clone()) else { return };
    let removed = c.windows.borrow_mut().remove(&(frame as XWindow));
    if removed.is_some() {
        let _ = c.conn.destroy_window(frame as XWindow);
        let _ = c.conn.flush();
    }
    release_if_idle(&c);
}

pub(crate) fn show_window(frame: isize) {
    with_win(frame, |c, w| {
        let x = &c.conn;
        let _ = x.map_window(w.frame);
        let _ = x.configure_window(w.frame, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
        // ask the window manager to bring it forward and give it focus (source: application)
        let msg = ClientMessageEvent::new(32, w.frame, c.atoms._NET_ACTIVE_WINDOW, [1, CURRENT_TIME, 0, 0, 0]);
        let _ = x.send_event(false, c.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, msg);
        let _ = x.flush();
    });
}

/// Pin (`pinned`) or unpin the window on top of the app's: the window manager's "above"
/// state, changed the EWMH way (a client message to the root; a window manager that
/// refuses it simply leaves the stacking as it was). The header's pin follows.
pub(crate) fn set_always_on_top(frame: isize, pinned: bool) {
    with_win(frame, |c, w| {
        w.state.set_pinned_state(pinned);
        send_above(c, w.frame, pinned);
        paint(c, w);
    });
}

/// The click's half: apply what the header's pin now says.
fn apply_pin(c: &Rc<Conn>, w: &Rc<Win>) {
    let pinned = w.state.pinned();
    send_above(c, w.frame, pinned);
    paint(c, w);
}

/// Ask the window manager to keep (`set`) or stop keeping the frame above other windows
/// (`_NET_WM_STATE`, source: application, `_NET_WM_STATE_ABOVE`).
fn send_above(c: &Conn, frame: XWindow, set: bool) {
    let action = u32::from(set); // 0 = remove, 1 = add
    let msg = ClientMessageEvent::new(32, frame, c.atoms._NET_WM_STATE, [action, c.atoms._NET_WM_STATE_ABOVE, 0, 0, 0]);
    let _ = c.conn.send_event(false, c.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, msg);
    let _ = c.conn.flush();
}

pub(crate) fn request_close(frame: isize) {
    let Some(state_ok) = with_win(frame, |_, w| Rc::clone(w)) else { return };
    state_ok.state.post(WindowEvent::Closed);
    state_ok.state.run_tick();
}

pub(crate) fn set_tick_handler(frame: isize, handler: Option<Rc<dyn Fn()>>) {
    with_win(frame, |_, w| *w.state.tick.borrow_mut() = handler);
}

pub(crate) fn dpi(_frame: isize) -> u32 {
    CONN.with(|c| c.borrow().as_ref().map_or(96, |c| c.dpi.get()))
}

pub(crate) fn has_child_window(frame: isize) -> Result<bool, WindowError> {
    with_win(frame, |c, w| {
        c.conn
            .query_tree(w.embed)
            .map_err(|e| err("QueryTree", e))?
            .reply()
            .map(|r| !r.children.is_empty())
            .map_err(|e| err("QueryTree", e))
    })
    .unwrap_or(Ok(false))
}

pub(crate) fn embedded_size(frame: isize) -> Result<Option<WindowSize>, WindowError> {
    with_win(frame, |c, w| {
        let tree = c.conn.query_tree(w.embed).map_err(|e| err("QueryTree", e))?.reply().map_err(|e| err("QueryTree", e))?;
        let Some(&child) = tree.children.first() else { return Ok(None) };
        let g = c.conn.get_geometry(child).map_err(|e| err("GetGeometry", e))?.reply().map_err(|e| err("GetGeometry", e))?;
        Ok(Some(WindowSize::new(u32::from(g.width), u32::from(g.height))))
    })
    .unwrap_or(Ok(None))
}

pub(crate) fn client_size(id: WindowId, frame: isize) -> Result<WindowSize, WindowError> {
    with_win(frame, |c, w| {
        let g = c
            .conn
            .get_geometry(w.embed)
            .map_err(|e| WindowError::X11 { id, what: "GetGeometry", message: e.to_string() })?
            .reply()
            .map_err(|e| WindowError::X11 { id, what: "GetGeometry", message: e.to_string() })?;
        Ok(WindowSize::new(u32::from(g.width), u32::from(g.height)))
    })
    .unwrap_or(Err(WindowError::Closed(id)))
}

pub(crate) fn set_client_size(id: WindowId, frame: isize, size: WindowSize) -> Result<(), WindowError> {
    with_win(frame, |c, w| {
        let (fw, fh) = frame_size(c, size, w.border.get());
        set_size_hints(c, w, Some(size));
        c.conn.configure_window(w.frame, &ConfigureWindowAux::new().width(fw).height(fh)).map_err(|e| WindowError::X11 {
            id,
            what: "ConfigureWindow",
            message: e.to_string(),
        })?;
        w.size.set((fw, fh));
        layout(c, w);
        c.conn.flush().map_err(|e| WindowError::X11 { id, what: "flush", message: e.to_string() })
    })
    .unwrap_or(Err(WindowError::Closed(id)))
}

/// Place the embed child for the frame's current size and tell the owner the plugin area.
fn layout(c: &Conn, w: &Win) {
    let (fw, fh) = w.size.get();
    let (hh, b) = (header_height(c), w.border.get());
    let (pw, ph) = (fw.saturating_sub(2 * b).max(1), fh.saturating_sub(hh + b).max(1));
    let _ = c.conn.configure_window(w.header, &ConfigureWindowAux::new().width(fw.max(1)).height(hh));
    let _ = c.conn.configure_window(w.embed, &ConfigureWindowAux::new().x(b as i32).y(hh as i32).width(pw).height(ph));
    if let [left, right, bottom] = w.borders[..] {
        let side = fh.saturating_sub(hh).max(1);
        let edge = |x: u32, y: u32, width: u32, height: u32| {
            ConfigureWindowAux::new().x(x as i32).y(y as i32).width(width.max(1)).height(height.max(1))
        };
        let _ = c.conn.configure_window(left, &edge(0, hh, b, side));
        let _ = c.conn.configure_window(right, &edge(fw.saturating_sub(b), hh, b, side));
        let _ = c.conn.configure_window(bottom, &edge(0, fh.saturating_sub(b), fw, b));
    }
    w.state.post(WindowEvent::Resized(WindowSize::new(pw, ph)));
    paint(c, w);
}

/// Draw the header (split into requests the server accepts).
fn paint(c: &Conn, w: &Win) {
    let (fw, _) = w.size.get();
    let scale = scale_of(c.dpi.get());
    let h = header::height(scale);
    if fw == 0 || !c.image_ok {
        return;
    }
    let mut px = vec![0_u32; (fw * h) as usize];
    w.state.header.borrow().render(&mut Canvas { px: &mut px, width: fw, height: h }, scale);
    let bytes: Vec<u8> = px.iter().flat_map(|p| p.to_le_bytes()).collect();
    let row = fw as usize * 4;
    let rows_per = ((c.conn.maximum_request_bytes().saturating_sub(64)) / row).clamp(1, h as usize);
    let mut y = 0;
    while y < h as usize {
        let n = rows_per.min(h as usize - y);
        let _ = c.conn.put_image(
            ImageFormat::Z_PIXMAP,
            w.header,
            c.gc,
            fw as u16,
            n as u16,
            0,
            y as i16,
            0,
            c.depth,
            &bytes[y * row..(y + n) * row],
        );
        y += n;
    }
}

fn send_xembed(c: &Conn, to: XWindow, message: u32, detail: u32, data1: u32) {
    let msg = ClientMessageEvent::new(32, to, c.atoms._XEMBED, [CURRENT_TIME, message, detail, data1, 0]);
    let _ = c.conn.send_event(false, to, EventMask::NO_EVENT, msg);
}

/// A window appeared in (or was reparented into) the embed child: the plugin's.
fn adopt(c: &Conn, w: &Win, client: XWindow) {
    if w.client.get() == client {
        return;
    }
    w.client.set(client);
    let _ = c.conn.change_window_attributes(
        client,
        &x11rb::protocol::xproto::ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    );
    send_xembed(c, client, XEMBED_EMBEDDED_NOTIFY, 0, w.embed);
    map_if_wanted(c, client);
}

/// Map an XEmbed client whose `_XEMBED_INFO` says it wants to be mapped.
fn map_if_wanted(c: &Conn, client: XWindow) {
    let info = c
        .conn
        .get_property(false, client, c.atoms._XEMBED_INFO, c.atoms._XEMBED_INFO, 0, 2)
        .ok()
        .and_then(|r| r.reply().ok())
        .and_then(|r| r.value32().map(|v| v.collect::<Vec<u32>>()));
    if let Some(info) = info {
        if info.get(1).is_some_and(|flags| flags & XEMBED_MAPPED != 0) {
            let _ = c.conn.map_window(client);
        }
    }
}

/// Where a press resizes from: the header's top pixels (`which` = `None`) or a border strip
/// (`Some(0)` left, `Some(1)` right, `Some(2)` bottom); (`x`, `y`) in frame coordinates.
fn resize_edge(w: &Win, which: Option<usize>, x: i32, y: i32) -> Option<u32> {
    if !w.state.resizable {
        return None;
    }
    let (fw, fh) = w.size.get();
    let (fw, fh, corner) = (fw as i32, fh as i32, 16);
    match which {
        None if y < TOP_RESIZE => Some(if x < corner {
            SIZE_TOPLEFT
        } else if x >= fw - corner {
            SIZE_TOPRIGHT
        } else {
            SIZE_TOP
        }),
        None => None,
        Some(0) => Some(if y >= fh - corner { SIZE_BOTTOMLEFT } else { SIZE_LEFT }),
        Some(1) => Some(if y >= fh - corner { SIZE_BOTTOMRIGHT } else { SIZE_RIGHT }),
        Some(_) => Some(if x < corner {
            SIZE_BOTTOMLEFT
        } else if x >= fw - corner {
            SIZE_BOTTOMRIGHT
        } else {
            SIZE_BOTTOM
        }),
    }
}

/// Hand a pointer drag to the window manager (move or resize).
fn wm_drag(c: &Conn, w: &Win, root_x: i16, root_y: i16, direction: u32) {
    let _ = c.conn.ungrab_pointer(CURRENT_TIME);
    let msg = ClientMessageEvent::new(32, w.frame, c.atoms._NET_WM_MOVERESIZE, [root_x as u32, root_y as u32, direction, 1, 1]);
    let _ = c.conn.send_event(false, c.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, msg);
}

fn set_header(c: &Conn, w: &Win, hover: Option<Part>, pressed: Option<Part>) {
    if w.state.set_header_state(hover, pressed) {
        paint(c, w);
    }
}

fn frame_of(c: &Conn, window: XWindow) -> Option<Rc<Win>> {
    let ws = c.windows.borrow();
    ws.get(&window).cloned().or_else(|| {
        ws.values().find(|w| w.header == window || w.embed == window || w.borders.contains(&window) || w.client.get() == window).cloned()
    })
}

fn dispatch(c: &Rc<Conn>, event: Event) {
    let a = &c.atoms;
    match event {
        Event::Expose(e) if e.count == 0 => {
            if let Some(w) = frame_of(c, e.window).filter(|w| w.header == e.window) {
                paint(c, &w);
            }
        }
        Event::ConfigureNotify(e) => {
            if let Some(w) = win(c, e.window as isize) {
                let size = (u32::from(e.width), u32::from(e.height));
                if size != w.size.get() {
                    w.size.set(size);
                    layout(c, &w);
                }
            }
        }
        Event::ClientMessage(e) if e.type_ == a.WM_PROTOCOLS && e.format == 32 => {
            if e.data.as_data32()[0] == a.WM_DELETE_WINDOW {
                if let Some(w) = win(c, e.window as isize) {
                    w.state.post(WindowEvent::Closed);
                }
            }
        }
        Event::ButtonPress(e) if e.detail == 1 => {
            let Some(w) = frame_of(c, e.event) else { return };
            let (x, y) = (i32::from(e.event_x), i32::from(e.event_y));
            let border = w.borders.iter().position(|b| *b == e.event);
            // strip-local to frame coordinates (the strips' origins, as `layout` puts them)
            let (hh, b, (fw, fh)) = (header_height(c) as i32, w.border.get() as i32, w.size.get());
            let (fx, fy) = match border {
                Some(0) => (x, y + hh),
                Some(1) => (x + fw as i32 - b, y + hh),
                Some(_) => (x, y + fh as i32 - b),
                None => (x, y),
            };
            if let Some(edge) = resize_edge(&w, border, fx, fy) {
                return wm_drag(c, &w, e.root_x, e.root_y, edge);
            }
            if e.event != w.header {
                return;
            }
            match header::hit(w.size.get().0, x, y, scale_of(c.dpi.get())) {
                Some(Part::Drag) => wm_drag(c, &w, e.root_x, e.root_y, MOVE),
                Some(part) => set_header(c, &w, Some(part), Some(part)),
                None => {}
            }
        }
        Event::ButtonRelease(e) if e.detail == 1 => {
            let Some(w) = frame_of(c, e.event).filter(|w| w.header == e.event) else { return };
            let now = header::hit(w.size.get().0, i32::from(e.event_x), i32::from(e.event_y), scale_of(c.dpi.get()));
            let pressed = w.state.header.borrow().pressed;
            set_header(c, &w, now, None);
            if let Some(part) = pressed.filter(|p| Some(*p) == now) {
                w.state.header_click(part);
                if part == Part::Pin {
                    apply_pin(c, &w);
                }
            }
        }
        Event::MotionNotify(e) => {
            if let Some(w) = frame_of(c, e.event).filter(|w| w.header == e.event) {
                let now = header::hit(w.size.get().0, i32::from(e.event_x), i32::from(e.event_y), scale_of(c.dpi.get()));
                let pressed = w.state.header.borrow().pressed;
                set_header(c, &w, now, pressed);
            }
        }
        Event::LeaveNotify(e) => {
            if let Some(w) = frame_of(c, e.event).filter(|w| w.header == e.event) {
                let pressed = w.state.header.borrow().pressed;
                set_header(c, &w, None, pressed);
            }
        }
        Event::FocusIn(e) => {
            if let Some(w) = win(c, e.event as isize) {
                let client = w.client.get();
                if client != x11rb::NONE {
                    send_xembed(c, client, XEMBED_WINDOW_ACTIVATE, 0, 0);
                    send_xembed(c, client, XEMBED_FOCUS_IN, 0, 0);
                    // keys go to the plugin (its text fields), not to our frame
                    let _ = c.conn.set_input_focus(InputFocus::PARENT, client, CURRENT_TIME);
                }
            }
        }
        Event::FocusOut(e) => {
            if let Some(w) = win(c, e.event as isize) {
                let client = w.client.get();
                if client != x11rb::NONE {
                    send_xembed(c, client, XEMBED_FOCUS_OUT, 0, 0);
                    send_xembed(c, client, XEMBED_WINDOW_DEACTIVATE, 0, 0);
                }
            }
        }
        Event::CreateNotify(e) => {
            if let Some(w) = frame_of(c, e.parent).filter(|w| w.embed == e.parent) {
                adopt(c, &w, e.window);
            }
        }
        Event::ReparentNotify(e) => {
            if let Some(w) = frame_of(c, e.parent).filter(|w| w.embed == e.parent) {
                adopt(c, &w, e.window);
            }
        }
        Event::DestroyNotify(e) => {
            if let Some(w) = frame_of(c, e.window).filter(|w| w.client.get() == e.window) {
                w.client.set(x11rb::NONE);
            }
        }
        Event::PropertyNotify(e) if e.window == c.root && e.atom == a.RESOURCE_MANAGER => {
            let dpi = read_dpi(&c.conn, c.root, a);
            if dpi != c.dpi.get() {
                c.dpi.set(dpi);
                let all: Vec<Rc<Win>> = c.windows.borrow().values().cloned().collect();
                for w in all {
                    w.state.post(WindowEvent::DpiChanged { dpi });
                    w.border.set(border_for(c, w.state.resizable));
                    layout(c, &w);
                }
            }
        }
        Event::PropertyNotify(e) if e.atom == a._XEMBED_INFO && frame_of(c, e.window).is_some_and(|w| w.client.get() == e.window) => {
            map_if_wanted(c, e.window);
        }
        // errors of requests on windows a plugin destroyed meanwhile: nothing to do
        _ => {}
    }
}

/// Read and handle everything the server has sent.
fn pump(c: &Rc<Conn>) {
    loop {
        match c.conn.poll_for_event() {
            Ok(Some(event)) => dispatch(c, event),
            Ok(None) => break,
            Err(e) => {
                log::error!("plugin windows: the X11 connection failed: {e}");
                // close every window's owner side; the next window reconnects
                let all: Vec<Rc<Win>> = c.windows.borrow().values().cloned().collect();
                for w in all {
                    w.state.post(WindowEvent::Closed);
                }
                break;
            }
        }
    }
    let _ = c.conn.flush();
}

/// The owners' ticks (resize requests, closing), like `WM_TIMER` on Windows.
fn tick(c: &Rc<Conn>) {
    let all: Vec<Rc<Win>> = c.windows.borrow().values().cloned().collect();
    for w in all {
        // closed by an earlier window's tick (close_all)
        if !c.windows.borrow().contains_key(&w.frame) {
            continue;
        }
        w.state.ticks.fetch_add(1, Ordering::Relaxed);
        w.state.run_tick();
    }
    let _ = c.conn.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktop_scale_comes_from_xft_dpi() {
        assert_eq!(parse_xft_dpi("Xft.antialias:\t1\nXft.dpi:\t192\nXft.hinting:\t1\n"), Some(192));
        assert_eq!(parse_xft_dpi("Xft.dpi: 120.5"), Some(121));
        assert_eq!(parse_xft_dpi("Xcursor.size: 24"), None);
        assert_eq!(parse_xft_dpi("Xft.dpi: 0"), None, "nonsense is ignored");
        assert!((scale_of(144) - 1.5).abs() < f32::EPSILON);
    }
}
