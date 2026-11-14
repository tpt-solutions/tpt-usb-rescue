//! Window layout, message handling and background jobs for the GUI.
//!
//! Jobs run on worker threads and call the same `tpt_usb_rescue::ops` functions as the CLI. Worker
//! threads never touch controls directly. They post messages to the window, and the UI thread
//! applies them. This keeps all Win32 control access on one thread.

use crate::win32::{self, HWND, LPARAM, LRESULT, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WPARAM};
use std::cell::{Cell, RefCell};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tpt_usb_rescue::ops::{self, ImageRequest, OpResult};
use tpt_usb_rescue_hal::{DeviceInfo, list_devices};
use tpt_usb_rescue_imager::engine::ProgressHook;
use tpt_usb_rescue_imager::mapfile::Totals;

const CLASS_NAME: &str = "TptUsbRescueMainWindow";
const TITLE: &str = "tpt-usb-rescue";

// Control identifiers.
const ID_SRC_BROWSE: usize = 102;
const ID_DEV: usize = 103;
const ID_DEV_REFRESH: usize = 104;
const ID_OUT_BROWSE: usize = 106;
const ID_RESUME: usize = 110;
const ID_SYSDISK: usize = 111;
const ID_VALIDATE: usize = 112;
const ID_EXTRACT: usize = 113;
const ID_IMAGE: usize = 120;
const ID_CARVE: usize = 121;
const ID_VERIFY: usize = 122;
const ID_ANALYZE: usize = 123;
const ID_PROGRESS: usize = 130;
const ID_LOG: usize = 131;

// Messages posted from worker threads.
const MSG_LOG: u32 = WM_APP + 1; // lparam: *mut String
const MSG_PROGRESS: u32 = WM_APP + 2; // wparam: percent 0..=100
const MSG_DONE: u32 = WM_APP + 3; // wparam: 0 ok / 1 failed; lparam: *mut String (error text)

const WINDOW_W: i32 = 740;
const WINDOW_H: i32 = 640;

#[derive(Clone, Copy)]
struct Controls {
    main: HWND,
    src: HWND,
    dev: HWND,
    out: HWND,
    map: HWND,
    resume: HWND,
    sysdisk: HWND,
    validate: HWND,
    extract: HWND,
    image_btn: HWND,
    carve_btn: HWND,
    verify_btn: HWND,
    analyze_btn: HWND,
    refresh_btn: HWND,
    src_btn: HWND,
    out_btn: HWND,
    progress: HWND,
    log: HWND,
}

#[derive(Default)]
struct State {
    ctrl: Option<Controls>,
    devices: Vec<DeviceInfo>,
}

thread_local! {
    // Only the UI thread touches this state. Worker threads never do.
    static STATE: RefCell<State> = RefCell::new(State::default());
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

fn controls() -> Option<Controls> {
    STATE.with(|s| s.borrow().ctrl)
}

pub fn run() {
    win32::init_common_controls();
    let class_w = win32::wide(CLASS_NAME);
    let class = win32::WNDCLASSEXW {
        cbSize: std::mem::size_of::<win32::WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: wnd_proc,
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: win32::module_handle(),
        hIcon: std::ptr::null_mut(),
        hCursor: win32::arrow_cursor(),
        hbrBackground: (win32::COLOR_BTNFACE + 1) as win32::HBRUSH,
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_w.as_ptr(),
        hIconSm: std::ptr::null_mut(),
    };
    if win32::register_class(&class) == 0 {
        win32::message_box("tpt-usb-rescue", "Could not register the window class.");
        return;
    }

    let main = win32::create_main_window(CLASS_NAME, TITLE, WINDOW_W, WINDOW_H);
    if main.is_null() {
        win32::message_box("tpt-usb-rescue", "Could not create the main window.");
        return;
    }
    build_controls(main);
    refresh_devices();
    append_log(
        "Pick a source (a device or a file) and an image path, then press Start imaging.\r\n\
         Physical drives need an Administrator window. Carve, Verify and Analyze work on the image path.\r\n",
    );
    win32::show(main);
    win32::run_message_loop(main);
}

fn build_controls(main: HWND) {
    use win32::{
        BS_AUTOCHECKBOX, CBS_DROPDOWNLIST, CBS_HASSTRINGS, ES_AUTOHSCROLL, ES_AUTOVSCROLL,
        ES_MULTILINE, ES_READONLY, WS_BORDER, WS_TABSTOP, WS_VSCROLL,
    };
    let tab = WS_TABSTOP;
    let label = |text, x, y, w| {
        win32::create_child("STATIC", text, 0, (x, y + 3, w, 20), main, 0);
    };
    let edit = |x, y, w| {
        win32::create_child(
            "EDIT",
            "",
            WS_BORDER | ES_AUTOHSCROLL | tab,
            (x, y, w, 22),
            main,
            0,
        )
    };
    let button = |text, x, y, w, id| {
        win32::create_child("BUTTON", text, WS_TABSTOP, (x, y, w, 26), main, id)
    };

    label("Source", 12, 12, 76);
    let src = edit(92, 10, 500);
    let src_btn = button("Browse...", 600, 10, 110, ID_SRC_BROWSE);

    label("Device", 12, 44, 76);
    let dev = win32::create_child(
        "COMBOBOX",
        "",
        CBS_DROPDOWNLIST | CBS_HASSTRINGS | WS_VSCROLL | tab,
        (92, 42, 500, 300),
        main,
        ID_DEV,
    );
    let refresh_btn = button("Refresh", 600, 42, 110, ID_DEV_REFRESH);

    label("Image", 12, 76, 76);
    let out = edit(92, 74, 500);
    let out_btn = button("Browse...", 600, 74, 110, ID_OUT_BROWSE);

    label("Mapfile", 12, 108, 76);
    let map = edit(92, 106, 500);

    let check = |text, x, id| {
        win32::create_child(
            "BUTTON",
            text,
            BS_AUTOCHECKBOX | tab,
            (x, 138, 170, 22),
            main,
            id,
        )
    };
    let resume = check("Resume existing", 92, ID_RESUME);
    let sysdisk = check("Allow system disk", 270, ID_SYSDISK);
    let validate = check("Validate when carving", 430, ID_VALIDATE);
    let extract = check("Extract on analyze", 600, ID_EXTRACT);
    win32::send(validate, win32::BM_SETCHECK, win32::BST_CHECKED, 0);

    let image_btn = button("Start imaging", 92, 172, 150, ID_IMAGE);
    let carve_btn = button("Carve", 252, 172, 110, ID_CARVE);
    let verify_btn = button("Verify", 372, 172, 110, ID_VERIFY);
    let analyze_btn = button("Analyze", 492, 172, 110, ID_ANALYZE);

    let progress = win32::create_child(
        "msctls_progress32",
        "",
        0,
        (92, 210, 618, 18),
        main,
        ID_PROGRESS,
    );
    win32::progress_range(progress, 100);

    label("Log", 12, 238, 76);
    let log = win32::create_child(
        "EDIT",
        "",
        WS_BORDER | WS_VSCROLL | ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY,
        (12, 262, 698, 330),
        main,
        ID_LOG,
    );

    STATE.with(|s| {
        s.borrow_mut().ctrl = Some(Controls {
            main,
            src,
            dev,
            out,
            map,
            resume,
            sysdisk,
            validate,
            extract,
            image_btn,
            carve_btn,
            verify_btn,
            analyze_btn,
            refresh_btn,
            src_btn,
            out_btn,
            progress,
            log,
        })
    });
}

/// Reloads the device list into the combo box.
fn refresh_devices() {
    let Some(c) = controls() else { return };
    win32::combo_reset(c.dev);
    match list_devices() {
        Ok(devices) => {
            if devices.is_empty() {
                append_log("No devices found. Physical drives need an Administrator window.\r\n");
            }
            for d in &devices {
                win32::combo_add(
                    c.dev,
                    &format!("{}  ({})", d.path.display(), ops::human_bytes(d.size_bytes)),
                );
            }
            STATE.with(|s| s.borrow_mut().devices = devices);
        }
        Err(e) => append_log(&format!("Could not list devices: {e}\r\n")),
    }
}

fn append_log(text: &str) {
    if let Some(c) = controls() {
        win32::append_text(c.log, text);
    }
}

fn set_busy(busy: bool) {
    BUSY.with(|b| b.set(busy));
    if let Some(c) = controls() {
        for h in [
            c.image_btn,
            c.carve_btn,
            c.verify_btn,
            c.analyze_btn,
            c.refresh_btn,
            c.src_btn,
            c.out_btn,
        ] {
            win32::enable(h, !busy);
        }
    }
}

fn is_busy() -> bool {
    BUSY.with(|b| b.get())
}

/// Takes ownership of a String that a worker thread posted with `Box::into_raw`.
fn take_string(lp: LPARAM) -> String {
    // SAFETY: every MSG_LOG / MSG_DONE is posted with a pointer from Box::into_raw on a String, and
    // each message is delivered exactly once, so reclaiming the box here is the only owner.
    let boxed = unsafe { Box::from_raw(lp as *mut String) };
    *boxed
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = wp & 0xFFFF;
            let code = ((wp >> 16) & 0xFFFF) as u32;
            on_command(id, code);
            0
        }
        MSG_LOG => {
            let text = take_string(lp);
            append_log(&text);
            0
        }
        MSG_PROGRESS => {
            if let Some(c) = controls() {
                win32::progress_pos(c.progress, wp);
            }
            0
        }
        MSG_DONE => {
            let text = take_string(lp);
            finish_job(wp == 0, &text);
            0
        }
        WM_CLOSE => win32::def_proc(hwnd, msg, wp, lp),
        WM_DESTROY => {
            win32::quit(0);
            0
        }
        _ => win32::def_proc(hwnd, msg, wp, lp),
    }
}

fn on_command(id: usize, code: u32) {
    let Some(c) = controls() else { return };
    if code == win32::BN_CLICKED {
        match id {
            ID_DEV_REFRESH => refresh_devices(),
            ID_SRC_BROWSE => {
                let filter = "All files (*.*)\0*.*\0\0";
                if let Some(p) = win32::file_dialog(c.main, false, filter, "") {
                    win32::set_text(c.src, &p);
                }
            }
            ID_OUT_BROWSE => {
                let filter = "Disk image (*.img)\0*.img\0All files (*.*)\0*.*\0\0";
                if let Some(p) = win32::file_dialog(c.main, true, filter, "img") {
                    win32::set_text(c.out, &p);
                }
            }
            ID_IMAGE => start_image(),
            ID_CARVE => start_on_image(|image| Job::Carve {
                image,
                validate: win32::is_checked(c.validate),
            }),
            ID_VERIFY => start_on_image(|image| Job::Verify { image }),
            ID_ANALYZE => start_on_image(|image| Job::Analyze {
                image,
                extract: win32::is_checked(c.extract),
            }),
            _ => {}
        }
    } else if id == ID_DEV && code == win32::CBN_SELCHANGE {
        if let Some(text) = win32::combo_selected_text(c.dev) {
            // The combo text is "<path>  (<size>)". The path is everything before the two spaces.
            let path = text.split("  (").next().unwrap_or(&text).to_string();
            win32::set_text(c.src, &path);
        }
    }
}

enum Job {
    Image(ImageRequest),
    Carve { image: PathBuf, validate: bool },
    Verify { image: PathBuf },
    Analyze { image: PathBuf, extract: bool },
}

fn start_image() {
    let Some(c) = controls() else { return };
    let device = win32::get_text(c.src).trim().to_string();
    let output = win32::get_text(c.out).trim().to_string();
    let mut mapfile = win32::get_text(c.map).trim().to_string();
    if device.is_empty() || output.is_empty() {
        append_log("Set a source and an image path first.\r\n");
        return;
    }
    if mapfile.is_empty() {
        mapfile = Path::new(&output)
            .with_extension("map")
            .display()
            .to_string();
        win32::set_text(c.map, &mapfile);
    }
    let req = ImageRequest {
        device: PathBuf::from(device),
        output: PathBuf::from(output),
        mapfile: PathBuf::from(mapfile),
        pass: None,
        resume: win32::is_checked(c.resume),
        allow_system_disk: win32::is_checked(c.sysdisk),
    };
    spawn(Job::Image(req));
}

fn start_on_image(make: impl FnOnce(PathBuf) -> Job) {
    let Some(c) = controls() else { return };
    let image = win32::get_text(c.out).trim().to_string();
    if image.is_empty() {
        append_log("Set an image path first.\r\n");
        return;
    }
    spawn(make(PathBuf::from(image)));
}

/// Starts a job on a worker thread, unless one is already running.
fn spawn(job: Job) {
    if is_busy() {
        append_log("A job is already running.\r\n");
        return;
    }
    let Some(c) = controls() else { return };
    set_busy(true);
    win32::progress_pos(c.progress, 0);
    let hwnd = c.main as usize;
    std::thread::spawn(move || {
        let mut log = LogWriter { hwnd };
        let result = run_job(job, hwnd, &mut log);
        let (code, text) = match result {
            Ok(()) => (0, String::new()),
            Err(e) => (1, e.to_string()),
        };
        win32::post(
            hwnd as HWND,
            MSG_DONE,
            code,
            Box::into_raw(Box::new(text)) as LPARAM,
        );
    });
}

fn run_job(job: Job, hwnd: usize, log: &mut LogWriter) -> OpResult {
    match job {
        Job::Image(req) => ops::image(&req, log, Some(progress_hook(hwnd))),
        Job::Carve { image, validate } => {
            let dir = carved_dir(&image);
            ops::carve_image(&image, &dir, validate, log).map(|_| ())
        }
        Job::Verify { image } => {
            let dir = carved_dir(&image);
            ops::verify(&dir, &image, log)
        }
        Job::Analyze { image, extract } => {
            let dir = image.parent().unwrap_or(Path::new(".")).join("extracted");
            ops::analyze(&image, extract.then_some(dir.as_path()), log)
        }
    }
}

fn carved_dir(image: &Path) -> PathBuf {
    image.parent().unwrap_or(Path::new(".")).join("carved")
}

/// Posts progress percentages to the window, at most every 250 ms.
fn progress_hook(hwnd: usize) -> ProgressHook {
    let mut last = Instant::now() - Duration::from_secs(1);
    Box::new(move |t: &Totals| {
        if last.elapsed() < Duration::from_millis(250) {
            return;
        }
        last = Instant::now();
        let total = t.recovered + t.bad + t.non_trimmed + t.untested;
        let done = total - t.untested;
        let pct = done.saturating_mul(100).checked_div(total).unwrap_or(0) as usize;
        win32::post(hwnd as HWND, MSG_PROGRESS, pct, 0);
    })
}

fn finish_job(ok: bool, error: &str) {
    set_busy(false);
    if ok {
        append_log("Done.\r\n\r\n");
    } else {
        append_log(&format!("Error: {error}\r\n\r\n"));
    }
    if let Some(c) = controls() {
        win32::progress_pos(c.progress, if ok { 100 } else { 0 });
    }
}

/// Writes text to the log box. Posts each chunk to the UI thread.
struct LogWriter {
    hwnd: usize,
}

impl Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf).replace('\n', "\r\n");
        win32::post(
            self.hwnd as HWND,
            MSG_LOG,
            0,
            Box::into_raw(Box::new(text)) as LPARAM,
        );
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
