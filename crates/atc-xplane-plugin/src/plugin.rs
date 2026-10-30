//! The X-Plane side: XPLM entry points, the window, the flight loop that reads the socket and
//! drives the follow-me car, and the toggle command. XPLM calls come only from X-Plane's main
//! thread, so the state lives in a thread local. Nothing here may panic: a panic across
//! `extern "C"` aborts X-Plane.
//!
//! The car is [`CAR_OBJECT`] from the X-Plane 11 library, drawn with the instancing API
//! (X-Plane 11.10+), placed on the ground with a terrain probe; [`crate::follow_me`] drives it.
//! The plugin steers its front wheels through the instance datarefs ([`CAR_DATAREFS`]), and
//! draws our own roof object (`objects/follow_me_roof.obj`: FOLLOW ME sign, beacon, turn
//! signals) as a second instance at the same pose; without that file the car comes alone.
//!
//! Custom instance datarefs: the XPLMInstance docs say "If you use any custom datarefs in
//! your object, they must be registered before the object is loaded. This is true even if
//! their data will be provided via the instance dataref list." So [`ROOF_DATAREFS`] are
//! registered with `XPLMRegisterDataAccessor` (read-only int/float, reading the last values
//! set) in `XPluginStart`, long before the roof object loads; the per-instance values come
//! from `XPLMInstanceSetPosition`.

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, c_char, c_float, c_int, c_void};
use std::io::ErrorKind;
use std::net::UdpSocket;
use std::path::Path;
use std::ptr::null_mut;
use std::time::Instant;

use atc_proto::{ServerMsg, parse_overlay_command};

use crate::follow_me::{FollowMe, LatLon, lights};
use crate::{Board, Kind, PORT, roof_object_path};

type WindowId = *mut c_void;
type CommandRef = *mut c_void;
type ObjectRef = *mut c_void;
type InstanceRef = *mut c_void;
type ProbeRef = *mut c_void;
type DataRef = *mut c_void;

/// The follow-me car: X-Plane 11's crew car (`airport scenery/library.txt`).
const CAR_OBJECT: &CStr = c"lib/airport/vehicles/servicing/crew_car.obj";
/// The car's instance datarefs, set every frame to `[steer_deg, 0, 0]`: with them the
/// plugin, not X-Plane's ground traffic, turns the wheels, and the doors stay shut.
const CAR_DATAREFS: [&CStr; 3] = [
    c"sim/graphics/animation/ground_traffic/tire_steer_deg[0]",
    c"sim/graphics/animation/ground_traffic/door_open[0]",
    c"sim/graphics/animation/ground_traffic/door_open[1]",
];
/// The roof object's instance datarefs, 0 or 1 ([`lights`]); registered by the plugin.
const ROOF_DATAREFS: [&CStr; 3] = [
    c"atc/follow_me/beacon",
    c"atc/follow_me/left",
    c"atc/follow_me/right",
];
/// `xplmType_Int | xplmType_Float`.
const TYPE_INT_FLOAT: c_int = 1 | 2;
/// Path buffers handed to XPLM (its minimum is 256 or 512 bytes).
const PATH_BUF: usize = 1024;
const PROBE_Y: c_int = 0;
const PROBE_HIT_TERRAIN: c_int = 0;
/// Flight loop interval without a car (seconds); with one it runs every frame.
const POLL_S: c_float = 0.1;

#[repr(C)]
struct DrawInfo {
    struct_size: c_int,
    x: c_float,
    y: c_float,
    z: c_float,
    pitch: c_float,
    heading: c_float,
    roll: c_float,
}

#[repr(C)]
#[derive(Default)]
struct ProbeInfo {
    struct_size: c_int,
    location_x: c_float,
    location_y: c_float,
    location_z: c_float,
    normal_x: c_float,
    normal_y: c_float,
    normal_z: c_float,
    velocity_x: c_float,
    velocity_y: c_float,
    velocity_z: c_float,
    is_wet: c_int,
}

type LibraryEnumerator = extern "C" fn(*const c_char, *mut c_void);
type ObjectLoaded = extern "C" fn(ObjectRef, *mut c_void);
type GetInt = extern "C" fn(*mut c_void) -> c_int;
type GetFloat = extern "C" fn(*mut c_void) -> c_float;

/// `xplmFont_Basic`: fixed width, so wrapping by character count fits.
const FONT_BASIC: c_int = 0;
const DECORATION_ROUND_RECTANGLE: c_int = 1;
const LAYER_FLOATING_WINDOWS: c_int = 1;
const COMMAND_BEGIN: c_int = 0;
/// Initial window size and the padding inside it, in boxels.
const WIDTH: c_int = 760;
const ROWS: c_int = 8;
const PAD: c_int = 8;

#[repr(C)]
struct CreateWindow {
    struct_size: c_int,
    left: c_int,
    top: c_int,
    right: c_int,
    bottom: c_int,
    visible: c_int,
    draw: extern "C" fn(WindowId, *mut c_void),
    click: extern "C" fn(WindowId, c_int, c_int, c_int, *mut c_void) -> c_int,
    key: extern "C" fn(WindowId, c_char, c_int, c_char, *mut c_void, c_int),
    cursor: extern "C" fn(WindowId, c_int, c_int, *mut c_void) -> c_int,
    wheel: extern "C" fn(WindowId, c_int, c_int, c_int, c_int, *mut c_void) -> c_int,
    refcon: *mut c_void,
    decoration: c_int,
    layer: c_int,
    right_click: extern "C" fn(WindowId, c_int, c_int, c_int, *mut c_void) -> c_int,
}

type FlightLoop = extern "C" fn(c_float, c_float, c_int, *mut c_void) -> c_float;
type CommandHandler = extern "C" fn(CommandRef, c_int, *mut c_void) -> c_int;

// Linux resolves these against X-Plane when it loads the plugin; on Windows the DLL imports
// them from X-Plane's XPLM_64.dll, described here so no SDK import library is needed.
#[cfg_attr(windows, link(name = "XPLM_64", kind = "raw-dylib"))]
unsafe extern "C" {
    fn XPLMDebugString(s: *const c_char);
    fn XPLMCreateWindowEx(params: *mut CreateWindow) -> WindowId;
    fn XPLMDestroyWindow(id: WindowId);
    fn XPLMSetWindowTitle(id: WindowId, title: *const c_char);
    fn XPLMGetWindowGeometry(
        id: WindowId,
        l: *mut c_int,
        t: *mut c_int,
        r: *mut c_int,
        b: *mut c_int,
    );
    fn XPLMGetWindowIsVisible(id: WindowId) -> c_int;
    fn XPLMSetWindowIsVisible(id: WindowId, visible: c_int);
    fn XPLMGetScreenBoundsGlobal(l: *mut c_int, t: *mut c_int, r: *mut c_int, b: *mut c_int);
    fn XPLMGetFontDimensions(font: c_int, w: *mut c_int, h: *mut c_int, digits_only: *mut c_int);
    fn XPLMDrawString(
        rgb: *mut c_float,
        x: c_int,
        y: c_int,
        s: *const c_char,
        wrap: *mut c_int,
        font: c_int,
    );
    fn XPLMRegisterFlightLoopCallback(cb: FlightLoop, interval: c_float, refcon: *mut c_void);
    fn XPLMUnregisterFlightLoopCallback(cb: FlightLoop, refcon: *mut c_void);
    fn XPLMCreateCommand(name: *const c_char, description: *const c_char) -> CommandRef;
    fn XPLMRegisterCommandHandler(
        cmd: CommandRef,
        cb: CommandHandler,
        before: c_int,
        refcon: *mut c_void,
    );
    fn XPLMUnregisterCommandHandler(
        cmd: CommandRef,
        cb: CommandHandler,
        before: c_int,
        refcon: *mut c_void,
    );
    fn XPLMLookupObjects(
        path: *const c_char,
        lat: c_float,
        lon: c_float,
        enumerator: LibraryEnumerator,
        refcon: *mut c_void,
    ) -> c_int;
    fn XPLMLoadObjectAsync(path: *const c_char, cb: ObjectLoaded, refcon: *mut c_void);
    fn XPLMUnloadObject(obj: ObjectRef);
    fn XPLMCreateInstance(obj: ObjectRef, datarefs: *const *const c_char) -> InstanceRef;
    fn XPLMDestroyInstance(instance: InstanceRef);
    fn XPLMInstanceSetPosition(instance: InstanceRef, pos: *const DrawInfo, data: *const c_float);
    fn XPLMCreateProbe(kind: c_int) -> ProbeRef;
    fn XPLMDestroyProbe(probe: ProbeRef);
    fn XPLMProbeTerrainXYZ(
        probe: ProbeRef,
        x: c_float,
        y: c_float,
        z: c_float,
        info: *mut ProbeInfo,
    ) -> c_int;
    fn XPLMWorldToLocal(lat: f64, lon: f64, alt: f64, x: *mut f64, y: *mut f64, z: *mut f64);
    fn XPLMFindDataRef(name: *const c_char) -> DataRef;
    fn XPLMGetDatad(dr: DataRef) -> f64;
    fn XPLMRegisterDataAccessor(
        name: *const c_char,
        data_type: c_int,
        writable: c_int,
        read_int: Option<GetInt>,
        write_int: *mut c_void,
        read_float: Option<GetFloat>,
        write_float: *mut c_void,
        read_double: *mut c_void,
        write_double: *mut c_void,
        read_int_array: *mut c_void,
        write_int_array: *mut c_void,
        read_float_array: *mut c_void,
        write_float_array: *mut c_void,
        read_data: *mut c_void,
        write_data: *mut c_void,
        read_refcon: *mut c_void,
        write_refcon: *mut c_void,
    ) -> DataRef;
    fn XPLMUnregisterDataAccessor(dr: DataRef);
    fn XPLMGetMyID() -> c_int;
    fn XPLMGetPluginInfo(
        id: c_int,
        name: *mut c_char,
        path: *mut c_char,
        sig: *mut c_char,
        desc: *mut c_char,
    );
    fn XPLMGetSystemPath(path: *mut c_char);
    fn XPLMEnableFeature(feature: *const c_char, enable: c_int);
}

/// The aircraft position datarefs.
struct Own {
    lat: DataRef,
    lon: DataRef,
    elevation_m: DataRef,
}

impl Own {
    fn find() -> Option<Own> {
        let find = |name: &CStr| {
            let dr = unsafe { XPLMFindDataRef(name.as_ptr()) };
            (!dr.is_null()).then_some(dr)
        };
        Some(Own {
            lat: find(c"sim/flightmodel/position/latitude")?,
            lon: find(c"sim/flightmodel/position/longitude")?,
            elevation_m: find(c"sim/flightmodel/position/elevation")?,
        })
    }

    fn pos(&self) -> (LatLon, f64) {
        unsafe {
            let p = LatLon {
                lat: XPLMGetDatad(self.lat),
                lon: XPLMGetDatad(self.lon),
            };
            (p, XPLMGetDatad(self.elevation_m))
        }
    }
}

/// The follow-me car on screen: its driver and its instances, the car and its roof object
/// (each null until its object loaded; the roof stays null without its object).
struct Car {
    driver: FollowMe,
    instance: InstanceRef,
    roof: InstanceRef,
    /// X-Plane refused the roof instance: not asked again for this car (logged once).
    roof_refused: bool,
}

/// An object: not looked up yet, loading, loaded, or missing (the car from the library,
/// the roof object from the plugin's folder).
#[derive(Clone, Copy, PartialEq)]
enum Object {
    None,
    Loading,
    Loaded(ObjectRef),
    Missing,
}

struct Overlay {
    window: WindowId,
    command: CommandRef,
    socket: Option<UdpSocket>,
    board: Board,
    start: Instant,
    own: Option<Own>,
    probe: ProbeRef,
    object: Object,
    roof: Object,
    /// [`ROOF_DATAREFS`] as registered (null: refused).
    roof_datarefs: [DataRef; 3],
    car: Option<Car>,
}

impl Overlay {
    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }
}

thread_local! {
    static OVERLAY: RefCell<Option<Overlay>> = const { RefCell::new(None) };
    /// The car object once loaded (null: loading failed), until [`Overlay::drive`] takes it.
    static LOADED: Cell<Option<ObjectRef>> = const { Cell::new(None) };
    /// The same for the roof object.
    static LOADED_ROOF: Cell<Option<ObjectRef>> = const { Cell::new(None) };
    /// The roof lights last set: what reading [`ROOF_DATAREFS`] gives.
    static LIGHTS: Cell<[f32; 3]> = const { Cell::new([0.0; 3]) };
}

/// Runs `f`; a panic is logged and gives `fallback` instead of unwinding into X-Plane
/// (which would abort it).
fn guard<T>(what: &str, fallback: T, f: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(v) => v,
        Err(e) => {
            let msg = e
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| e.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("unknown panic");
            log(&format!("{what} panicked: {msg}"));
            fallback
        }
    }
}

fn log(msg: &str) {
    let line = format!("atc-overlay: {msg}\n\0");
    unsafe { XPLMDebugString(line.as_ptr().cast()) };
}

/// Copies `s` into an XPLM name buffer (256 bytes).
unsafe fn put(buf: *mut c_char, s: &CStr) {
    unsafe { std::ptr::copy_nonoverlapping(s.as_ptr(), buf, s.count_bytes() + 1) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn XPluginStart(
    name: *mut c_char,
    sig: *mut c_char,
    desc: *mut c_char,
) -> c_int {
    guard("XPluginStart", 0, || {
        unsafe {
            put(name, c"Virtual ATC overlay");
            put(sig, c"atc.virtual.overlay");
            put(desc, c"Shows the Virtual ATC radio traffic on screen.");
        }
        let (mut l, mut t, mut r, mut b) = (0, 0, 0, 0);
        let (mut cw, mut ch, mut digits) = (0, 0, 0);
        unsafe {
            XPLMGetScreenBoundsGlobal(&mut l, &mut t, &mut r, &mut b);
            XPLMGetFontDimensions(FONT_BASIC, &mut cw, &mut ch, &mut digits);
        }
        let height = ROWS * (ch + 4) + 2 * PAD;
        let left = (l + r) / 2 - WIDTH / 2;
        let top = t - 60;
        let mut params = CreateWindow {
            struct_size: size_of::<CreateWindow>() as c_int,
            left,
            top,
            right: left + WIDTH,
            bottom: top - height,
            visible: 1,
            draw,
            click,
            key,
            cursor,
            wheel,
            refcon: null_mut(),
            decoration: DECORATION_ROUND_RECTANGLE,
            layer: LAYER_FLOATING_WINDOWS,
            right_click: click,
        };
        let window = unsafe { XPLMCreateWindowEx(&mut params) };
        unsafe { XPLMSetWindowTitle(window, c"Virtual ATC".as_ptr()) };
        let command = unsafe {
            XPLMCreateCommand(
                c"atc/overlay/toggle".as_ptr(),
                c"Show or hide the Virtual ATC window".as_ptr(),
            )
        };
        unsafe { XPLMRegisterCommandHandler(command, toggle, 1, null_mut()) };
        // Unix-style paths on every OS (macOS would give colon paths otherwise).
        unsafe { XPLMEnableFeature(c"XPLM_USE_NATIVE_PATHS".as_ptr(), 1) };
        let roof_datarefs = register_roof_datarefs();
        OVERLAY.with_borrow_mut(|o| {
            *o = Some(Overlay {
                window,
                command,
                socket: None,
                board: Board::default(),
                start: Instant::now(),
                own: Own::find(),
                probe: unsafe { XPLMCreateProbe(PROBE_Y) },
                object: Object::None,
                roof: Object::None,
                roof_datarefs,
                car: None,
            })
        });
        1
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn XPluginStop() {
    guard("XPluginStop", (), || {
        if let Some(mut o) = OVERLAY.with_borrow_mut(Option::take) {
            o.remove_car();
            for (object, loaded) in [(o.object, LOADED.take()), (o.roof, LOADED_ROOF.take())] {
                let obj = match (object, loaded) {
                    (Object::Loaded(obj), _) => obj,
                    (_, Some(obj)) => obj,
                    _ => null_mut(),
                };
                if !obj.is_null() {
                    unsafe { XPLMUnloadObject(obj) };
                }
            }
            for dr in o.roof_datarefs {
                if !dr.is_null() {
                    unsafe { XPLMUnregisterDataAccessor(dr) };
                }
            }
            unsafe {
                if !o.probe.is_null() {
                    XPLMDestroyProbe(o.probe);
                }
                XPLMUnregisterCommandHandler(o.command, toggle, 1, null_mut());
                XPLMDestroyWindow(o.window);
            }
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn XPluginEnable() -> c_int {
    guard("XPluginEnable", 0, || {
        let socket = UdpSocket::bind(("127.0.0.1", PORT)).and_then(|s| {
            s.set_nonblocking(true)?;
            Ok(s)
        });
        let socket = match socket {
            Ok(s) => Some(s),
            Err(e) => {
                log(&format!("cannot listen on udp port {PORT}: {e}"));
                None
            }
        };
        OVERLAY.with_borrow_mut(|o| {
            if let Some(o) = o {
                if socket.is_none() {
                    let now = o.now();
                    o.board
                        .push(&format!("cannot listen on udp port {PORT}"), now);
                }
                o.socket = socket;
            }
        });
        unsafe { XPLMRegisterFlightLoopCallback(poll, POLL_S, null_mut()) };
        1
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn XPluginDisable() {
    guard("XPluginDisable", (), || {
        unsafe { XPLMUnregisterFlightLoopCallback(poll, null_mut()) };
        OVERLAY.with_borrow_mut(|o| {
            if let Some(o) = o {
                o.socket = None;
                o.remove_car();
            }
        });
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn XPluginReceiveMessage(_from: c_int, _msg: c_int, _param: *mut c_void) {}

/// Reads every waiting datagram and moves the car; every [`POLL_S`], every frame while a
/// car is shown.
extern "C" fn poll(since_last: c_float, _: c_float, _: c_int, _: *mut c_void) -> c_float {
    guard("poll", POLL_S, || {
        OVERLAY.with_borrow_mut(|o| {
            let Some(o) = o else { return POLL_S };
            let mut buf = [0u8; 65536];
            let mut lines = Vec::new();
            if let Some(socket) = &o.socket {
                loop {
                    match socket.recv(&mut buf) {
                        Ok(n) => lines.push(String::from_utf8_lossy(&buf[..n]).into_owned()),
                        Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                        Err(e) => {
                            log(&format!("udp receive failed: {e}"));
                            break;
                        }
                    }
                }
            }
            for line in lines {
                match parse_overlay_command(&line) {
                    Some(cmd) => o.command(cmd),
                    None => {
                        let now = o.now();
                        o.board.push(&line, now);
                    }
                }
            }
            o.drive(f64::from(since_last));
            if o.car.is_some() { -1.0 } else { POLL_S }
        })
    })
}

impl Overlay {
    fn command(&mut self, cmd: ServerMsg) {
        match cmd {
            ServerMsg::FollowMe {
                id,
                path,
                end_heading_deg,
                aside_left,
            } => {
                let Some(own) = &self.own else {
                    return log("no aircraft position datarefs: no follow-me car");
                };
                let path: Vec<LatLon> =
                    path.iter().map(|&[lat, lon]| LatLon { lat, lon }).collect();
                let (aircraft, _) = own.pos();
                let driver = match FollowMe::new(id, &path, end_heading_deg, aside_left, aircraft) {
                    Ok(d) => d,
                    Err(e) => return log(&format!("follow-me {id} ignored: {e}")),
                };
                self.remove_car();
                self.car = Some(Car {
                    driver,
                    instance: null_mut(),
                    roof: null_mut(),
                    roof_refused: false,
                });
                self.load_object(aircraft);
                self.load_roof();
            }
            ServerMsg::FollowMeCancel { id }
                if self.car.as_ref().is_some_and(|c| c.driver.id() == id) =>
            {
                self.remove_car();
            }
            _ => {}
        }
    }

    /// Looks the car up in the library near `at` and starts loading it, once.
    fn load_object(&mut self, at: LatLon) {
        if self.object != Object::None {
            return;
        }
        let mut found: Option<Vec<u8>> = None;
        // SAFETY: the enumerator runs only during the call, while `found` is alive.
        let n = unsafe {
            XPLMLookupObjects(
                CAR_OBJECT.as_ptr(),
                at.lat as c_float,
                at.lon as c_float,
                first_object,
                (&raw mut found).cast(),
            )
        };
        match found {
            Some(mut path) if n > 0 => {
                path.push(0);
                self.object = Object::Loading;
                unsafe { XPLMLoadObjectAsync(path.as_ptr().cast(), object_loaded, null_mut()) };
            }
            _ => {
                self.object = Object::Missing;
                log(&format!(
                    "{} is not in the library",
                    CAR_OBJECT.to_string_lossy()
                ));
                let now = self.now();
                self.board
                    .push("follow-me car object missing from the X-Plane library", now);
            }
        }
    }

    /// One frame of the car: drives it, creates its instance once the object is there,
    /// places it on the ground; removes it when the driver is done.
    fn drive(&mut self, dt: f64) {
        match LOADED.take() {
            Some(obj) if obj.is_null() => {
                self.object = Object::Missing;
                log("follow-me car object failed to load");
            }
            Some(obj) => self.object = Object::Loaded(obj),
            None => {}
        }
        match LOADED_ROOF.take() {
            Some(obj) if obj.is_null() => {
                self.roof = Object::Missing;
                log("follow-me roof object failed to load: the car comes without its sign");
            }
            Some(obj) => self.roof = Object::Loaded(obj),
            None => {}
        }
        let now = self.now();
        let (Some(car), Some(own)) = (self.car.as_mut(), self.own.as_ref()) else {
            return;
        };
        let (aircraft, elevation_m) = own.pos();
        let Some(pose) = car.driver.update(dt, aircraft) else {
            return self.remove_car();
        };
        if car.instance.is_null() {
            let Object::Loaded(obj) = self.object else {
                return;
            };
            car.instance = create_instance(obj, &CAR_DATAREFS);
            if car.instance.is_null() {
                return;
            }
        }
        if car.roof.is_null()
            && !car.roof_refused
            && let Object::Loaded(obj) = self.roof
        {
            car.roof = create_instance(obj, &ROOF_DATAREFS);
            if car.roof.is_null() {
                car.roof_refused = true;
                log("follow-me roof instance refused: the car comes without its sign");
            }
        }
        let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
        unsafe {
            XPLMWorldToLocal(
                pose.pos.lat,
                pose.pos.lon,
                elevation_m,
                &mut x,
                &mut y,
                &mut z,
            )
        };
        let mut probe = ProbeInfo {
            struct_size: size_of::<ProbeInfo>() as c_int,
            ..ProbeInfo::default()
        };
        if !self.probe.is_null()
            && unsafe {
                XPLMProbeTerrainXYZ(
                    self.probe,
                    x as c_float,
                    y as c_float,
                    z as c_float,
                    &mut probe,
                )
            } == PROBE_HIT_TERRAIN
        {
            y = f64::from(probe.location_y);
        }
        let at = DrawInfo {
            struct_size: size_of::<DrawInfo>() as c_int,
            x: x as c_float,
            y: y as c_float,
            z: z as c_float,
            pitch: 0.0,
            heading: pose.heading_deg as c_float,
            roll: 0.0,
        };
        // One float per instance dataref, always a valid pointer (before X-Plane 11.50 a
        // null one crashed even without datarefs).
        let wheels = [pose.steer_deg as c_float, 0.0, 0.0];
        unsafe { XPLMInstanceSetPosition(car.instance, &at, wheels.as_ptr()) };
        let roof = lights(&pose, now);
        LIGHTS.set(roof);
        if !car.roof.is_null() {
            unsafe { XPLMInstanceSetPosition(car.roof, &at, roof.as_ptr()) };
        }
    }

    /// Starts loading the roof object from the plugin's folder, once; without the file the
    /// car is drawn alone (logged once).
    fn load_roof(&mut self) {
        if self.roof != Object::None {
            return;
        }
        self.roof = Object::Missing;
        if self.roof_datarefs.iter().any(|dr| dr.is_null()) {
            return log("follow-me roof datarefs not registered: the car comes without its sign");
        }
        let path = match roof_object_xplm_path() {
            Ok(path) => path,
            Err(why) => {
                return log(&format!(
                    "follow-me roof object not loaded ({why}): the car comes without its sign"
                ));
            }
        };
        self.roof = Object::Loading;
        unsafe { XPLMLoadObjectAsync(path.as_ptr().cast(), roof_loaded, null_mut()) };
    }

    fn remove_car(&mut self) {
        if let Some(car) = self.car.take() {
            for instance in [car.instance, car.roof] {
                if !instance.is_null() {
                    unsafe { XPLMDestroyInstance(instance) };
                }
            }
        }
        LIGHTS.set([0.0; 3]);
    }
}

/// An instance of `obj` with `datarefs` (null if X-Plane refuses it).
fn create_instance(obj: ObjectRef, datarefs: &[&CStr]) -> InstanceRef {
    let mut list: Vec<*const c_char> = datarefs.iter().map(|d| d.as_ptr()).collect();
    list.push(std::ptr::null());
    // SAFETY: a NULL-terminated list of NUL-terminated names, alive during the call.
    unsafe { XPLMCreateInstance(obj, list.as_ptr()) }
}

/// Registers [`ROOF_DATAREFS`] (read-only; index `i` passed as the refcon); a refused one
/// stays null.
fn register_roof_datarefs() -> [DataRef; 3] {
    let none = null_mut();
    std::array::from_fn(|i| {
        let dr = unsafe {
            XPLMRegisterDataAccessor(
                ROOF_DATAREFS[i].as_ptr(),
                TYPE_INT_FLOAT,
                0,
                Some(read_light_int),
                none,
                Some(read_light_float),
                none,
                none,
                none,
                none,
                none,
                none,
                none,
                none,
                none,
                std::ptr::without_provenance_mut(i),
                none,
            )
        };
        if dr.is_null() {
            log(&format!(
                "cannot register {}",
                ROOF_DATAREFS[i].to_string_lossy()
            ));
        }
        dr
    })
}

/// The value of roof dataref number `refcon` (an index, not a pointer).
fn light(refcon: *mut c_void) -> f32 {
    LIGHTS.get().get(refcon.addr()).copied().unwrap_or(0.0)
}

extern "C" fn read_light_int(refcon: *mut c_void) -> c_int {
    guard("read_light_int", 0, || light(refcon) as c_int)
}

extern "C" fn read_light_float(refcon: *mut c_void) -> c_float {
    guard("read_light_float", 0.0, || light(refcon))
}

/// The roof object's path for `XPLMLoadObjectAsync`, NUL-terminated: relative to the
/// X-Plane folder (as XPLMLoadObject wants it), or absolute if the plugin lies elsewhere;
/// why not, when the file is not there or a path is not UTF-8.
fn roof_object_xplm_path() -> Result<Vec<u8>, String> {
    let mut xpl = [0 as c_char; PATH_BUF];
    let mut root = [0 as c_char; PATH_BUF];
    unsafe {
        XPLMGetPluginInfo(
            XPLMGetMyID(),
            null_mut(),
            xpl.as_mut_ptr(),
            null_mut(),
            null_mut(),
        );
        XPLMGetSystemPath(root.as_mut_ptr());
    }
    // SAFETY: XPLM wrote NUL-terminated strings into the zeroed buffers.
    let xpl = unsafe { CStr::from_ptr(xpl.as_ptr()) };
    let root = unsafe { CStr::from_ptr(root.as_ptr()) };
    let not_utf8 =
        |what: &str, p: &CStr| format!("{what} path is not UTF-8: {}", p.to_string_lossy());
    let xpl = xpl.to_str().map_err(|_| not_utf8("plugin", xpl))?;
    let root = root.to_str().map_err(|_| not_utf8("X-Plane", root))?;
    let path = roof_object_path(Path::new(xpl));
    if !path.is_file() {
        return Err(format!("{} is missing", path.display()));
    }
    let relative = if root.is_empty() {
        &path
    } else {
        path.strip_prefix(root).unwrap_or(&path)
    };
    let mut bytes = relative
        .to_str()
        .ok_or_else(|| format!("{} is not UTF-8", relative.display()))?
        .as_bytes()
        .to_vec();
    bytes.push(0);
    Ok(bytes)
}

/// `XPLMLookupObjects` enumerator: keeps the first file path.
extern "C" fn first_object(path: *const c_char, refcon: *mut c_void) {
    guard("first_object", (), || {
        if path.is_null() || refcon.is_null() {
            return;
        }
        // SAFETY: `refcon` is the `Option<Vec<u8>>` that `load_object` passed, alive for the
        // call; `path` is a NUL-terminated string from X-Plane.
        let found = unsafe { &mut *refcon.cast::<Option<Vec<u8>>>() };
        if found.is_none() {
            *found = Some(unsafe { CStr::from_ptr(path) }.to_bytes().to_vec());
        }
    })
}

/// `XPLMLoadObjectAsync` callback, on the main thread. It may come while `OVERLAY` is
/// borrowed (from inside the load call), so the object is handed over in [`LOADED`].
extern "C" fn object_loaded(obj: ObjectRef, _: *mut c_void) {
    guard("object_loaded", (), || {
        LOADED.set(Some(obj));
    })
}

/// The same for the roof object, through [`LOADED_ROOF`].
extern "C" fn roof_loaded(obj: ObjectRef, _: *mut c_void) {
    guard("roof_loaded", (), || {
        LOADED_ROOF.set(Some(obj));
    })
}

extern "C" fn draw(id: WindowId, _: *mut c_void) {
    guard("draw", (), || {
        let (mut l, mut t, mut r, mut b) = (0, 0, 0, 0);
        let (mut cw, mut ch, mut digits) = (0, 0, 0);
        unsafe {
            XPLMGetWindowGeometry(id, &mut l, &mut t, &mut r, &mut b);
            XPLMGetFontDimensions(FONT_BASIC, &mut cw, &mut ch, &mut digits);
        }
        let width = ((r - l - 2 * PAD) / cw.max(1)).max(1) as usize;
        let rows = OVERLAY.with_borrow(|o| match o {
            Some(o) => o.board.rows(o.now(), width),
            None => Vec::new(),
        });
        // Newest at the bottom; rows that do not fit fall off the top.
        let mut y = b + PAD;
        for row in rows.iter().rev() {
            if y + ch > t - PAD {
                break;
            }
            let mut rgb = match row.kind {
                Kind::Atc => [1.0, 1.0, 1.0],
                Kind::Pilot => [0.55, 0.9, 0.55],
                Kind::Info => [0.9, 0.8, 0.4],
                Kind::Transmitting => [1.0, 0.4, 0.3],
            };
            if !row.fresh {
                rgb = rgb.map(|c| c * 0.55);
            }
            let mut text = crate::ascii_fold(&row.text).into_bytes();
            text.push(0);
            unsafe {
                XPLMDrawString(
                    rgb.as_mut_ptr(),
                    l + PAD,
                    y,
                    text.as_ptr().cast(),
                    null_mut(),
                    FONT_BASIC,
                )
            };
            y += ch + 4;
        }
    })
}

extern "C" fn toggle(_: CommandRef, phase: c_int, _: *mut c_void) -> c_int {
    guard("toggle", 1, || {
        if phase == COMMAND_BEGIN {
            OVERLAY.with_borrow(|o| {
                if let Some(o) = o {
                    unsafe {
                        let visible = XPLMGetWindowIsVisible(o.window);
                        XPLMSetWindowIsVisible(o.window, (visible == 0) as c_int);
                    }
                }
            });
        }
        1
    })
}

extern "C" fn click(_: WindowId, _: c_int, _: c_int, _: c_int, _: *mut c_void) -> c_int {
    0
}

extern "C" fn key(_: WindowId, _: c_char, _: c_int, _: c_char, _: *mut c_void, _: c_int) {}

extern "C" fn cursor(_: WindowId, _: c_int, _: c_int, _: *mut c_void) -> c_int {
    0
}

extern "C" fn wheel(_: WindowId, _: c_int, _: c_int, _: c_int, _: c_int, _: *mut c_void) -> c_int {
    0
}
