use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use serde::Serialize;
use tauri::{Manager, PhysicalPosition};
use windows::{
    core::Result as WindowsResult,
    Win32::{
        Foundation::POINT,
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
                COINIT_MULTITHREADED, SAFEARRAY,
            },
            Ole::{
                SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
                SafeArrayUnaccessData,
            },
        },
        UI::{
            Accessibility::{
                CUIAutomation8, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
                IUIAutomationTextRange, TextPatternRangeEndpoint_End,
                TextPatternRangeEndpoint_Start, TextUnit_Character, UIA_TextPatternId,
            },
            WindowsAndMessaging::{GetCursorPos, GetForegroundWindow, GetWindowTextW},
        },
    },
};

const DOT_SIZE: i32 = 32;
const DOT_OFFSET_X: i32 = 5;
const DOT_OFFSET_Y: i32 = 4;
const CONTEXT_RADIUS: i32 = 1_000;
const MAX_SELECTED_CHARS: i32 = 4_000;
const MAX_CONTEXT_CHARS: i32 = 6_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionSnapshot {
    pub selected_text: String,
    pub surrounding_text: String,
    pub source_title: String,
    pub captured_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SelectionRect {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

pub fn start_watcher(app: tauri::AppHandle, suppressed: Arc<AtomicBool>) {
    thread::spawn(move || {
        let Ok(automation) = AutomationSession::new() else {
            return;
        };
        let mut last_position = None;
        let mut visible = false;

        loop {
            if suppressed.load(Ordering::Relaxed) {
                if visible {
                    hide_dot(&app);
                    visible = false;
                }
                thread::sleep(Duration::from_millis(220));
                continue;
            }

            let selection = automation.selection_rect();
            match selection {
                Some(rect) => {
                    let position = dot_position(rect);
                    if last_position != Some(position) {
                        if let Some(window) = app.get_webview_window("capture-dot") {
                            let _ =
                                window.set_position(PhysicalPosition::new(position.0, position.1));
                        }
                        last_position = Some(position);
                    }
                    if !visible {
                        if let Some(window) = app.get_webview_window("capture-dot") {
                            let _ = window.show();
                        }
                        visible = true;
                    }
                }
                None => {
                    if visible {
                        hide_dot(&app);
                        visible = false;
                    }
                    last_position = None;
                }
            }
            thread::sleep(Duration::from_millis(320));
        }
    });
}

pub fn read_current_selection() -> Result<SelectionSnapshot, String> {
    thread::spawn(|| {
        let automation = AutomationSession::new().map_err(clean_windows_error)?;
        automation.snapshot()
    })
    .join()
    .map_err(|_| "读取选区的系统线程意外退出".to_string())?
}

fn hide_dot(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("capture-dot") {
        let _ = window.hide();
    }
}

fn dot_position(rect: SelectionRect) -> (i32, i32) {
    let x = (rect.left + rect.width + DOT_OFFSET_X as f64).round() as i32;
    let y = (rect.top + rect.height - (DOT_SIZE / 2) as f64 + DOT_OFFSET_Y as f64).round() as i32;
    (x, y)
}

struct AutomationSession {
    automation: IUIAutomation,
    com_initialized: bool,
}

impl AutomationSession {
    fn new() -> WindowsResult<Self> {
        unsafe {
            let result = CoInitializeEx(None, COINIT_MULTITHREADED);
            result.ok()?;
            match CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) {
                Ok(automation) => Ok(Self {
                    automation,
                    com_initialized: true,
                }),
                Err(error) => {
                    CoUninitialize();
                    Err(error)
                }
            }
        }
    }

    fn selection_rect(&self) -> Option<SelectionRect> {
        let (_, range) = self.current_selection().ok()?;
        bounding_rectangles(&range).ok()?.into_iter().last()
    }

    fn snapshot(&self) -> Result<SelectionSnapshot, String> {
        let (_, range) = self.current_selection().map_err(clean_windows_error)?;
        let selected_text = unsafe { range.GetText(MAX_SELECTED_CHARS) }
            .map_err(clean_windows_error)?
            .to_string();
        if selected_text.trim().is_empty() {
            return Err("当前选区为空，请重新选中文字".to_string());
        }

        let context_range = unsafe { range.Clone() }.map_err(clean_windows_error)?;
        unsafe {
            let _ = context_range.MoveEndpointByUnit(
                TextPatternRangeEndpoint_Start,
                TextUnit_Character,
                -CONTEXT_RADIUS,
            );
            let _ = context_range.MoveEndpointByUnit(
                TextPatternRangeEndpoint_End,
                TextUnit_Character,
                CONTEXT_RADIUS,
            );
        }
        let surrounding_text = unsafe { context_range.GetText(MAX_CONTEXT_CHARS) }
            .map_err(clean_windows_error)?
            .to_string();

        Ok(SelectionSnapshot {
            selected_text,
            surrounding_text,
            source_title: foreground_window_title(),
            captured_at: chrono::Utc::now().timestamp_millis(),
        })
    }

    fn current_selection(&self) -> WindowsResult<(IUIAutomationElement, IUIAutomationTextRange)> {
        unsafe {
            let focused = self.automation.GetFocusedElement()?;
            if let Some(result) = self.find_selection(focused) {
                return Ok(result);
            }

            let mut point = POINT::default();
            GetCursorPos(&mut point)?;
            let pointed = self.automation.ElementFromPoint(point)?;
            self.find_selection(pointed).ok_or_else(|| {
                windows::core::Error::new(
                    windows::core::HRESULT(0x8000_4005_u32 as i32),
                    "当前软件没有提供可读取的非空文本选区",
                )
            })
        }
    }

    unsafe fn find_selection(
        &self,
        mut element: IUIAutomationElement,
    ) -> Option<(IUIAutomationElement, IUIAutomationTextRange)> {
        let walker = self.automation.RawViewWalker().ok()?;
        for _ in 0..12 {
            let process_id = element.CurrentProcessId().ok()?;
            let is_password = element
                .CurrentIsPassword()
                .map(|value| value.as_bool())
                .unwrap_or(false);
            if process_id as u32 != std::process::id() && !is_password {
                if let Ok(pattern) =
                    element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                {
                    if let Ok(ranges) = pattern.GetSelection() {
                        let length = ranges.Length().unwrap_or(0);
                        for index in (0..length).rev() {
                            if let Ok(range) = ranges.GetElement(index) {
                                let non_empty = range
                                    .CompareEndpoints(
                                        TextPatternRangeEndpoint_Start,
                                        &range,
                                        TextPatternRangeEndpoint_End,
                                    )
                                    .map(|comparison| comparison != 0)
                                    .unwrap_or(false);
                                if non_empty {
                                    return Some((element, range));
                                }
                            }
                        }
                    }
                }
            }
            element = match walker.GetParentElement(&element) {
                Ok(parent) => parent,
                Err(_) => break,
            };
        }
        None
    }
}

impl Drop for AutomationSession {
    fn drop(&mut self) {
        if self.com_initialized {
            unsafe { CoUninitialize() };
        }
    }
}

fn bounding_rectangles(range: &IUIAutomationTextRange) -> WindowsResult<Vec<SelectionRect>> {
    let array = unsafe { range.GetBoundingRectangles()? };
    let values = unsafe { read_double_array(array)? };
    Ok(values
        .chunks_exact(4)
        .filter_map(|values| {
            let rect = SelectionRect {
                left: values[0],
                top: values[1],
                width: values[2],
                height: values[3],
            };
            (rect.width > 1.0 && rect.height > 1.0).then_some(rect)
        })
        .collect())
}

unsafe fn read_double_array(array: *mut SAFEARRAY) -> WindowsResult<Vec<f64>> {
    if array.is_null() {
        return Ok(Vec::new());
    }
    let result = (|| unsafe {
        let lower = SafeArrayGetLBound(array, 1)?;
        let upper = SafeArrayGetUBound(array, 1)?;
        if upper < lower {
            return Ok(Vec::new());
        }
        let mut data: *mut c_void = std::ptr::null_mut();
        SafeArrayAccessData(array, &mut data)?;
        let length = (upper - lower + 1) as usize;
        let values = std::slice::from_raw_parts(data.cast::<f64>(), length).to_vec();
        SafeArrayUnaccessData(array)?;
        Ok(values)
    })();
    let _ = SafeArrayDestroy(array);
    result
}

fn foreground_window_title() -> String {
    unsafe {
        let window = GetForegroundWindow();
        let mut buffer = [0_u16; 512];
        let length = GetWindowTextW(window, &mut buffer).max(0) as usize;
        String::from_utf16_lossy(&buffer[..length])
    }
}

fn clean_windows_error(error: windows::core::Error) -> String {
    let message = error.message();
    if message.trim().is_empty() {
        "当前软件没有提供可读取的文本选区".to_string()
    } else {
        message
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_is_placed_after_selection_end() {
        assert_eq!(
            dot_position(SelectionRect {
                left: 100.0,
                top: 50.0,
                width: 80.0,
                height: 24.0,
            }),
            (185, 62)
        );
    }
}
