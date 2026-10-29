//! تكامل سطح مكتب ويندوز المتقدم (`ITaskbarList3` شريط التقدم على أيقونة شريط المهام،
//! شفافية النافذة `SetLayeredWindowAttributes`، وسحب الملفات المكتملة للخارج `SHDoDragDrop`)
//! مستلهم من `ezoo-download/crates/app/src/winfx.rs`.

use std::path::Path;

/// حالة شريط التقدم المعروض على أيقونة البرنامج في شريط مهام ويندوز (Windows Taskbar)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskbarProgressState {
    Off,
    /// حجم غير معلوم: شريط متحرك (Indeterminate)
    Busy,
    Normal(u64, u64),
    Paused(u64, u64),
    Error(u64, u64),
}

/// حساب إحداثيات توسيط نافذة بحجم `size` داخل شاشة تبدأ عند `screen` وبأبعاد `screen_size`
pub fn centered_window_position(
    screen: (i32, i32),
    screen_size: (u32, u32),
    size: (u32, u32),
) -> (i32, i32) {
    let x = screen.0 + (screen_size.0 as i32 - size.0 as i32).max(0) / 2;
    let y = screen.1 + (screen_size.1 as i32 - size.1 as i32).max(0) / 2;
    (x, y)
}

/// ضبط شفافية نافذة ويندوز (`hwnd`) بنسبة مئوية بين `50%` و `100%`
#[allow(dead_code)]
pub fn set_hwnd_opacity(hwnd: isize, pct: u32) {
    let pct = pct.clamp(50, 100);
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, GWL_EXSTYLE,
            LWA_ALPHA, WS_EX_LAYERED,
        };
        if hwnd == 0 {
            return;
        }
        let h = hwnd as windows_sys::Win32::Foundation::HWND;
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        let layered = (ex & WS_EX_LAYERED as isize) != 0;
        if pct >= 100 {
            if layered {
                SetWindowLongPtrW(h, GWL_EXSTYLE, ex & !(WS_EX_LAYERED as isize));
            }
        } else {
            if !layered {
                SetWindowLongPtrW(h, GWL_EXSTYLE, ex | WS_EX_LAYERED as isize);
            }
            SetLayeredWindowAttributes(h, 0, (pct * 255 / 100) as u8, LWA_ALPHA);
        }
    }
    #[cfg(not(windows))]
    let _ = (hwnd, pct);
}

#[cfg(windows)]
mod taskbar_com {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    const CLSID_TASKBAR_LIST: Guid = Guid {
        data1: 0x56fdf344,
        data2: 0xfd6d,
        data3: 0x11d0,
        data4: [0x95, 0x8a, 0x00, 0x60, 0x97, 0xc9, 0xa0, 0x90],
    };
    const IID_ITASKBAR_LIST3: Guid = Guid {
        data1: 0xea1afb91,
        data2: 0x9e28,
        data3: 0x4b86,
        data4: [0x90, 0xe9, 0x9e, 0x9f, 0x8a, 0x5e, 0xef, 0xaf],
    };

    const CLSCTX_INPROC_SERVER: u32 = 0x1;
    const COINIT_APARTMENTTHREADED: u32 = 0x2;

    pub const TBPF_NOPROGRESS: i32 = 0;
    pub const TBPF_INDETERMINATE: i32 = 1;
    pub const TBPF_NORMAL: i32 = 2;
    pub const TBPF_ERROR: i32 = 4;
    pub const TBPF_PAUSED: i32 = 8;

    #[repr(C)]
    struct ITaskbarList3Vtbl {
        _query_interface: usize,
        _add_ref: usize,
        _release: usize,
        hr_init: extern "system" fn(*mut c_void) -> i32,
        _add_tab: usize,
        _delete_tab: usize,
        _activate_tab: usize,
        _set_active_alt: usize,
        _mark_fullscreen_window: usize,
        set_progress_value: extern "system" fn(*mut c_void, *mut c_void, u64, u64) -> i32,
        set_progress_state: extern "system" fn(*mut c_void, *mut c_void, i32) -> i32,
    }

    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(pv_reserved: *mut c_void, dw_co_init: u32) -> i32;
        fn CoCreateInstance(
            rclsid: *const Guid,
            p_unk_outer: *mut c_void,
            dw_cls_context: u32,
            riid: *const Guid,
            ppv: *mut *mut c_void,
        ) -> i32;
    }

    thread_local! {
        static TASKBAR_LIST: usize = unsafe {
            let _ = CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED);
            let mut p: *mut c_void = std::ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_TASKBAR_LIST,
                std::ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_ITASKBAR_LIST3,
                &mut p,
            );
            if hr < 0 || p.is_null() {
                0
            } else {
                let vt = *(p as *const *const ITaskbarList3Vtbl);
                ((*vt).hr_init)(p);
                p as usize
            }
        };
    }

    pub fn set_progress(hwnd: isize, state: i32, value: Option<(u64, u64)>) {
        if hwnd == 0 {
            return;
        }
        TASKBAR_LIST.with(|&p| {
            if p == 0 {
                return;
            }
            let ptr = p as *mut c_void;
            unsafe {
                let vt = *(ptr as *const *const ITaskbarList3Vtbl);
                ((*vt).set_progress_state)(ptr, hwnd as *mut c_void, state);
                if let Some((done, total)) = value {
                    ((*vt).set_progress_value)(ptr, hwnd as *mut c_void, done, total.max(1));
                }
            }
        });
    }
}

/// تحديث شريط التقدم على أيقونة البرنامج في شريط مهام ويندوز تلقائياً للنافذة الرئيسية النشطة
pub fn update_process_taskbar_progress(progress: TaskbarProgressState) {
    #[cfg(windows)]
    unsafe {
        #[link(name = "user32")]
        extern "system" {
            fn GetActiveWindow() -> isize;
            fn GetForegroundWindow() -> isize;
        }
        static LAST_KNOWN_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
        let mut hwnd = GetActiveWindow();
        if hwnd != 0 {
            LAST_KNOWN_HWND.store(hwnd, std::sync::atomic::Ordering::Relaxed);
        } else {
            hwnd = LAST_KNOWN_HWND.load(std::sync::atomic::Ordering::Relaxed);
            if hwnd == 0 {
                hwnd = GetForegroundWindow();
            }
        }
        if hwnd != 0 {
            set_taskbar_progress(hwnd, progress);
        }
    }
    #[cfg(not(windows))]
    let _ = progress;
}

/// تحديث حالة ونسبة شريط التقدم على أيقونة النافذة في شريط مهام ويندوز
pub fn set_taskbar_progress(hwnd: isize, progress: TaskbarProgressState) {
    #[cfg(windows)]
    {
        use taskbar_com::*;
        match progress {
            TaskbarProgressState::Off => set_progress(hwnd, TBPF_NOPROGRESS, None),
            TaskbarProgressState::Busy => set_progress(hwnd, TBPF_INDETERMINATE, None),
            TaskbarProgressState::Normal(d, t) => set_progress(hwnd, TBPF_NORMAL, Some((d, t))),
            TaskbarProgressState::Paused(d, t) => set_progress(hwnd, TBPF_PAUSED, Some((d, t))),
            TaskbarProgressState::Error(d, t) => set_progress(hwnd, TBPF_ERROR, Some((d, t))),
        }
    }
    #[cfg(not(windows))]
    let _ = (hwnd, progress);
}

/// سحب ملف مكتمل بالماوس من قائمة البرنامج إلى سطح المكتب أو أي مجلد أو تطبيق محادثة (`SHDoDragDrop`)
#[allow(dead_code)]
pub fn drag_completed_file_out(hwnd: isize, path: &Path) {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        use std::os::windows::ffi::OsStrExt;

        #[repr(C)]
        struct Guid {
            data1: u32,
            data2: u16,
            data3: u16,
            data4: [u8; 8],
        }

        const IID_IDATA_OBJECT: Guid = Guid {
            data1: 0x0000010e,
            data2: 0x0000,
            data3: 0x0000,
            data4: [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
        };
        const DROPEFFECT_COPY: u32 = 1;
        const DROPEFFECT_LINK: u32 = 4;

        #[link(name = "ole32")]
        extern "system" {
            fn OleInitialize(pv_reserved: *mut c_void) -> i32;
        }

        #[link(name = "shell32")]
        extern "system" {
            fn ILCreateFromPathW(psz_path: *const u16) -> *mut c_void;
            fn ILClone(pidl: *const c_void) -> *mut c_void;
            fn ILFindLastID(pidl: *const c_void) -> *mut c_void;
            fn ILRemoveLastID(pidl: *mut c_void) -> i32;
            fn ILFree(pidl: *mut c_void);
            fn SHCreateDataObject(
                pidl_folder: *const c_void,
                cidl: u32,
                apidl: *const *const c_void,
                pdt_inner: *mut c_void,
                riid: *const Guid,
                ppv: *mut *mut c_void,
            ) -> i32;
            fn SHDoDragDrop(
                hwnd: *mut c_void,
                pdata: *mut c_void,
                pdsrc: *mut c_void,
                dw_effect: u32,
                pdw_effect: *mut u32,
            ) -> i32;
        }

        if hwnd == 0 || !path.is_file() {
            return;
        }
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            let _ = OleInitialize(std::ptr::null_mut());
            let full = ILCreateFromPathW(wide.as_ptr());
            if full.is_null() {
                return;
            }
            let folder = ILClone(full);
            if !folder.is_null() {
                ILRemoveLastID(folder);
                let child = ILFindLastID(full) as *const c_void;
                let mut data: *mut c_void = std::ptr::null_mut();
                if SHCreateDataObject(
                    folder,
                    1,
                    &child,
                    std::ptr::null_mut(),
                    &IID_IDATA_OBJECT,
                    &mut data,
                ) >= 0
                    && !data.is_null()
                {
                    let mut effect: u32 = 0;
                    let _ = SHDoDragDrop(
                        hwnd as *mut c_void,
                        data,
                        std::ptr::null_mut(),
                        DROPEFFECT_COPY | DROPEFFECT_LINK,
                        &mut effect,
                    );
                    let vt = *(data as *const *const usize);
                    let release: extern "system" fn(*mut c_void) -> u32 =
                        std::mem::transmute(*vt.add(2));
                    release(data);
                }
                ILFree(folder);
            }
            ILFree(full);
        }
    }
    #[cfg(not(windows))]
    let _ = (hwnd, path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_window_on_primary_and_secondary_monitors() {
        assert_eq!(
            centered_window_position((0, 0), (1920, 1080), (920, 600)),
            (500, 240)
        );
        assert_eq!(
            centered_window_position((-1920, 0), (1920, 1080), (920, 600)),
            (-1420, 240)
        );
        assert_eq!(
            centered_window_position((100, 50), (800, 500), (1000, 700)),
            (100, 50)
        );
    }
}
