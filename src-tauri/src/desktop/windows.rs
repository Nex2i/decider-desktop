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

pub fn inspect(
    target_pid: Option<u32>,
    include_labels: bool,
    inventory_only: bool,
) -> Result<DesktopSnapshot, String> {
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
    snapshot.inventory_truncated = truncated;
    if truncated {
        snapshot.warn("The installed-app inventory reached its folder/entry limit. This limit is independent of the control scan.");
    }
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
                snapshot.limit_controls();
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
        if inventory_only {
            return Ok(snapshot);
        }
        if let Some(pid) = target_pid
            && !snapshot.apps.iter().any(|app| app.pid == pid)
        {
            return Err("That app has closed. Refresh the running app list.".into());
        }
        let selected: HashSet<_> = snapshot
            .apps
            .iter()
            .filter(|app| target_pid.map_or(app.pid != std::process::id(), |pid| app.pid == pid))
            .take(MAX_APPS)
            .map(|app| app.pid)
            .collect();
        if target_pid.is_none() && snapshot.apps.len() > MAX_APPS {
            snapshot.limit_controls();
        }
        let mut queue = DiscoveryQueue::new();
        for (index, (root, id, pid)) in roots.into_iter().enumerate() {
            if !selected.contains(&pid) {
                continue;
            }
            queue.push(&id, (root, id.clone(), format!("window/{index}"), 0));
        }
        scan_windows(&mut snapshot, &walker, queue);
    }
    if target_pid.is_none() {
        snapshot.warn("Desktop-wide scans omit Decider itself. Select Decider explicitly to inspect its controls.");
    }
    snapshot.warn("This is a snapshot of standard UI Automation capabilities. Elevated apps, secure desktop, custom controls, and hidden commands may be unavailable. No actions were executed.");
    Ok(snapshot)
}

fn scan_windows(
    snapshot: &mut DesktopSnapshot,
    walker: &IUIAutomationTreeWalker,
    mut queue: DiscoveryQueue<(IUIAutomationElement, String, String, usize)>,
) {
    while let Some((element, app_id, path, depth)) = queue.pop() {
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
                    app_id: app_id.clone(),
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
                snapshot.limit_controls();
                continue;
            }
            let mut next = walker.GetFirstChildElement(&element).ok();
            let mut index = 0;
            while let Some(child) = next {
                if !snapshot.has_budget() || queue.len() + snapshot.nodes_visited >= MAX_NODES {
                    snapshot.limit_controls();
                    break;
                }
                next = walker.GetNextSiblingElement(&child).ok();
                queue.push(
                    &app_id,
                    (child, app_id.clone(), format!("{path}/{index}"), depth + 1),
                );
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

pub struct CommandContext;
impl Drop for CommandContext {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}
pub fn command_context() -> Result<CommandContext, String> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(|_| "Could not initialize Windows controls.")?;
    }
    Ok(CommandContext)
}
fn command_automation() -> Result<IUIAutomation, String> {
    unsafe {
        let automation: IUIAutomation2 =
            CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
                .map_err(|_| "Windows UI Automation is unavailable.")?;
        automation
            .SetConnectionTimeout(250)
            .map_err(|_| "Could not set control time limits.")?;
        automation
            .SetTransactionTimeout(250)
            .map_err(|_| "Could not set control time limits.")?;
        automation
            .cast()
            .map_err(|_| "Windows UI Automation is unavailable.".into())
    }
}

pub fn open_app(app: &crate::commands::AppCandidate) -> Result<(), String> {
    use ::windows::Win32::UI::WindowsAndMessaging::{
        IsIconic, SW_RESTORE, SW_SHOWNORMAL, SetForegroundWindow, ShowWindow,
    };
    if let Some(pid) = app.pid {
        let automation = command_automation()?;
        unsafe {
            let desktop = automation
                .GetRootElement()
                .map_err(|_| "Could not inspect desktop windows.")?;
            let walker = automation
                .ControlViewWalker()
                .map_err(|_| "Could not inspect desktop windows.")?;
            let mut next = walker.GetFirstChildElement(&desktop).ok();
            for _ in 0..512 {
                let Some(window) = next else {
                    break;
                };
                next = walker.GetNextSiblingElement(&window).ok();
                if window.CurrentProcessId().ok() != Some(pid as i32) {
                    continue;
                }
                if let Ok(handle) = window.CurrentNativeWindowHandle() {
                    if IsIconic(handle).as_bool() {
                        let _ = ShowWindow(handle, SW_RESTORE);
                    }
                    if SetForegroundWindow(handle).as_bool() {
                        return Ok(());
                    }
                }
            }
        }
        return Err("Windows could not activate the app. Open its window and try again.".into());
    }
    use ::windows::Win32::System::Com::{IPersistFile, STGM_READ};
    use ::windows::Win32::UI::Shell::{IShellLinkW, ShellExecuteW, ShellLink};
    use ::windows::core::{HSTRING, PCWSTR};
    let path = app.path.as_ref().ok_or("The app has no launch target.")?;
    if path
        .extension()
        .is_none_or(|ext| !ext.eq_ignore_ascii_case("lnk"))
        || !path.is_file()
    {
        return Err("The app shortcut is no longer available.".into());
    }
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .map_err(|_| "Could not inspect the installed shortcut.")?;
        let persist: IPersistFile = link
            .cast()
            .map_err(|_| "Could not inspect the installed shortcut.")?;
        persist
            .Load(&HSTRING::from(path.as_os_str()), STGM_READ)
            .map_err(|_| "Could not read the installed shortcut.")?;
        let mut buffer = [0u16; 32768];
        link.GetPath(&mut buffer, std::ptr::null_mut(), 0)
            .map_err(|_| "Could not resolve the application executable.")?;
        let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        let target = std::path::PathBuf::from(String::from_utf16_lossy(&buffer[..end]));
        let mut arguments = [0u16; 32768];
        link.GetArguments(&mut arguments)
            .map_err(|_| "Could not inspect shortcut arguments.")?;
        if arguments[0] != 0
            || !target.is_absolute()
            || !target.is_file()
            || target
                .extension()
                .is_none_or(|ext| !ext.eq_ignore_ascii_case("exe"))
            || target.to_string_lossy().starts_with("\\\\")
        {
            return Err("This shortcut requires arguments or does not point to a local app executable. Open it manually; command launching supports plain app shortcuts.".into());
        }
        let target = HSTRING::from(target.as_os_str());
        let result = ShellExecuteW(
            None,
            ::windows::core::w!("open"),
            &target,
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
        if result.0 as isize > 32 {
            Ok(())
        } else {
            Err("Windows rejected the app launch request.".into())
        }
    }
}

pub struct Control {
    pub description: String,
    element: IUIAutomationElement,
    pid: u32,
    role: i32,
    label: String,
    operation: &'static str,
}
fn command_label(element: &IUIAutomationElement) -> String {
    unsafe {
        element
            .CurrentName()
            .map(|label| text(&label.to_string()))
            .unwrap_or_default()
    }
}
fn available(element: &IUIAutomationElement, pid: u32) -> bool {
    unsafe {
        element.CurrentProcessId().ok() == Some(pid as i32)
            && element
                .CurrentIsPassword()
                .is_ok_and(|value| !value.as_bool())
            && element
                .CurrentIsEnabled()
                .is_ok_and(|value| value.as_bool())
            && element
                .CurrentIsOffscreen()
                .is_ok_and(|value| !value.as_bool())
    }
}

pub fn command_controls(pid: u32) -> Result<(Vec<Control>, bool), String> {
    let automation = command_automation()?;
    let mut controls = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    let started = Instant::now();
    let mut visited = 0;
    let mut truncated = false;
    unsafe {
        let walker = automation
            .ControlViewWalker()
            .map_err(|_| "Could not inspect app controls.")?;
        let desktop = automation
            .GetRootElement()
            .map_err(|_| "Could not inspect desktop windows.")?;
        let mut next = walker.GetFirstChildElement(&desktop).ok();
        for _ in 0..512 {
            let Some(window) = next else {
                break;
            };
            next = walker.GetNextSiblingElement(&window).ok();
            if window.CurrentProcessId().ok() == Some(pid as i32) {
                queue.push_back((window, String::new(), 0));
            }
            if started.elapsed() >= Duration::from_secs(8) {
                return Ok((controls, true));
            }
        }
        while let Some((element, context, depth)) = queue.pop_front() {
            if visited >= MAX_NODES
                || controls.len() >= 250
                || started.elapsed() >= Duration::from_secs(8)
            {
                return Ok((controls, true));
            }
            visited += 1;
            if element
                .CurrentIsPassword()
                .map(|v| v.as_bool())
                .unwrap_or(true)
            {
                continue;
            }
            let label = command_label(&element);
            let role = element
                .CurrentControlType()
                .map(|value| value.0)
                .unwrap_or_default();
            if !label.is_empty() && available(&element, pid) {
                let mut operations = Vec::new();
                for (pattern, operation) in [
                    (UIA_InvokePatternId, "Invoke"),
                    (UIA_TogglePatternId, "Toggle"),
                    (UIA_SelectionItemPatternId, "Select"),
                    (UIA_ExpandCollapsePatternId, "Expand"),
                    (UIA_ExpandCollapsePatternId, "Collapse"),
                ] {
                    if element.GetCurrentPattern(pattern).is_ok() {
                        operations.push(operation);
                    }
                }
                if element
                    .CurrentIsKeyboardFocusable()
                    .is_ok_and(|value| value.as_bool())
                {
                    operations.push("Focus");
                }
                for operation in operations {
                    if controls.len() >= 250 {
                        truncated = true;
                        break;
                    }
                    controls.push(Control {
                        description: format!(
                            "{operation} ‘{label}’ ({}){context}",
                            element
                                .CurrentLocalizedControlType()
                                .map(|value| text(&value.to_string()))
                                .unwrap_or_default()
                        ),
                        element: element.clone(),
                        pid,
                        role,
                        label: label.clone(),
                        operation,
                    });
                }
            }
            if depth >= MAX_DEPTH {
                truncated = true;
                continue;
            }
            let context = if role == UIA_WindowControlTypeId.0 && !label.is_empty() {
                format!(" in {}", text(&label))
            } else {
                context
            };
            let mut next = walker.GetFirstChildElement(&element).ok();
            while let Some(child) = next {
                if queue.len() + visited >= MAX_NODES || started.elapsed() >= Duration::from_secs(8)
                {
                    truncated = true;
                    break;
                }
                next = walker.GetNextSiblingElement(&child).ok();
                queue.push_back((child, context.clone(), depth + 1));
            }
        }
    }
    Ok((controls, truncated))
}

pub fn execute_control(control: &Control) -> Result<(), String> {
    let element = &control.element;
    if !available(element, control.pid)
        || command_label(element) != control.label
        || unsafe {
            element
                .CurrentControlType()
                .map(|value| value.0)
                .unwrap_or_default()
        } != control.role
    {
        return Err(
            "The control changed, closed, or became unavailable. Speak the command again.".into(),
        );
    }
    let result = unsafe {
        match control.operation {
            "Invoke" => element
                .GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
                .and_then(|p| p.Invoke()),
            "Toggle" => element
                .GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId)
                .and_then(|p| p.Toggle()),
            "Select" => element
                .GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                    UIA_SelectionItemPatternId,
                )
                .and_then(|p| p.Select()),
            "Expand" => element
                .GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(
                    UIA_ExpandCollapsePatternId,
                )
                .and_then(|p| p.Expand()),
            "Collapse" => element
                .GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(
                    UIA_ExpandCollapsePatternId,
                )
                .and_then(|p| p.Collapse()),
            "Focus" => element.SetFocus(),
            _ => return Err("Unsupported control operation.".into()),
        }
    };
    result.map_err(|_| "The app rejected the control action. Nothing else was attempted.".into())
}
