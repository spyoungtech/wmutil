// Most of this code in this file is taken verbatim from winit and, therefore, is conveyed under winit's Apache 2.0 license.
// https://github.com/rust-windowing/winit/blob/39a7d5b738c79687f3d493181378ad129702cbfc/LICENSE


use std::{io, mem, ptr};
use std::collections::VecDeque;
use std::ffi::{c_void, OsString};
use std::hash::Hash;
use std::ops::{BitAnd, Neg};
use std::ops::Deref;
use std::os::windows::prelude::OsStringExt;
use std::sync::OnceLock;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use dpi::{PhysicalPosition, PhysicalSize};
use pyo3::prelude::*;
use pyo3::pymodule;
use windows_sys::core::HRESULT;
use windows_sys::Win32::Foundation::{BOOL, HWND, WPARAM, LPARAM, POINT, RECT, POINTL, S_OK};
use windows_sys::Win32::Graphics::Gdi::{
    DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplayMonitors, EnumDisplaySettingsExW,
    GetMonitorInfoW, HDC,
    HMONITOR, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint, MonitorFromWindow, MONITORINFO,
    MONITORINFOEXW,
};
use windows_sys::Win32::Graphics::Gdi::*;

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
#[derive(Clone)]
struct Monitor {
    monitor_handle: MonitorHandle,
}

#[pymethods]
impl Monitor {
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

    pub fn set_primary(&self) -> PyResult<()> {
        set_primary_monitor(self.name());
        Ok(())
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
fn get_primary_monitor() -> Monitor {
    let handle = primary_monitor();
    Monitor {
        monitor_handle: handle
    }
}

#[pyfunction]
fn get_window_monitor(hwnd: isize) -> Monitor {
    let handle = current_monitor(hwnd.into());
    Monitor {
        monitor_handle: handle
    }
}

#[pyfunction]
fn enumerate_monitors() -> Vec<Monitor> {
    let mut monitors: Vec<Monitor> = Vec::new();
    for monitor in available_monitors() {
        monitors.push(Monitor { monitor_handle: monitor })
    }
    monitors
}

#[pyfunction]
fn get_monitor_from_point(x: i32, y: i32) -> Monitor {
    let point = POINT {x, y};
    let hmonitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTOPRIMARY) };
    let handle = MonitorHandle::new(hmonitor);
    Monitor {
        monitor_handle: handle
    }
}

fn wide_string(s: &str) -> Vec<u16> {
    let mut vec: Vec<u16> = s.encode_utf16().collect();
    vec.push(0);
    vec
}


fn get_dev_mode(display_name: &str) -> Result<DEVMODEW, String> {
    let mut devmode: DEVMODEW = unsafe { std::mem::zeroed() };
    devmode.dmSize = size_of::<DEVMODEW>() as u16;

    let wide_name = wide_string(display_name);

    let success = unsafe {
        EnumDisplaySettingsW(wide_name.as_ptr(), ENUM_CURRENT_SETTINGS, &mut devmode)
    };

    if success == 0 {
        return Err(format!("Failed to retrieve settings for display: {}", display_name));
    }

    Ok(devmode)
}


#[pyfunction]
fn set_primary_monitor(display_name: String) -> PyResult<bool> {
    let all_monitors = enumerate_monitors();
    let mut maybe_this_monitor: Option<Monitor> = None;
    for monitor in all_monitors.clone() {
        if monitor.name() == display_name {
            maybe_this_monitor = Some(monitor);
            break
        }
    }

    // todo: raise a proper exception instead of a panic exception
    assert!(maybe_this_monitor.is_some(), "Monitor with name {:?} not found", display_name);

    let this_monitor = maybe_this_monitor.unwrap();

    let (this_x, this_y) = this_monitor.position();

    if (this_x == 0 && this_y == 0) {
        // the requested monitor is already the primary monitor
        return Ok(true)
    }

    let x_offset = this_x.neg();
    let y_offset = this_y.neg();

    let display_name_string = display_name.as_str();
    let wide_name = wide_string(display_name_string);

    for monitor in all_monitors.clone() {
        if monitor.name() != display_name {
            let mut devmode: DEVMODEW = get_dev_mode(monitor.name().as_str()).unwrap();
            unsafe {
                let (monitor_x, monitor_y) = monitor.position();
                let new_x = monitor_x + x_offset;
                let new_y = monitor_y + y_offset;
                devmode.Anonymous1.Anonymous2.dmPosition = POINTL { x: new_x, y: new_y };
                // println!("display: {} old: {} {} new: {} {}", monitor.name(), monitor_x, monitor_y, new_x, new_y);
                ChangeDisplaySettingsExW(wide_string(monitor.name().as_str()).as_ptr(), &mut devmode, 0, CDS_UPDATEREGISTRY | CDS_NORESET, null_mut());
            }
        }
    }
    let mut devmode: DEVMODEW = get_dev_mode(display_name_string).unwrap();
    unsafe {
        // println!("{} being set as primary to 0 0", display_name);
        devmode.Anonymous1.Anonymous2.dmPosition = POINTL { x: 0, y: 0 };
        ChangeDisplaySettingsExW(wide_name.as_ptr(), &mut devmode, 0, CDS_SET_PRIMARY | CDS_UPDATEREGISTRY | CDS_NORESET, null_mut());
    }

    let result = unsafe {
        ChangeDisplaySettingsExW(null_mut(), null_mut(), 0, 0, null_mut())
    };
    if result == DISP_CHANGE_SUCCESSFUL {
        Ok(true)
    } else {
        Ok(false)
    }
}



#[pymodule]
fn wmutil(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<Monitor>()?;
    m.add_function(wrap_pyfunction!(enumerate_monitors, m)?);
    m.add_function(wrap_pyfunction!(get_window_monitor, m)?);
    m.add_function(wrap_pyfunction!(get_primary_monitor, m)?);
    m.add_function(wrap_pyfunction!(get_monitor_from_point, m)?);
    m.add_function(wrap_pyfunction!(set_primary_monitor, m)?);

    Ok(())
}


