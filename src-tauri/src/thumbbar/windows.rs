//! UI-thread-only COM/subclass ownership. Never move ITaskbarList3 across apartments.
use super::*;
use ::windows::Win32::{
    Foundation::HWND as ComHwnd,
    System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    },
    UI::Shell::{ITaskbarList3, THUMBBUTTON, THUMBBUTTONFLAGS, THUMBBUTTONMASK, TaskbarList},
};
use std::{cell::RefCell, rc::Rc};
use tauri::{AppHandle, Manager, WebviewWindow};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPV5HEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
        DeleteObject, LCS_GM_IMAGES,
    },
    UI::{
        Shell::{DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            CreateIconIndirect, DestroyIcon, HICON, ICONINFO, RegisterWindowMessageW, WM_COMMAND,
            WM_NCDESTROY, WM_SHOWWINDOW,
        },
    },
};
const SUBCLASS: usize = 0x534e4f57;
struct Apartment(bool);
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}
struct Toolbar {
    taskbar: Option<ITaskbarList3>,
    state: State,
}
struct Context {
    toolbar: RefCell<Toolbar>,
    app: AppHandle,
    taskbar_created: u32,
    // Drops last, after COM interfaces.
    _apartment: Apartment,
}
struct NativeIcon(HICON);
impl Drop for NativeIcon {
    fn drop(&mut self) {
        unsafe {
            DestroyIcon(self.0);
        }
    }
}
fn icon(which: Icon) -> Option<NativeIcon> {
    let png: &[u8] = match which {
        Icon::Prev => include_bytes!("../../../assets/thumbbar/prev.png"),
        Icon::Play => include_bytes!("../../../assets/thumbbar/play.png"),
        Icon::Pause => include_bytes!("../../../assets/thumbbar/pause.png"),
        Icon::Next => include_bytes!("../../../assets/thumbbar/next.png"),
    };
    let image = tauri::image::Image::from_bytes(png).ok()?;
    let (w, h) = (image.width() as usize, image.height() as usize);
    unsafe {
        let info = BITMAPV5HEADER {
            bV5Size: size_of::<BITMAPV5HEADER>() as u32,
            bV5Width: w as i32,
            bV5Height: -(h as i32),
            bV5Planes: 1,
            bV5BitCount: 32,
            bV5Compression: BI_RGB,
            bV5RedMask: 0x00ff0000,
            bV5GreenMask: 0x0000ff00,
            bV5BlueMask: 0x000000ff,
            bV5AlphaMask: 0xff000000,
            bV5CSType: 0x57696e20, // LCS_WINDOWS_COLOR_SPACE ("Win ")
            bV5Intent: LCS_GM_IMAGES as u32,
            ..std::mem::zeroed()
        };
        let mut bits = std::ptr::null_mut();
        let color = CreateDIBSection(
            std::ptr::null_mut(),
            (&info as *const BITMAPV5HEADER).cast::<BITMAPINFO>(),
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        if color.is_null() {
            return None;
        }
        let pixels = std::slice::from_raw_parts_mut(bits.cast::<u8>(), w * h * 4);
        let stride = w.div_ceil(16) * 2;
        let has_alpha = image.rgba().chunks_exact(4).any(|p| p[3] != 0);
        let mask = vec![if has_alpha { 0 } else { 0xff }; stride * h];
        for (src, dst) in image.rgba().chunks_exact(4).zip(pixels.chunks_exact_mut(4)) {
            let a = src[3] as u16;
            // Chromium IconUtil: premultiplied BGRA plus an all-opaque AND mask
            // (all transparent only if every alpha value is zero).
            dst.copy_from_slice(&[
                ((src[2] as u16 * a + 127) / 255) as u8,
                ((src[1] as u16 * a + 127) / 255) as u8,
                ((src[0] as u16 * a + 127) / 255) as u8,
                src[3],
            ]);
        }
        let mono = CreateBitmap(w as i32, h as i32, 1, 1, mask.as_ptr().cast());
        let native = if mono.is_null() {
            std::ptr::null_mut()
        } else {
            CreateIconIndirect(&ICONINFO {
                fIcon: 1,
                xHotspot: 0,
                yHotspot: 0,
                hbmMask: mono,
                hbmColor: color,
            })
        };
        DeleteObject(color);
        if !mono.is_null() {
            DeleteObject(mono);
        }
        (!native.is_null()).then(|| NativeIcon(native))
    }
}
impl Toolbar {
    fn apply(&mut self, hwnd: HWND, buttons: Vec<Button>) {
        if buttons.len() > MAX_BUTTONS {
            return;
        }
        if self.taskbar.is_none() {
            let result = unsafe {
                CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER)
            };
            let Ok(taskbar) = result else {
                return;
            };
            if unsafe { taskbar.HrInit() }.is_err() {
                return;
            }
            self.taskbar = Some(taskbar);
        }
        let Ok((operation, slots)) = self.state.apply(buttons) else {
            return;
        };
        let mut icons = Vec::new();
        let native = slots.map(|slot| {
            let handle = slot.icon.and_then(icon);
            let hicon = handle.as_ref().map_or(std::ptr::null_mut(), |i| i.0);
            if let Some(icon) = handle {
                icons.push(icon);
            }
            THUMBBUTTON {
                dwMask: THUMBBUTTONMASK(slot.mask as i32),
                iId: slot.id,
                iBitmap: 0,
                hIcon: ::windows::Win32::UI::WindowsAndMessaging::HICON(hicon),
                szTip: slot.tip,
                dwFlags: THUMBBUTTONFLAGS(slot.flags as i32),
            }
        });
        let taskbar = self.taskbar.as_ref().unwrap();
        unsafe {
            let _ = match operation {
                Operation::Add => taskbar.ThumbBarAddButtons(ComHwnd(hwnd), &native),
                Operation::Update => taskbar.ThumbBarUpdateButtons(ComHwnd(hwnd), &native),
            };
        }
        // The taskbar copies HICONs, as in Electron's local ScopedHICON array.
    }
}
unsafe extern "system" fn subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    // Clone before calls that can reenter/destroy the HWND; NCDESTROY owns the Box release.
    let context = unsafe { (&*(data as *const Rc<Context>)).clone() };
    if message == WM_NCDESTROY {
        unsafe {
            RemoveWindowSubclass(hwnd, Some(subclass), SUBCLASS);
            drop(Box::from_raw(data as *mut Rc<Context>));
        }
    } else if message == WM_COMMAND {
        let channel = context
            .toolbar
            .try_borrow()
            .ok()
            .and_then(|t| t.state.click(wparam));
        if let Some(channel) = channel {
            crate::shell::send_to_renderer(&context.app, channel, vec![]);
            return 0;
        }
    } else if let Ok(mut toolbar) = context.toolbar.try_borrow_mut() {
        if message == WM_SHOWWINDOW && wparam == 0 {
            toolbar.state.added = false;
        }
        if message == context.taskbar_created && toolbar.state.added {
            toolbar.state.added = false;
            let buttons = toolbar.state.last.clone();
            toolbar.apply(hwnd, buttons);
        }
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}
pub fn install(window: &WebviewWindow, buttons: Vec<Button>) {
    let win = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Ok(hwnd) = win.hwnd() else {
            return;
        };
        let hwnd = hwnd.0;
        let apartment =
            Apartment(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok());
        let context = Rc::new(Context {
            toolbar: RefCell::new(Toolbar {
                taskbar: None,
                state: State::default(),
            }),
            app: win.app_handle().clone(),
            taskbar_created: unsafe { RegisterWindowMessageW(windows_sys::w!("TaskbarCreated")) },
            _apartment: apartment,
        });
        let data = Box::into_raw(Box::new(context.clone()));
        if unsafe { SetWindowSubclass(hwnd, Some(subclass), SUBCLASS, data as usize) } == 0 {
            unsafe {
                drop(Box::from_raw(data));
            }
            return;
        }
        context.toolbar.borrow_mut().apply(hwnd, buttons);
    });
}
pub fn update(window: &WebviewWindow, buttons: Vec<Button>) {
    let win = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Ok(hwnd) = win.hwnd() else {
            return;
        };
        let mut data = 0;
        if unsafe { GetWindowSubclass(hwnd.0, Some(subclass), SUBCLASS, &mut data) } != 0 {
            let context = unsafe { (&*(data as *const Rc<Context>)).clone() };
            if let Ok(mut toolbar) = context.toolbar.try_borrow_mut() {
                toolbar.apply(hwnd.0, buttons);
            }
        }
    });
}
