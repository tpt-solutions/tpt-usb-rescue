//! Minimal Win32 bindings and safe helpers. Only the calls the GUI needs are declared.
//!
//! Every raw call is wrapped in a helper that documents why the arguments are valid.

#![allow(non_snake_case, non_camel_case_types, clippy::upper_case_acronyms)]

use std::ffi::c_void;

pub type HWND = *mut c_void;
pub type HINSTANCE = *mut c_void;
pub type HMENU = *mut c_void;
pub type HICON = *mut c_void;
pub type HCURSOR = *mut c_void;
pub type HBRUSH = *mut c_void;
pub type HGDIOBJ = *mut c_void;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;
pub type WNDPROC = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

// --- constants ------------------------------------------------------------------------------

pub const WM_DESTROY: u32 = 0x0002;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_SETFONT: u32 = 0x0030;
pub const WM_COMMAND: u32 = 0x0111;
pub const WM_APP: u32 = 0x8000;
pub const WM_USER: u32 = 0x0400;

pub const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
pub const WS_CHILD: u32 = 0x4000_0000;
pub const WS_VISIBLE: u32 = 0x1000_0000;
pub const WS_TABSTOP: u32 = 0x0001_0000;
pub const WS_BORDER: u32 = 0x0080_0000;
pub const WS_VSCROLL: u32 = 0x0020_0000;
pub const BS_AUTOCHECKBOX: u32 = 0x0000_0003;
pub const ES_AUTOHSCROLL: u32 = 0x0000_0080;
pub const ES_MULTILINE: u32 = 0x0000_0004;
pub const ES_AUTOVSCROLL: u32 = 0x0000_0040;
pub const ES_READONLY: u32 = 0x0000_0800;
pub const CBS_DROPDOWNLIST: u32 = 0x0000_0003;
pub const CBS_HASSTRINGS: u32 = 0x0000_0200;
pub const CW_USEDEFAULT: i32 = i32::MIN;
pub const SW_SHOW: i32 = 5;
pub const COLOR_BTNFACE: usize = 15;
pub const IDC_ARROW: usize = 32512;
pub const DEFAULT_GUI_FONT: i32 = 17;
pub const ICC_PROGRESS_CLASS: u32 = 0x0000_0020;
pub const OFN_FILEMUSTEXIST: u32 = 0x0000_1000;
pub const OFN_PATHMUSTEXIST: u32 = 0x0000_0800;
pub const OFN_OVERWRITEPROMPT: u32 = 0x0000_0002;
pub const OFN_NOCHANGEDIR: u32 = 0x0000_0008;
pub const MB_OK: u32 = 0x0000_0000;
pub const MB_ICONERROR: u32 = 0x0000_0010;

pub const BM_GETCHECK: u32 = 0x00F0;
pub const BM_SETCHECK: u32 = 0x00F1;
pub const BST_CHECKED: usize = 1;
pub const CB_ADDSTRING: u32 = 0x0143;
pub const CB_RESETCONTENT: u32 = 0x014B;
pub const CB_GETCURSEL: u32 = 0x0147;
pub const CB_GETLBTEXT: u32 = 0x0148;
pub const CB_GETLBTEXTLEN: u32 = 0x0149;
pub const PBM_SETRANGE32: u32 = WM_USER + 6;
pub const PBM_SETPOS: u32 = WM_USER + 2;
pub const EM_SETSEL: u32 = 0x00B1;
pub const EM_REPLACESEL: u32 = 0x00C2;

pub const BN_CLICKED: u32 = 0;
pub const CBN_SELCHANGE: u32 = 1;

// --- structures -----------------------------------------------------------------------------

#[repr(C)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: u32,
    pub wparam: WPARAM,
    pub lparam: LPARAM,
    pub time: u32,
    pub pt: POINT,
}

#[repr(C)]
pub struct WNDCLASSEXW {
    pub cbSize: u32,
    pub style: u32,
    pub lpfnWndProc: WNDPROC,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HINSTANCE,
    pub hIcon: HICON,
    pub hCursor: HCURSOR,
    pub hbrBackground: HBRUSH,
    pub lpszMenuName: *const u16,
    pub lpszClassName: *const u16,
    pub hIconSm: HICON,
}

#[repr(C)]
pub struct OPENFILENAMEW {
    pub lStructSize: u32,
    pub hwndOwner: HWND,
    pub hInstance: HINSTANCE,
    pub lpstrFilter: *const u16,
    pub lpstrCustomFilter: *mut u16,
    pub nMaxCustFilter: u32,
    pub nFilterIndex: u32,
    pub lpstrFile: *mut u16,
    pub nMaxFile: u32,
    pub lpstrFileTitle: *mut u16,
    pub nMaxFileTitle: u32,
    pub lpstrInitialDir: *const u16,
    pub lpstrTitle: *const u16,
    pub Flags: u32,
    pub nFileOffset: u16,
    pub nFileExtension: u16,
    pub lpstrDefExt: *const u16,
    pub lCustData: isize,
    pub lpfnHook: *const c_void,
    pub lpTemplateName: *const u16,
    pub pvReserved: *mut c_void,
    pub dwReserved: u32,
    pub FlagsEx: u32,
}

#[repr(C)]
struct INITCOMMONCONTROLSEX {
    dwSize: u32,
    dwICC: u32,
}

// --- foreign functions ----------------------------------------------------------------------

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassExW(class: *const WNDCLASSEXW) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: HWND,
        menu: HMENU,
        instance: HINSTANCE,
        param: *const c_void,
    ) -> HWND;
    fn DefWindowProcW(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT;
    fn ShowWindow(hwnd: HWND, cmd: i32) -> i32;
    fn UpdateWindow(hwnd: HWND) -> i32;
    fn GetMessageW(msg: *mut MSG, hwnd: HWND, min: u32, max: u32) -> i32;
    fn IsDialogMessageW(hwnd: HWND, msg: *mut MSG) -> i32;
    fn TranslateMessage(msg: *const MSG) -> i32;
    fn DispatchMessageW(msg: *const MSG) -> LRESULT;
    fn PostQuitMessage(code: i32);
    fn PostMessageW(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> i32;
    fn SendMessageW(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT;
    fn GetWindowTextW(hwnd: HWND, buf: *mut u16, max: i32) -> i32;
    fn GetWindowTextLengthW(hwnd: HWND) -> i32;
    fn SetWindowTextW(hwnd: HWND, text: *const u16) -> i32;
    fn EnableWindow(hwnd: HWND, enable: i32) -> i32;
    fn LoadCursorW(instance: HINSTANCE, name: *const u16) -> HCURSOR;
    fn MessageBoxW(hwnd: HWND, text: *const u16, caption: *const u16, ty: u32) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn GetStockObject(obj: i32) -> HGDIOBJ;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> HINSTANCE;
}

#[link(name = "comctl32")]
unsafe extern "system" {
    fn InitCommonControlsEx(icc: *const INITCOMMONCONTROLSEX) -> i32;
}

#[link(name = "comdlg32")]
unsafe extern "system" {
    fn GetOpenFileNameW(ofn: *mut OPENFILENAMEW) -> i32;
    fn GetSaveFileNameW(ofn: *mut OPENFILENAMEW) -> i32;
}

// --- helpers --------------------------------------------------------------------------------

/// UTF-16, NUL-terminated.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Message-box helper for fatal startup problems.
pub fn message_box(title: &str, text: &str) {
    let t = wide(title);
    let m = wide(text);
    // SAFETY: both buffers are NUL-terminated UTF-16 and live until the call returns; a null owner
    // window makes the box application-modal to nothing in particular.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            m.as_ptr(),
            t.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

pub fn module_handle() -> HINSTANCE {
    // SAFETY: a null name asks for the handle of the calling executable; no pointers are passed.
    unsafe { GetModuleHandleW(std::ptr::null()) }
}

pub fn init_common_controls() {
    let icc = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_PROGRESS_CLASS,
    };
    // SAFETY: `icc` is a fully initialised struct of the size declared in dwSize.
    unsafe {
        InitCommonControlsEx(&icc);
    }
}

pub fn default_font() -> HGDIOBJ {
    // SAFETY: GetStockObject takes a plain integer id and returns a shared system object.
    unsafe { GetStockObject(DEFAULT_GUI_FONT) }
}

pub fn arrow_cursor() -> HCURSOR {
    // SAFETY: IDC_ARROW is an integer resource id passed as a pointer-sized value, which is how
    // the system expects predefined cursors to be named (MAKEINTRESOURCE).
    unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW as *const u16) }
}

/// Registers a window class. Returns the class atom.
pub fn register_class(class: &WNDCLASSEXW) -> u16 {
    // SAFETY: `class` points to a fully initialised WNDCLASSEXW whose string pointers outlive the call.
    unsafe { RegisterClassExW(class) }
}

/// Creates a child window with the default GUI font.
#[allow(clippy::too_many_arguments)]
pub fn create_child(
    class: &str,
    text: &str,
    style: u32,
    rect: (i32, i32, i32, i32),
    parent: HWND,
    id: usize,
) -> HWND {
    let c = wide(class);
    let t = wide(text);
    // SAFETY: class and text are NUL-terminated UTF-16 buffers that live for the call; `parent` is a
    // window created by this process; the control id is passed through the menu-handle slot as the
    // Win32 API requires for child windows.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            c.as_ptr(),
            t.as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            rect.0,
            rect.1,
            rect.2,
            rect.3,
            parent,
            id as HMENU,
            module_handle(),
            std::ptr::null(),
        )
    };
    set_font(hwnd);
    hwnd
}

/// Creates the top-level window. Returns null on failure.
pub fn create_main_window(class: &str, title: &str, width: i32, height: i32) -> HWND {
    let c = wide(class);
    let t = wide(title);
    // SAFETY: class and title are NUL-terminated UTF-16 buffers alive for the call; a null parent
    // creates a top-level window; no creation parameter is needed.
    unsafe {
        CreateWindowExW(
            0,
            c.as_ptr(),
            t.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            width,
            height,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            module_handle(),
            std::ptr::null(),
        )
    }
}

pub fn show(hwnd: HWND) {
    // SAFETY: hwnd is a live window handle created by this process.
    unsafe {
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
    }
}

pub fn set_font(hwnd: HWND) {
    // SAFETY: hwnd is a live control; the font is a stock object that is never freed.
    unsafe {
        SendMessageW(hwnd, WM_SETFONT, default_font() as usize, 1);
    }
}

/// Pumps messages until the window closes.
pub fn run_message_loop(main: HWND) {
    let mut msg = MSG {
        hwnd: std::ptr::null_mut(),
        message: 0,
        wparam: 0,
        lparam: 0,
        time: 0,
        pt: POINT { x: 0, y: 0 },
    };
    loop {
        // SAFETY: `msg` is a valid, writable MSG; a null filter receives all messages for this thread.
        let r = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
        if r <= 0 {
            break;
        }
        // SAFETY: `msg` was just filled in by GetMessageW; `main` is the top-level window.
        unsafe {
            if IsDialogMessageW(main, &mut msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

pub fn post(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) {
    // SAFETY: posting to a window handle is safe even if the window has since been destroyed;
    // the call then fails and returns zero.
    unsafe {
        PostMessageW(hwnd, msg, wp, lp);
    }
}

pub fn send(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: caller passes a live control and a message whose lparam matches its documented type.
    unsafe { SendMessageW(hwnd, msg, wp, lp) }
}

pub fn quit(code: i32) {
    // SAFETY: PostQuitMessage only enqueues WM_QUIT for the calling thread.
    unsafe { PostQuitMessage(code) }
}

pub fn def_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: forwards the arguments unchanged to the default handler, as the window procedure contract requires.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

pub fn get_text(hwnd: HWND) -> String {
    // SAFETY: hwnd is a live control; the buffer is sized from GetWindowTextLengthW plus the terminator.
    unsafe {
        let len = GetWindowTextLengthW(hwnd).max(0) as usize;
        let mut buf = vec![0u16; len + 1];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32).max(0) as usize;
        String::from_utf16_lossy(&buf[..n])
    }
}

pub fn set_text(hwnd: HWND, text: &str) {
    let w = wide(text);
    // SAFETY: `w` is NUL-terminated and alive for the call; hwnd is a live control.
    unsafe {
        SetWindowTextW(hwnd, w.as_ptr());
    }
}

pub fn enable(hwnd: HWND, on: bool) {
    // SAFETY: hwnd is a live control.
    unsafe {
        EnableWindow(hwnd, on as i32);
    }
}

pub fn is_checked(hwnd: HWND) -> bool {
    send(hwnd, BM_GETCHECK, 0, 0) as usize == BST_CHECKED
}

pub fn combo_reset(hwnd: HWND) {
    send(hwnd, CB_RESETCONTENT, 0, 0);
}

pub fn combo_add(hwnd: HWND, text: &str) {
    let w = wide(text);
    // SAFETY: `w` is NUL-terminated and alive while the message is processed (SendMessage is synchronous).
    unsafe {
        SendMessageW(hwnd, CB_ADDSTRING, 0, w.as_ptr() as isize);
    }
}

pub fn combo_selected_text(hwnd: HWND) -> Option<String> {
    let idx = send(hwnd, CB_GETCURSEL, 0, 0);
    if idx < 0 {
        return None;
    }
    let len = send(hwnd, CB_GETLBTEXTLEN, idx as usize, 0);
    if len <= 0 {
        return None;
    }
    let mut buf = vec![0u16; len as usize + 1];
    // SAFETY: the buffer holds the item text plus the terminator, as CB_GETLBTEXT requires.
    unsafe {
        SendMessageW(hwnd, CB_GETLBTEXT, idx as usize, buf.as_mut_ptr() as isize);
    }
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

pub fn progress_range(hwnd: HWND, max: usize) {
    send(hwnd, PBM_SETRANGE32, 0, max as isize);
}

pub fn progress_pos(hwnd: HWND, pos: usize) {
    send(hwnd, PBM_SETPOS, pos, 0);
}

/// Appends text to a read-only multi-line edit box.
pub fn append_text(hwnd: HWND, text: &str) {
    let len = get_text_len(hwnd);
    send(hwnd, EM_SETSEL, len as usize, len as isize);
    let w = wide(text);
    // SAFETY: EM_REPLACESEL reads the NUL-terminated buffer during the synchronous send.
    unsafe {
        SendMessageW(hwnd, EM_REPLACESEL, 0, w.as_ptr() as isize);
    }
}

fn get_text_len(hwnd: HWND) -> i32 {
    // SAFETY: hwnd is a live window handle.
    unsafe { GetWindowTextLengthW(hwnd) }
}

/// Shows a standard open or save dialog. Returns the chosen path.
pub fn file_dialog(owner: HWND, save: bool, filter: &str, default_ext: &str) -> Option<String> {
    let filter_w: Vec<u16> = filter.encode_utf16().chain(std::iter::once(0)).collect();
    let ext_w = wide(default_ext);
    let mut file = vec![0u16; 4096];
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        hInstance: std::ptr::null_mut(),
        lpstrFilter: filter_w.as_ptr(),
        lpstrCustomFilter: std::ptr::null_mut(),
        nMaxCustFilter: 0,
        nFilterIndex: 1,
        lpstrFile: file.as_mut_ptr(),
        nMaxFile: file.len() as u32,
        lpstrFileTitle: std::ptr::null_mut(),
        nMaxFileTitle: 0,
        lpstrInitialDir: std::ptr::null(),
        lpstrTitle: std::ptr::null(),
        Flags: OFN_NOCHANGEDIR
            | if save {
                OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST
            } else {
                OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST
            },
        nFileOffset: 0,
        nFileExtension: 0,
        lpstrDefExt: ext_w.as_ptr(),
        lCustData: 0,
        lpfnHook: std::ptr::null(),
        lpTemplateName: std::ptr::null(),
        pvReserved: std::ptr::null_mut(),
        dwReserved: 0,
        FlagsEx: 0,
    };
    // SAFETY: `ofn` and every buffer it points to (filter, default extension, file) live until the
    // dialog returns; the struct size is set as the API requires.
    let ok = unsafe {
        if save {
            GetSaveFileNameW(&mut ofn)
        } else {
            GetOpenFileNameW(&mut ofn)
        }
    };
    if ok == 0 {
        return None;
    }
    let end = file.iter().position(|&c| c == 0).unwrap_or(file.len());
    Some(String::from_utf16_lossy(&file[..end]))
}
