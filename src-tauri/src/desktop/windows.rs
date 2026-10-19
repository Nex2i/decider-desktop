use super::*;
use ::windows::Win32::Foundation::CloseHandle;
use ::windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use ::windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use ::windows::Win32::UI::Accessibility::*;
use ::windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
use ::windows::core::{Interface, PWSTR};
use std::collections::VecDeque;

struct ComApartment;
impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn process_name(pid: u32) -> String {
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return format!("Application {pid}");
        };
        let mut buffer = [0u16; 32768];
        let mut length = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(handle);
        if result.is_err() {
            return format!("Application {pid}");
        }
        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        text(
            &std::path::Path::new(&path)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy(),
        )
    }
}

pub fn inspect(target_pid: Option<u32>, include_labels: bool) -> Result<DesktopSnapshot, String> {
    let mut snapshot = DesktopSnapshot::new(include_labels);
    let folders: Vec<_> = ["APPDATA", "PROGRAMDATA"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|folder| {
            std::path::PathBuf::from(folder).join("Microsoft/Windows/Start Menu/Programs")
        })
        .collect();
    let (installed, truncated) = installed_in_folders(&folders, "lnk");
    snapshot.installed_apps = installed;
    snapshot.truncated |= truncated;
    snapshot.warn("Installed app inventory covers Start menu shortcuts. Portable executables and packaged apps without shortcuts may be missing.");
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(|_| "Could not initialize Windows UI Automation.")?;
        let _apartment = ComApartment;
        let automation2: IUIAutomation2 =
            CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
                .map_err(|_| "Windows UI Automation is unavailable.")?;
        automation2
            .SetConnectionTimeout(250)
            .map_err(|_| "Could not set discovery time limits.")?;
        automation2
            .SetTransactionTimeout(250)
            .map_err(|_| "Could not set discovery time limits.")?;
        let automation: IUIAutomation = automation2
            .cast()
            .map_err(|_| "Windows UI Automation is unavailable.")?;
        let walker = automation
            .ControlViewWalker()
            .map_err(|_| "Could not read desktop controls.")?;
        let desktop = automation
            .GetRootElement()
            .map_err(|_| "Could not inspect the Windows desktop.")?;
        let foreground = GetForegroundWindow();
        snapshot.accessibility_available = true;
        snapshot.started = Some(Instant::now());
        let mut roots = Vec::new();
        let mut next = walker.GetFirstChildElement(&desktop).ok();
        let mut top_level = 0;
        while let Some(window) = next {
            if !snapshot.has_budget() || top_level >= 512 {
                snapshot.truncated = true;
                break;
            }
            top_level += 1;
            next = walker.GetNextSiblingElement(&window).ok();
            let Ok(pid) = window.CurrentProcessId() else {
                snapshot.warn("A window denied discovery access.");
                continue;
            };
            if pid <= 0 {
                continue;
            }
            let pid = pid as u32;
            let id = format!("windows:{pid}");
            let active = window
                .CurrentNativeWindowHandle()
                .is_ok_and(|handle| handle == foreground);
            if let Some(app) = snapshot.apps.iter_mut().find(|app| app.pid == pid) {
                app.foreground |= active;
            } else {
                snapshot.apps.push(DesktopApp {
                    id: id.clone(),
                    name: process_name(pid),
                    pid,
                    foreground: active,
                });
            }
            roots.push((window, id, pid));
        }
        snapshot
            .apps
            .sort_by_key(|app| (!app.foreground, app.name.to_lowercase()));
        if let Some(pid) = target_pid
            && !snapshot.apps.iter().any(|app| app.pid == pid)
        {
            return Err("That app has closed. Refresh the running app list.".into());
        }
        let selected: HashSet<_> = snapshot
            .apps
            .iter()
            .filter(|app| target_pid.is_none_or(|pid| app.pid == pid))
            .take(MAX_APPS)
            .map(|app| app.pid)
            .collect();
        if target_pid.is_none() && snapshot.apps.len() > MAX_APPS {
            snapshot.truncated = true;
        }
        for (index, (root, id, pid)) in roots.into_iter().enumerate() {
            if !selected.contains(&pid) {
                continue;
            }
            scan_window(&mut snapshot, &walker, root, &id, index);
            if !snapshot.has_budget() {
                break;
            }
        }
    }
    snapshot.warn("This is a snapshot of standard UI Automation capabilities. Elevated apps, secure desktop, custom controls, and hidden commands may be unavailable. No actions were executed.");
    Ok(snapshot)
}

fn scan_window(
    snapshot: &mut DesktopSnapshot,
    walker: &IUIAutomationTreeWalker,
    root: IUIAutomationElement,
    app_id: &str,
    window: usize,
) {
    let mut queue = VecDeque::from([(root, format!("window/{window}"), 0)]);
    while let Some((element, path, depth)) = queue.pop_front() {
        if !snapshot.has_budget() {
            break;
        }
        snapshot.nodes_visited += 1;
        unsafe {
            // Fail closed if the provider will not reveal whether this is a password control.
            if element
                .CurrentIsPassword()
                .map(|value| value.as_bool())
                .unwrap_or(true)
            {
                continue;
            }
            let enabled = element.CurrentIsEnabled().ok().map(|value| value.as_bool());
            let visible = element
                .CurrentIsOffscreen()
                .is_ok_and(|value| !value.as_bool());
            let available = enabled == Some(true) && visible;
            let mut capabilities = Vec::new();
            for (pattern, name, operations, kind) in PATTERNS {
                if !snapshot.has_budget() {
                    break;
                }
                if element.GetCurrentPattern(UIA_PATTERN_ID(*pattern)).is_err() {
                    continue;
                }
                let writable = match *pattern {
                    10002 => element
                        .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                        .ok()
                        .and_then(|pattern| pattern.CurrentIsReadOnly().ok())
                        .is_some_and(|value| !value.as_bool()),
                    10003 => element
                        .GetCurrentPatternAs::<IUIAutomationRangeValuePattern>(
                            UIA_RangeValuePatternId,
                        )
                        .ok()
                        .and_then(|pattern| pattern.CurrentIsReadOnly().ok())
                        .is_some_and(|value| !value.as_bool()),
                    _ => true,
                };
                for operation in operations.split(',') {
                    let supported = match (*pattern, operation) {
                        (10016, "Move") => element
                            .GetCurrentPatternAs::<IUIAutomationTransformPattern>(
                                UIA_TransformPatternId,
                            )
                            .ok()
                            .and_then(|p| p.CurrentCanMove().ok())
                            .is_some_and(|v| v.as_bool()),
                        (10016, "Resize") => element
                            .GetCurrentPatternAs::<IUIAutomationTransformPattern>(
                                UIA_TransformPatternId,
                            )
                            .ok()
                            .and_then(|p| p.CurrentCanResize().ok())
                            .is_some_and(|v| v.as_bool()),
                        (10016, "Rotate") => element
                            .GetCurrentPatternAs::<IUIAutomationTransformPattern>(
                                UIA_TransformPatternId,
                            )
                            .ok()
                            .and_then(|p| p.CurrentCanRotate().ok())
                            .is_some_and(|v| v.as_bool()),
                        (10028, _) => element
                            .GetCurrentPatternAs::<IUIAutomationTransformPattern2>(
                                UIA_TransformPattern2Id,
                            )
                            .ok()
                            .and_then(|p| p.CurrentCanZoom().ok())
                            .is_some_and(|v| v.as_bool()),
                        _ => writable,
                    };
                    capabilities.push(Capability {
                        native_name: (*name).into(),
                        operation: operation.into(),
                        kind,
                        available: supported && (available || *kind == "inspection"),
                    });
                }
            }
            if !capabilities.is_empty() {
                snapshot.targets.push(DesktopTarget {
                    id: identifier(&format!("{}:{app_id}:{path}", snapshot.captured_at)),
                    app_id: app_id.into(),
                    role: element
                        .CurrentLocalizedControlType()
                        .map(|value| text(&value.to_string()))
                        .unwrap_or_else(|_| "Control".into()),
                    label: if snapshot.labels_included {
                        element
                            .CurrentName()
                            .ok()
                            .map(|value| text(&value.to_string()))
                    } else {
                        None
                    },
                    enabled,
                    focused: element
                        .CurrentHasKeyboardFocus()
                        .is_ok_and(|value| value.as_bool()),
                    capabilities,
                });
            }
            if depth >= MAX_DEPTH {
                snapshot.truncated = true;
                continue;
            }
            let mut next = walker.GetFirstChildElement(&element).ok();
            let mut index = 0;
            while let Some(child) = next {
                if !snapshot.has_budget() || queue.len() + snapshot.nodes_visited >= MAX_NODES {
                    snapshot.truncated = true;
                    break;
                }
                next = walker.GetNextSiblingElement(&child).ok();
                queue.push_back((child, format!("{path}/{index}"), depth + 1));
                index += 1;
            }
        }
    }
}

// All 35 standard UI Automation patterns. Inspection capabilities are distinct
// from actions; no pattern method that changes the desktop is called here.
const PATTERNS: &[(i32, &str, &str, &str)] = &[
    (10000, "Invoke", "Invoke", "action"),
    (10001, "Selection", "Inspect selection", "inspection"),
    (10002, "Value", "SetValue", "writable_attribute"),
    (10003, "RangeValue", "SetValue", "writable_attribute"),
    (10004, "Scroll", "Scroll,SetScrollPercent", "action"),
    (10005, "ExpandCollapse", "Expand,Collapse", "action"),
    (10006, "Grid", "Inspect grid", "inspection"),
    (10007, "GridItem", "Inspect grid item", "inspection"),
    (10008, "MultipleView", "SetCurrentView", "action"),
    (10009, "Window", "Close,SetWindowVisualState", "action"),
    (
        10010,
        "SelectionItem",
        "Select,AddToSelection,RemoveFromSelection",
        "action",
    ),
    (10011, "Dock", "SetDockPosition", "action"),
    (10012, "Table", "Inspect table", "inspection"),
    (10013, "TableItem", "Inspect table item", "inspection"),
    (10014, "Text", "Inspect text support", "inspection"),
    (10015, "Toggle", "Toggle", "action"),
    (10016, "Transform", "Move,Resize,Rotate", "action"),
    (10017, "ScrollItem", "ScrollIntoView", "action"),
    (
        10018,
        "LegacyIAccessible",
        "Legacy default action",
        "action",
    ),
    (10019, "ItemContainer", "Find item", "inspection"),
    (10020, "VirtualizedItem", "Realize", "action"),
    (
        10021,
        "SynchronizedInput",
        "StartListening,Cancel",
        "action",
    ),
    (
        10022,
        "ObjectModel",
        "Inspect object model support",
        "inspection",
    ),
    (
        10023,
        "Annotation",
        "Inspect annotation support",
        "inspection",
    ),
    (10024, "Text2", "Inspect text range support", "inspection"),
    (10025, "Styles", "Inspect style support", "inspection"),
    (
        10026,
        "Spreadsheet",
        "Inspect spreadsheet support",
        "inspection",
    ),
    (
        10027,
        "SpreadsheetItem",
        "Inspect spreadsheet item support",
        "inspection",
    ),
    (10028, "Transform2", "Zoom,ZoomByUnit", "action"),
    (
        10029,
        "TextChild",
        "Inspect text child support",
        "inspection",
    ),
    (10030, "Drag", "Inspect drag support", "inspection"),
    (
        10031,
        "DropTarget",
        "Inspect drop target support",
        "inspection",
    ),
    (10032, "TextEdit", "Inspect text edit support", "inspection"),
    (
        10033,
        "CustomNavigation",
        "Inspect navigation support",
        "inspection",
    ),
    (
        10034,
        "Selection2",
        "Inspect selection support",
        "inspection",
    ),
];
