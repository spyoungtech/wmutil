// Most of this code in this file is taken verbatim from winit and, therefore, is conveyed under winit's Apache 2.0 license.
// https://github.com/rust-windowing/winit/blob/39a7d5b738c79687f3d493181378ad129702cbfc/LICENSE


use std::{io, mem, ptr};
use std::collections::VecDeque;
use std::ffi::{c_void, OsString};
use std::hash::Hash;
use std::ops::BitAnd;
use std::ops::Deref;
use std::os::windows::prelude::OsStringExt;
use std::sync::OnceLock;

use dpi::{PhysicalPosition, PhysicalSize};
use pyo3::prelude::*;
use pyo3::pymodule;
use windows_sys::core::HRESULT;
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT, S_OK};
use windows_sys::Win32::Graphics::Gdi::{
    DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplayMonitors, EnumDisplaySettingsExW,
    GetMonitorInfoW, HDC,
    HMONITOR, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint, MonitorFromWindow, MONITORINFO,
    MONITORINFOEXW,
};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};
use windows_sys::Win32::UI::HiDpi::{
    MDT_EFFECTIVE_DPI, MONITOR_DPI_TYPE,
};

pub const BASE_DPI: u32 = 96;

pub fn dpi_to_scale_factor(dpi: u32) -> f64 {
    dpi as f64 / BASE_DPI as f64
}

pub fn has_flag<T>(bitset: T, flag: T) -> bool
where
    T: Copy + PartialEq + BitAnd<T, Output=T>,
{
    bitset & flag == flag
}


#[derive(Debug, Clone, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct MonitorHandle(HMONITOR);


impl MonitorHandle {
    pub(crate) fn new(hmonitor: HMONITOR) -> Self {
        MonitorHandle(hmonitor)
    }

    #[inline]
    pub fn name(&self) -> Option<String> {
        let monitor_info = get_monitor_info(self.0).unwrap();
        Some(decode_wide(&monitor_info.szDevice).to_string_lossy().to_string())
    }

    #[inline]
    pub fn native_identifier(&self) -> String {
        self.name().unwrap()
    }

    #[inline]
    pub fn hmonitor(&self) -> HMONITOR {
        self.0
    }

    #[inline]
    pub fn size(&self) -> PhysicalSize<u32> {
        let rc_monitor = get_monitor_info(self.0).unwrap().monitorInfo.rcMonitor;
        PhysicalSize {
            width: (rc_monitor.right - rc_monitor.left) as u32,
            height: (rc_monitor.bottom - rc_monitor.top) as u32,
        }
    }

    #[inline]
    pub fn refresh_rate_millihertz(&self) -> Option<u32> {
        let monitor_info = get_monitor_info(self.0).ok()?;
        let device_name = monitor_info.szDevice.as_ptr();
        unsafe {
            let mut mode: DEVMODEW = mem::zeroed();
            mode.dmSize = mem::size_of_val(&mode) as u16;
            if EnumDisplaySettingsExW(device_name, ENUM_CURRENT_SETTINGS, &mut mode, 0)
                == false.into()
            {
                None
            } else {
                Some(mode.dmDisplayFrequency * 1000)
            }
        }
    }

    #[inline]
    pub fn position(&self) -> PhysicalPosition<i32> {
        get_monitor_info(self.0)
            .map(|info| {
                let rc_monitor = info.monitorInfo.rcMonitor;
                PhysicalPosition { x: rc_monitor.left, y: rc_monitor.top }
            })
            .unwrap_or(PhysicalPosition { x: 0, y: 0 })
    }

    #[inline]
    pub fn scale_factor(&self) -> f64 {
        dpi_to_scale_factor(get_monitor_dpi(self.0).unwrap_or(96))
    }
}


pub fn available_monitors() -> VecDeque<MonitorHandle> {
    let mut monitors: VecDeque<MonitorHandle> = VecDeque::new();
    unsafe {
        EnumDisplayMonitors(
            0,
            ptr::null(),
            Some(monitor_enum_proc),
            &mut monitors as *mut _ as LPARAM,
        );
    }
    monitors
}

pub(crate) fn get_monitor_info(hmonitor: HMONITOR) -> Result<MONITORINFOEXW, io::Error> {
    let mut monitor_info: MONITORINFOEXW = unsafe { mem::zeroed() };
    monitor_info.monitorInfo.cbSize = mem::size_of::<MONITORINFOEXW>() as u32;
    let status = unsafe {
        GetMonitorInfoW(hmonitor, &mut monitor_info as *mut MONITORINFOEXW as *mut MONITORINFO)
    };
    if status == false.into() {
        Err(io::Error::last_os_error())
    } else {
        Ok(monitor_info)
    }
}

pub fn decode_wide(mut wide_c_string: &[u16]) -> OsString {
    if let Some(null_pos) = wide_c_string.iter().position(|c| *c == 0) {
        wide_c_string = &wide_c_string[..null_pos];
    }

    OsString::from_wide(wide_c_string)
}

pub type GetDpiForMonitor = unsafe extern "system" fn(
    hmonitor: HMONITOR,
    dpi_type: MONITOR_DPI_TYPE,
    dpi_x: *mut u32,
    dpi_y: *mut u32,
) -> HRESULT;

fn get_function_impl(library: &str, function: &str) -> Option<*const c_void> {
    assert_eq!(library.chars().last(), Some('\0'));
    assert_eq!(function.chars().last(), Some('\0'));

    // Library names we will use are ASCII so we can use the A version to avoid string conversion.
    let module = unsafe { LoadLibraryA(library.as_ptr()) };
    if module == 0 {
        return None;
    }

    unsafe { GetProcAddress(module, function.as_ptr()) }.map(|function_ptr| function_ptr as _)
}


macro_rules! get_function {
    ($lib:expr, $func:ident) => {
        get_function_impl(
            concat!($lib, '\0'),
            concat!(stringify!($func), '\0'),
        )
        .map(|f| unsafe { std::mem::transmute::<*const _, $func>(f) })
    };
}


pub(crate) struct Lazy<T> {
    cell: OnceLock<T>,
    init: fn() -> T,
}

impl<T> Lazy<T> {
    pub const fn new(f: fn() -> T) -> Self {
        Self { cell: OnceLock::new(), init: f }
    }
}

impl<T> Deref for Lazy<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &'_ T {
        self.cell.get_or_init(self.init)
    }
}

pub(crate) static GET_DPI_FOR_MONITOR: Lazy<Option<GetDpiForMonitor>> =
    Lazy::new(|| get_function!("shcore.dll", GetDpiForMonitor));

pub fn get_monitor_dpi(hmonitor: HMONITOR) -> Option<u32> {
    unsafe {
        if let Some(GetDpiForMonitor) = *GET_DPI_FOR_MONITOR {
            // We are on Windows 8.1 or later.
            let mut dpi_x = 0;
            let mut dpi_y = 0;
            if GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) == S_OK {
                // MSDN says that "the values of *dpiX and *dpiY are identical. You only need to
                // record one of the values to determine the DPI and respond appropriately".
                // https://msdn.microsoft.com/en-us/library/windows/desktop/dn280510(v=vs.85).aspx
                return Some(dpi_x);
            }
        }
    }
    None
}


pub fn primary_monitor() -> MonitorHandle {
    const ORIGIN: POINT = POINT { x: 0, y: 0 };
    let hmonitor = unsafe { MonitorFromPoint(ORIGIN, MONITOR_DEFAULTTOPRIMARY) };
    MonitorHandle::new(hmonitor)
}

pub fn current_monitor(hwnd: HWND) -> MonitorHandle {
    let hmonitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    MonitorHandle::new(hmonitor)
}

unsafe extern "system" fn monitor_enum_proc(
    hmonitor: HMONITOR,
    _hdc: HDC,
    _place: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let monitors = data as *mut VecDeque<MonitorHandle>;
    unsafe { (*monitors).push_back(MonitorHandle::new(hmonitor)) };
    true.into() // continue enumeration
}


// Python bindings

#[pyclass(module = "wmutil")]
struct MonitorInfo {
    monitor_handle: MonitorHandle,
}

#[pymethods]
impl MonitorInfo {
    #[getter]
    fn name(&self) -> String {
        self.monitor_handle.name().unwrap_or(String::from("Unknown monitor name"))
    }

    #[getter]
    fn size(&self) -> (u32, u32) {
        let size = self.monitor_handle.size();
        let width = size.width;
        let height = size.height;
        (width, height)
    }

    #[getter]
    fn position(&self) -> (i32, i32) {
        let position = self.monitor_handle.position();
        let x_pos = position.x;
        let y_pos = position.y;
        (x_pos, y_pos)
    }

    #[getter]
    fn scale_factor(&self) -> f64 {
        self.monitor_handle.scale_factor()
    }

    #[getter]
    fn refresh_rate_millihertz(&self) -> Option<u32> {
        self.monitor_handle.refresh_rate_millihertz()
    }

    #[getter]
    fn handle(&self) -> isize {
        self.monitor_handle.0 as isize
    }

    pub fn __hash__(&self) -> isize {
        self.handle()
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        if self.handle() == other.handle() {
            true
        } else {
            false
        }
    }

    pub fn __repr__(&self) -> PyResult<String> {
        Ok(format!("<wmutil.Monitor object; handle={}>", self.handle()))
    }
}

#[pyfunction]
fn get_primary_monitor() -> MonitorInfo {
    let handle = primary_monitor();
    MonitorInfo {
        monitor_handle: handle
    }
}

#[pyfunction]
fn get_window_monitor(hwnd: isize) -> MonitorInfo {
    let handle = current_monitor(hwnd.into());
    MonitorInfo {
        monitor_handle: handle
    }
}

#[pyfunction]
fn enumerate_monitors() -> Vec<MonitorInfo> {
    let mut monitors: Vec<MonitorInfo> = Vec::new();
    for monitor in available_monitors() {
        monitors.push(MonitorInfo { monitor_handle: monitor })
    }
    monitors
}

#[pyfunction]
fn get_monitor_from_point(x: i32, y: i32) -> MonitorInfo {
    let point = POINT {x, y};
    let hmonitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTOPRIMARY) };
    let handle = MonitorHandle::new(hmonitor);
    MonitorInfo {
        monitor_handle: handle
    }
}


#[pymodule]
fn wmutil(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<MonitorInfo>()?;
    m.add_function(wrap_pyfunction!(enumerate_monitors, m)?);
    m.add_function(wrap_pyfunction!(get_window_monitor, m)?);
    m.add_function(wrap_pyfunction!(get_primary_monitor, m)?);
    m.add_function(wrap_pyfunction!(get_monitor_from_point, m)?);

    Ok(())
}


