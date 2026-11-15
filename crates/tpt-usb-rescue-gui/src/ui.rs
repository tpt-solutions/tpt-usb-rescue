//! Window layout, messages and background jobs for the GUI.
//!
//! The window is a four-step guide. Each control has a tooltip, every destructive action asks
//! first, and every failure is explained in a dialog. Jobs run on worker threads and talk to the
//! window only through posted messages, so all control access stays on the UI thread.

use crate::win32::{self, HWND, LPARAM, LRESULT, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WPARAM};
use std::cell::{Cell, RefCell};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tpt_usb_rescue::ops::{self, ImageRequest, OpResult, STOPPED_MESSAGE};
use tpt_usb_rescue_hal::{DeviceInfo, list_devices};
use tpt_usb_rescue_imager::engine::ProgressHook;
use tpt_usb_rescue_imager::mapfile::Totals;

const CLASS_NAME: &str = "TptUsbRescueMainWindow";
const TITLE: &str = "tpt-usb-rescue: recover files from a failing USB stick";

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
const ID_STOP: usize = 124;
const ID_HELP: usize = 125;
const ID_PROGRESS: usize = 130;
const ID_LOG: usize = 131;

// Messages posted from worker threads. lparam carries a `Box<String>` where noted.
const MSG_LOG: u32 = WM_APP + 1;
const MSG_PROGRESS: u32 = WM_APP + 2; // wparam: percent 0..=100
const MSG_DONE: u32 = WM_APP + 3; // wparam: 0 ok, 1 failed, 2 stopped; lparam: summary or error
const MSG_STATUS: u32 = WM_APP + 4; // lparam: status line

const WINDOW_W: i32 = 860;
const WINDOW_H: i32 = 800;
const LEFT: i32 = 14;
const WIDTH: i32 = 820;

const HELP_TEXT: &str = "HOW TO COPY A FAILING USB STICK

Step 1 - Choose what to copy
  Pick your USB stick in \"Found disks\". Press Refresh after you plug it in.
  Disks need the program to run as Administrator. The top line shows whether it does.

Step 2 - Choose where to save the copy
  Type a file name or press Browse, for example D:\\rescue\\stick.img.
  Save the copy on a DIFFERENT disk from the stick.
  Progress notes are created for you. Keep them next to the copy.

Step 3 - Copy the disk
  Press \"Start copy\". The stick is only read. Its files are never changed.
  A copy can take hours. Press \"Stop\" at any time: your progress is saved.
  To carry on later, tick \"Continue a previous run\" and press \"Start copy\" again.

Step 4 - Find files in the copy (optional)
  \"List files\" shows the folders and file names Windows can read in the copy.
  \"Find files\" searches the copy for pictures, documents and other files,
  and saves them in a folder called \"carved\" next to the copy.
  \"Check found files\" confirms the saved files match the copy.

Tip: rest the mouse on any box or button to see what it does.";

#[derive(Clone, Copy)]
struct Controls {
    main: HWND,
    admin: HWND,
    src: HWND,
    dev: HWND,
    out: HWND,
    map: HWND,
    resume: HWND,
    sysdisk: HWND,
    validate: HWND,
    extract: HWND,
    start_btn: HWND,
    stop_btn: HWND,
    carve_btn: HWND,
    verify_btn: HWND,
    analyze_btn: HWND,
    refresh_btn: HWND,
    src_btn: HWND,
    out_btn: HWND,
    progress: HWND,
    status: HWND,
    log: HWND,
}

#[derive(Default)]
struct State {
    ctrl: Option<Controls>,
    devices: Vec<DeviceInfo>,
    cancel: Option<Arc<AtomicBool>>,
    job_title: &'static str,
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
        win32::dialog(
            std::ptr::null_mut(),
            "tpt-usb-rescue",
            "The program could not start (window class registration failed).",
            win32::MB_OK | win32::MB_ICONERROR,
        );
        return;
    }

    let main = win32::create_main_window(CLASS_NAME, TITLE, WINDOW_W, WINDOW_H);
    if main.is_null() {
        win32::dialog(
            std::ptr::null_mut(),
            "tpt-usb-rescue",
            "The program could not open its window.",
            win32::MB_OK | win32::MB_ICONERROR,
        );
        return;
    }
    build_controls(main);
    refresh_devices();
    show_admin_status();
    set_status("Ready. Start with Step 1: choose the disk or file to copy.");
    append_log(
        "Welcome! Follow the four steps from top to bottom. Press Help for a guide.\r\n\r\n",
    );
    win32::show(main);
    win32::run_message_loop(main);
}

fn build_controls(main: HWND) {
    use win32::{
        BS_AUTOCHECKBOX, BS_GROUPBOX, CBS_DROPDOWNLIST, CBS_HASSTRINGS, ES_AUTOHSCROLL,
        ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, WS_BORDER, WS_TABSTOP, WS_VSCROLL,
    };
    let tab = WS_TABSTOP;
    let tip = win32::create_tooltip(main);

    // Groups first, so they sit behind the controls inside them.
    let group = |title: &str, y: i32, h: i32| {
        win32::create_child("BUTTON", title, BS_GROUPBOX, (LEFT, y, WIDTH, h), main, 0)
    };
    group("Step 1: Choose what to copy", 42, 132);
    group("Step 2: Choose where to save the copy", 182, 132);
    group("Step 3: Copy the disk", 322, 122);
    group("Step 4: Find files in the copy (optional)", 452, 140);
    group("Activity log", 600, 176);

    let label = |text: &str, x: i32, y: i32, w: i32| {
        win32::create_child("STATIC", text, 0, (x, y + 3, w, 20), main, 0)
    };
    let edit = |x: i32, y: i32, w: i32| {
        win32::create_child(
            "EDIT",
            "",
            WS_BORDER | ES_AUTOHSCROLL | tab,
            (x, y, w, 22),
            main,
            0,
        )
    };
    let button = |text: &str, x: i32, y: i32, w: i32, h: i32, id: usize| {
        win32::create_child("BUTTON", text, WS_TABSTOP, (x, y, w, h), main, id)
    };

    // Administrator status and help, across the top.
    let admin = win32::create_child("STATIC", "", 0, (LEFT, 12, 640, 22), main, 0);
    let help_btn = button("Help: how do I use this?", 672, 8, 162, 28, ID_HELP);

    // Step 1.
    label("Disk or file:", 28, 70, 110);
    let src = edit(140, 68, 540);
    let src_btn = button("Browse file...", 690, 67, 130, 26, ID_SRC_BROWSE);
    label("Found disks:", 28, 102, 110);
    let dev = win32::create_child(
        "COMBOBOX",
        "",
        CBS_DROPDOWNLIST | CBS_HASSTRINGS | WS_VSCROLL | tab,
        (140, 100, 540, 300),
        main,
        ID_DEV,
    );
    let refresh_btn = button("Refresh", 690, 99, 130, 26, ID_DEV_REFRESH);
    label(
        "Tip: choose a disk from the list, or browse for a file to practise with. The original is never changed.",
        28,
        132,
        792,
    );

    // Step 2.
    label("Save copy as:", 28, 210, 110);
    let out = edit(140, 208, 540);
    let out_btn = button("Browse...", 690, 207, 130, 26, ID_OUT_BROWSE);
    label("Progress notes:", 28, 240, 110);
    let map = edit(140, 238, 540);
    let resume = win32::create_child(
        "BUTTON",
        "Continue a previous run",
        BS_AUTOCHECKBOX | tab,
        (140, 270, 240, 22),
        main,
        ID_RESUME,
    );
    let sysdisk = win32::create_child(
        "BUTTON",
        "Allow system disk (advanced, risky)",
        BS_AUTOCHECKBOX | tab,
        (400, 270, 320, 22),
        main,
        ID_SYSDISK,
    );
    label(
        "Progress notes remember which parts are already copied. Leave the box empty to create them automatically.",
        28,
        296,
        792,
    );

    // Step 3.
    let start_btn = button("Start copy", 28, 352, 170, 36, ID_IMAGE);
    let stop_btn = button("Stop", 210, 352, 110, 36, ID_STOP);
    let status = win32::create_child("STATIC", "", 0, (340, 360, 480, 22), main, 0);
    let progress = win32::create_child(
        "msctls_progress32",
        "",
        0,
        (28, 402, 792, 18),
        main,
        ID_PROGRESS,
    );
    win32::progress_range(progress, 100);

    // Step 4.
    let analyze_btn = button("List files in the copy", 28, 480, 200, 32, ID_ANALYZE);
    let carve_btn = button("Find files in the copy", 240, 480, 200, 32, ID_CARVE);
    let verify_btn = button("Check found files", 452, 480, 200, 32, ID_VERIFY);
    let validate = win32::create_child(
        "BUTTON",
        "Only keep files that look intact (recommended)",
        BS_AUTOCHECKBOX | tab,
        (28, 524, 400, 22),
        main,
        ID_VALIDATE,
    );
    let extract = win32::create_child(
        "BUTTON",
        "Also save the listed files to an \"extracted\" folder",
        BS_AUTOCHECKBOX | tab,
        (28, 552, 480, 22),
        main,
        ID_EXTRACT,
    );
    win32::set_check(validate, true);
    label(
        "Found files are saved in a \"carved\" folder next to your copy.",
        28,
        582,
        792,
    );

    // Activity log.
    let log = win32::create_child(
        "EDIT",
        "",
        WS_BORDER | WS_VSCROLL | ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY,
        (LEFT + 12, 624, WIDTH - 24, 140),
        main,
        ID_LOG,
    );

    // Tooltips: one sentence per control, in plain language.
    let t = |c: HWND, text: &str| win32::add_tooltip(tip, c, text);
    t(
        admin,
        "Physical disks can only be read when this program runs as Administrator.",
    );
    t(help_btn, "Show a step-by-step guide.");
    t(
        src,
        "The disk or file to copy. Choose a disk from the list below, or type a path. The original is never changed.",
    );
    t(
        src_btn,
        "Choose a regular file to copy instead of a disk. Handy for practice.",
    );
    t(
        dev,
        "Disks found on this computer. USB sticks usually appear here after you plug them in.",
    );
    t(
        refresh_btn,
        "Look for disks again. Use this after you plug in a stick.",
    );
    t(
        out,
        "Where the copy will be saved, for example D:\\rescue\\stick.img. Use a different disk from the stick.",
    );
    t(out_btn, "Pick a folder and a file name for the copy.");
    t(
        map,
        "A small file that remembers which parts have been copied. Keep it with the copy. Leave it empty to create it automatically.",
    );
    t(
        resume,
        "Tick this to carry on a copy that was stopped earlier. Use the same copy and progress notes.",
    );
    t(
        sysdisk,
        "Advanced. The disk that Windows runs from. Only tick this if you know exactly why you need it.",
    );
    t(
        start_btn,
        "Reads the whole disk and saves it to the copy file. The disk is only read. This can take hours.",
    );
    t(
        stop_btn,
        "Stop after the current part. Progress is saved, so you can carry on later.",
    );
    t(progress, "How far the copy has got.");
    t(
        analyze_btn,
        "Show the folders and file names Windows can read inside the copy.",
    );
    t(
        carve_btn,
        "Search the copy for pictures, documents and other files, and save them in a \"carved\" folder.",
    );
    t(
        verify_btn,
        "Check that the files in the \"carved\" folder match the copy.",
    );
    t(
        validate,
        "Skip files that look like a match by name but are damaged. Recommended.",
    );
    t(
        extract,
        "When listing files, also save copies of them in an \"extracted\" folder.",
    );

    win32::enable(stop_btn, false);
    STATE.with(|s| {
        s.borrow_mut().ctrl = Some(Controls {
            main,
            admin,
            src,
            dev,
            out,
            map,
            resume,
            sysdisk,
            validate,
            extract,
            start_btn,
            stop_btn,
            carve_btn,
            verify_btn,
            analyze_btn,
            refresh_btn,
            src_btn,
            out_btn,
            progress,
            status,
            log,
        })
    });
}

fn show_admin_status() {
    let Some(c) = controls() else { return };
    let text = if win32::is_administrator() {
        "Administrator: yes. Disks can be read."
    } else {
        "Administrator: NO. Disks may not appear. Right-click the program and choose \"Run as administrator\"."
    };
    win32::set_text(c.admin, text);
}

/// Reloads the device list into the combo box.
fn refresh_devices() {
    let Some(c) = controls() else { return };
    win32::combo_reset(c.dev);
    match list_devices() {
        Ok(devices) => {
            if devices.is_empty() {
                append_log(
                    "No disks were found. Plug in your stick, press Refresh, and check the Administrator line at the top.\r\n",
                );
            }
            for d in &devices {
                win32::combo_add(
                    c.dev,
                    &format!("{}  ({})", d.path.display(), ops::human_bytes(d.size_bytes)),
                );
            }
            STATE.with(|s| s.borrow_mut().devices = devices);
        }
        Err(e) => append_log(&format!("Could not list disks: {e}\r\n")),
    }
}

fn append_log(text: &str) {
    if let Some(c) = controls() {
        win32::append_text(c.log, text);
    }
}

fn set_status(text: &str) {
    if let Some(c) = controls() {
        win32::set_text(c.status, text);
    }
}

fn set_busy(busy: bool) {
    BUSY.with(|b| b.set(busy));
    if let Some(c) = controls() {
        for h in [
            c.start_btn,
            c.carve_btn,
            c.verify_btn,
            c.analyze_btn,
            c.refresh_btn,
            c.src_btn,
            c.out_btn,
            c.resume,
            c.sysdisk,
            c.dev,
        ] {
            win32::enable(h, !busy);
        }
        win32::enable(c.stop_btn, busy);
    }
}

fn is_busy() -> bool {
    BUSY.with(|b| b.get())
}

/// Takes ownership of a String that a worker thread posted with `Box::into_raw`.
fn take_string(lp: LPARAM) -> String {
    // SAFETY: every MSG_LOG / MSG_STATUS / MSG_DONE is posted with a pointer from Box::into_raw on a
    // String, and each message is delivered exactly once, so reclaiming the box here is the only owner.
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
        MSG_STATUS => {
            let text = take_string(lp);
            set_status(&text);
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
            finish_job(wp, &text);
            0
        }
        WM_CLOSE => {
            if is_busy() {
                let choice = win32::dialog(
                    hwnd,
                    "Copy still running",
                    "A copy is still running.\n\nClosing now stops it. Your progress is saved, and you can carry on later with \"Continue a previous run\".\n\nClose anyway?",
                    win32::MB_YESNO | win32::MB_ICONWARNING,
                );
                if choice != win32::IDYES {
                    return 0;
                }
                request_stop();
            }
            win32::def_proc(hwnd, msg, wp, lp)
        }
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
            ID_HELP => {
                win32::dialog(
                    c.main,
                    "How to use tpt-usb-rescue",
                    HELP_TEXT,
                    win32::MB_OK | win32::MB_ICONINFORMATION,
                );
            }
            ID_DEV_REFRESH => {
                refresh_devices();
                set_status("Disk list refreshed.");
            }
            ID_SRC_BROWSE => {
                let filter = "All files (*.*)\0*.*\0\0";
                if let Some(p) = win32::file_dialog(c.main, false, filter, "") {
                    win32::set_text(c.src, &p);
                    set_status("Step 1 done. Next: choose where to save the copy (Step 2).");
                }
            }
            ID_OUT_BROWSE => {
                let filter = "Disk image (*.img)\0*.img\0All files (*.*)\0*.*\0\0";
                if let Some(p) = win32::file_dialog(c.main, true, filter, "img") {
                    win32::set_text(c.out, &p);
                    set_status("Step 2 done. Next: press \"Start copy\" (Step 3).");
                }
            }
            ID_IMAGE => start_copy(),
            ID_STOP => request_stop(),
            ID_CARVE => start_on_copy("find files", |image| Job::Carve {
                image,
                validate: win32::is_checked(c.validate),
            }),
            ID_VERIFY => start_on_copy("check found files", |image| Job::Verify { image }),
            ID_ANALYZE => start_on_copy("list files", |image| Job::Analyze {
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
            set_status("Step 1 done. Next: choose where to save the copy (Step 2).");
        }
    }
}

enum Job {
    Image(ImageRequest),
    Carve { image: PathBuf, validate: bool },
    Verify { image: PathBuf },
    Analyze { image: PathBuf, extract: bool },
}

fn job_title(job: &Job) -> &'static str {
    match job {
        Job::Image(_) => "Copy",
        Job::Carve { .. } => "Find files",
        Job::Verify { .. } => "Check found files",
        Job::Analyze { .. } => "List files",
    }
}

fn start_copy() {
    let Some(c) = controls() else { return };
    let source = win32::get_text(c.src).trim().to_string();
    let output = win32::get_text(c.out).trim().to_string();
    let mut mapfile = win32::get_text(c.map).trim().to_string();
    let resume = win32::is_checked(c.resume);

    if source.is_empty() {
        warn(
            "Step 1 is not done yet.\n\nChoose the disk or file to copy: pick a disk from \"Found disks\", or browse for a file.",
        );
        return;
    }
    if output.is_empty() {
        warn(
            "Step 2 is not done yet.\n\nChoose where to save the copy. Type a file name, for example D:\\rescue\\stick.img, or press Browse.",
        );
        return;
    }
    if mapfile.is_empty() {
        mapfile = Path::new(&output)
            .with_extension("map")
            .display()
            .to_string();
        win32::set_text(c.map, &mapfile);
    }

    if !Path::new(&source).is_file() {
        let size = STATE
            .with(|s| {
                s.borrow()
                    .devices
                    .iter()
                    .find(|d| d.path.display().to_string() == source)
                    .map(|d| ops::human_bytes(d.size_bytes))
            })
            .unwrap_or_else(|| "unknown size".to_string());
        let text = format!(
            "You are about to copy the whole disk {source} ({size}).\n\nThe disk is only read. Its files are not changed.\nThe copy will be saved to:\n{output}\n\nThis can take hours. You can press Stop at any time.\n\nStart copying now?"
        );
        if win32::dialog(
            c.main,
            "Start copy?",
            &text,
            win32::MB_YESNO | win32::MB_ICONQUESTION,
        ) != win32::IDYES
        {
            set_status("Copy cancelled. Nothing was changed.");
            return;
        }
        if !win32::is_administrator() {
            let text = "This program is not running as Administrator, so Windows may refuse to read the disk.\n\nContinue anyway?";
            if win32::dialog(
                c.main,
                "Not Administrator",
                text,
                win32::MB_YESNO | win32::MB_ICONWARNING,
            ) != win32::IDYES
            {
                set_status("Copy cancelled. Run the program as Administrator, then try again.");
                return;
            }
        }
    }
    if !resume && Path::new(&output).exists() {
        let text = format!(
            "A file already exists at:\n{output}\n\nStarting a new copy will replace it. To keep it, choose a different name, or tick \"Continue a previous run\".\n\nReplace it?"
        );
        if win32::dialog(
            c.main,
            "Replace existing copy?",
            &text,
            win32::MB_YESNO | win32::MB_ICONWARNING,
        ) != win32::IDYES
        {
            set_status("Copy cancelled. The existing file was kept.");
            return;
        }
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let req = ImageRequest {
        device: PathBuf::from(source),
        output: PathBuf::from(output),
        mapfile: PathBuf::from(mapfile),
        pass: None,
        resume,
        allow_system_disk: win32::is_checked(c.sysdisk),
        tuning: ops::Tuning::default(),
        cancel: Some(Arc::clone(&cancel)),
    };
    spawn(Job::Image(req), Some(cancel));
}

fn start_on_copy(action: &str, make: impl FnOnce(PathBuf) -> Job) {
    let Some(c) = controls() else { return };
    let image = win32::get_text(c.out).trim().to_string();
    if image.is_empty() || !Path::new(&image).is_file() {
        warn(&format!(
            "Cannot {action} yet: the copy file was not found.\n\nMake a copy in Step 3 first, or choose an existing copy in Step 2 (\"Save copy as\")."
        ));
        return;
    }
    spawn(make(PathBuf::from(image)), None);
}

fn warn(text: &str) {
    if let Some(c) = controls() {
        win32::dialog(
            c.main,
            "tpt-usb-rescue",
            text,
            win32::MB_OK | win32::MB_ICONWARNING,
        );
    }
}

fn request_stop() {
    let flag = STATE.with(|s| s.borrow().cancel.clone());
    if let Some(flag) = flag {
        flag.store(true, Ordering::SeqCst);
        set_status("Stopping after the current part... progress is being saved.");
    }
}

/// Starts a job on a worker thread, unless one is already running.
fn spawn(job: Job, cancel: Option<Arc<AtomicBool>>) {
    if is_busy() {
        warn("A job is already running. Wait for it to finish, or press Stop.");
        return;
    }
    let Some(c) = controls() else { return };
    let title = job_title(&job);
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.cancel = cancel;
        s.job_title = title;
    });
    set_busy(true);
    win32::progress_pos(c.progress, 0);
    set_status(&format!("{title} is running..."));
    append_log(&format!("--- {title} started ---\r\n"));
    let hwnd = c.main as usize;
    std::thread::spawn(move || {
        let mut log = LogWriter { hwnd };
        let (code, text) = match run_job(job, hwnd, &mut log) {
            Ok(summary) => (0, summary),
            Err(e) => {
                let msg = e.to_string();
                if msg == STOPPED_MESSAGE {
                    (2, msg)
                } else {
                    (1, msg)
                }
            }
        };
        win32::post(
            hwnd as HWND,
            MSG_DONE,
            code,
            Box::into_raw(Box::new(text)) as LPARAM,
        );
    });
}

fn run_job(job: Job, hwnd: usize, log: &mut LogWriter) -> OpResult<String> {
    match job {
        Job::Image(req) => {
            let output = req.output.clone();
            ops::image(&req, log, Some(progress_hook(hwnd)))?;
            Ok(format!(
                "The copy is finished and saved to:\n{}\n\nNext, Step 4 can list or find files inside it.",
                output.display()
            ))
        }
        Job::Carve { image, validate } => {
            let dir = carved_dir(&image);
            let count = ops::carve_image(&image, &dir, validate, log)?;
            Ok(if count == 0 {
                "No files were found in the copy.\n\nTry turning off \"Only keep files that look intact\" in Step 4 to see everything that was found.".to_string()
            } else {
                format!(
                    "Found {count} file{}.\n\nThey are saved in:\n{}",
                    if count == 1 { "" } else { "s" },
                    dir.display()
                )
            })
        }
        Job::Verify { image } => {
            let dir = carved_dir(&image);
            ops::verify(&dir, &image, log)?;
            Ok(format!(
                "Every saved file matches the copy.\n\nChecked folder:\n{}",
                dir.display()
            ))
        }
        Job::Analyze { image, extract } => {
            let dir = image.parent().unwrap_or(Path::new(".")).join("extracted");
            ops::analyze(&image, extract.then_some(dir.as_path()), log)?;
            Ok(if extract {
                format!(
                    "The file list is in the activity log.\n\nExtracted files are in:\n{}",
                    dir.display()
                )
            } else {
                "The file list is in the activity log.".to_string()
            })
        }
    }
}

fn carved_dir(image: &Path) -> PathBuf {
    image.parent().unwrap_or(Path::new(".")).join("carved")
}

/// Posts progress to the window at most every 250 ms: a percentage and a readable status line.
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
        let status = format!(
            "Copying... {pct}% done. Saved: {}. Problem areas: {}.",
            ops::human_bytes(t.recovered),
            ops::human_bytes(t.bad + t.non_trimmed),
        );
        win32::post(hwnd as HWND, MSG_PROGRESS, pct, 0);
        win32::post(
            hwnd as HWND,
            MSG_STATUS,
            0,
            Box::into_raw(Box::new(status)) as LPARAM,
        );
    })
}

fn finish_job(code: WPARAM, text: &str) {
    set_busy(false);
    let title = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.cancel = None;
        s.job_title
    });
    let Some(c) = controls() else { return };
    match code {
        0 => {
            win32::progress_pos(c.progress, 100);
            set_status(&format!("{title}: finished."));
            append_log("Finished.\r\n\r\n");
            win32::dialog(
                c.main,
                &format!("{title}: finished"),
                text,
                win32::MB_OK | win32::MB_ICONINFORMATION,
            );
        }
        2 => {
            set_status("Stopped. Your progress is saved.");
            append_log("Stopped by you. Progress is saved.\r\n\r\n");
            let msg = "The copy was stopped.\n\nYour progress is saved. To carry on, tick \"Continue a previous run\" and press \"Start copy\" again.";
            win32::dialog(
                c.main,
                "Copy stopped",
                msg,
                win32::MB_OK | win32::MB_ICONINFORMATION,
            );
        }
        _ => {
            set_status(&format!("{title}: something went wrong. See the message."));
            append_log(&format!("Problem: {text}\r\n\r\n"));
            let mut msg = format!("{title} did not finish.\n\nWhat went wrong:\n{text}\n");
            let lower = text.to_lowercase();
            if lower.contains("denied") || lower.contains("permission") {
                msg.push_str("\nTip: close the program and run it as Administrator.");
            } else if text.contains("already records progress") {
                msg.push_str("\nTip: tick \"Continue a previous run\" to carry on, or delete the progress notes to start over.");
            }
            win32::dialog(
                c.main,
                &format!("{title}: problem"),
                &msg,
                win32::MB_OK | win32::MB_ICONERROR,
            );
        }
    }
    win32::progress_pos(c.progress, if code == 0 { 100 } else { 0 });
}

/// Writes text to the activity log. Posts each chunk to the UI thread.
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
