//! Writing to the clipboard, and an opt-in monitor for clipboard history.
use crate::wide;
use std::sync::mpsc;
use std::sync::OnceLock;
use windows::core::{Error, Result, PCWSTR};
use windows::Win32::Foundation::{
    GlobalFree, E_FAIL, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostThreadMessageW,
    RegisterClassW, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE, WM_QUIT,
    WNDCLASSW,
};

/// Other programs may hold the clipboard briefly, so retry opening it.
fn open_clipboard() -> Result<()> {
    let mut last = Error::from(E_FAIL);
    for _ in 0..10 {
        // SAFETY: no pointers involved.
        match unsafe { OpenClipboard(None) } {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Err(last)
}

/// Put text on the clipboard.
pub fn set_text(text: &str) -> Result<()> {
    let data = wide(text);
    open_clipboard()?;
    // SAFETY: the clipboard is open; the buffer is sized for `data`, and on success the
    // clipboard takes ownership of it, otherwise it is freed here.
    let result = unsafe {
        (|| {
            EmptyClipboard()?;
            let mem = GlobalAlloc(GMEM_MOVEABLE, data.len() * 2)?;
            let dst = GlobalLock(mem) as *mut u16;
            if dst.is_null() {
                let _ = GlobalFree(Some(mem));
                return Err(Error::from(E_FAIL));
            }
            std::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
            let _ = GlobalUnlock(mem);
            if let Err(e) = SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(mem.0))) {
                let _ = GlobalFree(Some(mem));
                return Err(e);
            }
            Ok(())
        })()
    };
    // SAFETY: the clipboard was opened above.
    let _ = unsafe { CloseClipboard() };
    result
}

/// Read clipboard text, unless the copying app marked it as private. Password managers use
/// these standard formats to keep secrets out of clipboard history.
fn read_shareable_text() -> Option<String> {
    let format = |name: &str| {
        let name = wide(name);
        // SAFETY: the name outlives the call.
        unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) }
    };
    // SAFETY: format checks take no pointers; data handles are only read while the clipboard
    // is open and locked, and the text is NUL-terminated by the clipboard contract.
    unsafe {
        for private in [
            "ExcludeClipboardContentFromMonitorProcessing",
            "Clipboard Viewer Ignore",
        ] {
            if IsClipboardFormatAvailable(format(private)).is_ok() {
                return None;
            }
        }
        open_clipboard().ok()?;
        let text = (|| {
            let allow = format("CanIncludeInClipboardHistory");
            if IsClipboardFormatAvailable(allow).is_ok() {
                let handle = GetClipboardData(allow).ok()?;
                let mem = HGLOBAL(handle.0);
                let flag = GlobalLock(mem) as *const u32;
                let allowed = !flag.is_null() && *flag != 0;
                let _ = GlobalUnlock(mem);
                if !allowed {
                    return None;
                }
            }
            let handle = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
            let mem = HGLOBAL(handle.0);
            let ptr = GlobalLock(mem) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let mut len = 0;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
            let _ = GlobalUnlock(mem);
            Some(text)
        })();
        let _ = CloseClipboard();
        text
    }
}

static SINK: OnceLock<std::sync::Mutex<Option<mpsc::Sender<String>>>> = OnceLock::new();

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_CLIPBOARDUPDATE {
        if let Some(text) = read_shareable_text() {
            if let Some(tx) = SINK.get().and_then(|s| s.lock().ok()?.clone()) {
                let _ = tx.send(text);
            }
        }
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, w, l)
}

/// Delivers copied text while alive. Nothing reads the clipboard once this is dropped.
pub struct Monitor {
    thread: u32,
}

impl Monitor {
    pub fn start() -> (Monitor, mpsc::Receiver<String>) {
        let (tx, rx) = mpsc::channel();
        *SINK
            .get_or_init(|| std::sync::Mutex::new(None))
            .lock()
            .unwrap() = Some(tx);
        let (id_tx, id_rx) = mpsc::channel();
        std::thread::spawn(move || message_loop(id_tx));
        (
            Monitor {
                thread: id_rx.recv().unwrap_or(0),
            },
            rx,
        )
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        // SAFETY: posting to a thread id is harmless if the thread has exited.
        let _ = unsafe { PostThreadMessageW(self.thread, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(sink) = SINK.get() {
            *sink.lock().unwrap() = None;
        }
    }
}

fn message_loop(id_tx: mpsc::Sender<u32>) {
    let class = wide("GlintClipboardMonitor");
    // SAFETY: the class name lives for the whole loop; the window handle is only used here,
    // and the window dies with this thread.
    unsafe {
        let _ = id_tx.send(GetCurrentThreadId());
        let module = GetModuleHandleW(None).unwrap_or_default();
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: module.into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        });
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class.as_ptr()),
            PCWSTR::null(),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(module.into()),
            None,
        ) else {
            return;
        };
        if AddClipboardFormatListener(hwnd).is_err() {
            return;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    }
}
