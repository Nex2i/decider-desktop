use super::*;
use core_foundation::array::{
    CFArrayGetCount, CFArrayGetTypeID, CFArrayGetValueAtIndex, CFArrayRef,
};
use core_foundation::base::{CFGetTypeID, CFHash, CFRelease, CFRetain, CFTypeRef, TCFType};
use core_foundation::boolean::{CFBoolean, CFBooleanGetTypeID};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringGetTypeID, CFStringRef};
use objc2_app_kit::{NSApplicationActivationPolicy, NSWorkspace};
use std::ptr;

// Discovery calls are separate from the command executor below. The inspector
// never invokes the executor or changes accessibility values.
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> u8;
    fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementCopyActionNames(element: CFTypeRef, names: *mut CFArrayRef) -> i32;
    fn AXUIElementCopyAttributeNames(element: CFTypeRef, names: *mut CFArrayRef) -> i32;
    fn AXUIElementIsAttributeSettable(
        element: CFTypeRef,
        attribute: CFStringRef,
        settable: *mut u8,
    ) -> i32;
    fn AXUIElementSetMessagingTimeout(element: CFTypeRef, timeout: f32) -> i32;
}

struct Owned(CFTypeRef);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) };
    }
}

fn attribute(element: CFTypeRef, name: &str) -> Option<Owned> {
    let key = CFString::new(name);
    let mut value = ptr::null();
    let result =
        unsafe { AXUIElementCopyAttributeValue(element, key.as_concrete_TypeRef(), &mut value) };
    (result == 0 && !value.is_null()).then(|| Owned(value))
}

fn string(value: CFTypeRef) -> Option<String> {
    if value.is_null() || unsafe { CFGetTypeID(value) != CFStringGetTypeID() } {
        return None;
    }
    Some(unsafe { CFString::wrap_under_get_rule(value.cast()) }.to_string())
}

fn string_attribute(element: CFTypeRef, name: &str) -> Option<String> {
    attribute(element, name).and_then(|value| string(value.0))
}

fn boolean_attribute(element: CFTypeRef, name: &str) -> Option<bool> {
    let value = attribute(element, name)?;
    if unsafe { CFGetTypeID(value.0) != CFBooleanGetTypeID() } {
        return None;
    }
    Some(bool::from(unsafe {
        CFBoolean::wrap_under_get_rule(value.0.cast())
    }))
}

fn children(value: &Owned, limit: usize) -> (Vec<Owned>, bool) {
    if unsafe { CFGetTypeID(value.0) != CFArrayGetTypeID() } {
        return (vec![], false);
    }
    let array: CFArrayRef = value.0.cast();
    let count = unsafe { CFArrayGetCount(array) }.max(0) as usize;
    let values = (0..count.min(limit))
        .filter_map(|index| {
            let value = unsafe { CFArrayGetValueAtIndex(array, index as isize) };
            (!value.is_null()).then(|| Owned(unsafe { CFRetain(value) }))
        })
        .collect();
    (values, count > limit)
}

fn names(element: CFTypeRef, actions: bool, snapshot: &mut DesktopSnapshot) -> Vec<String> {
    let mut array = ptr::null();
    let result = unsafe {
        if actions {
            AXUIElementCopyActionNames(element, &mut array)
        } else {
            AXUIElementCopyAttributeNames(element, &mut array)
        }
    };
    if result != 0 || array.is_null() {
        snapshot.warn("Some elements did not expose action or attribute information; this can be an app limitation or a read failure.");
        return vec![];
    }
    let owned = Owned(array.cast());
    let (values, truncated) = children(&owned, 256);
    if truncated {
        snapshot.limit_controls();
    }
    values.iter().filter_map(|value| string(value.0)).collect()
}

pub fn request_access() -> bool {
    let options = CFDictionary::from_CFType_pairs(&[(
        CFString::new("AXTrustedCheckOptionPrompt"),
        CFBoolean::true_value(),
    )]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) != 0 }
}

pub fn inspect(
    target_pid: Option<u32>,
    include_labels: bool,
    inventory_only: bool,
) -> Result<DesktopSnapshot, String> {
    let mut snapshot = DesktopSnapshot::new(include_labels);
    let mut folders = vec![
        "/Applications".into(),
        "/System/Applications".into(),
        "/System/Library/CoreServices".into(),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        folders.push(std::path::PathBuf::from(home).join("Applications"));
    }
    let (installed, truncated) = installed_in_folders(&folders, "app");
    snapshot.installed_apps = installed;
    snapshot.truncated |= truncated;
    snapshot.inventory_truncated = truncated;
    if truncated {
        snapshot.warn("The installed-app inventory reached its folder/entry limit. This limit is independent of the control scan.");
    }
    snapshot.warn("Installed app inventory covers standard Applications folders; portable apps in other folders may be missing.");
    let workspace = NSWorkspace::sharedWorkspace();
    for app in workspace.runningApplications().iter() {
        if app.activationPolicy() != NSApplicationActivationPolicy::Regular {
            continue;
        }
        let pid = app.processIdentifier();
        if pid <= 0 {
            continue;
        }
        snapshot.apps.push(DesktopApp {
            id: format!(
                "mac:{}:{pid}",
                app.bundleIdentifier()
                    .map(|name| name.to_string())
                    .unwrap_or_default()
            ),
            name: text(
                &app.localizedName()
                    .map(|name| name.to_string())
                    .unwrap_or_else(|| "Application".into()),
            ),
            pid: pid as u32,
            foreground: app.isActive(),
        });
    }
    snapshot
        .apps
        .sort_by_key(|app| (!app.foreground, app.name.to_lowercase()));
    if let Some(pid) = target_pid
        && !snapshot.apps.iter().any(|app| app.pid == pid)
    {
        return Err("That app has closed. Refresh the running app list.".into());
    }
    if inventory_only {
        return Ok(snapshot);
    }
    snapshot.accessibility_available = unsafe { AXIsProcessTrusted() != 0 };
    if !snapshot.accessibility_available {
        snapshot.warn("Accessibility access is required to discover other apps' controls, menus, and writable attributes. Enable Decider Desktop in System Settings → Privacy & Security → Accessibility, then scan again.");
        return Ok(snapshot);
    }
    snapshot.started = Some(Instant::now());
    let apps: Vec<_> = snapshot
        .apps
        .iter()
        .filter(|app| target_pid.map_or(app.pid != std::process::id(), |pid| app.pid == pid))
        .map(|app| (app.id.clone(), app.pid))
        .collect();
    if apps.len() > MAX_APPS {
        snapshot.limit_controls();
    }
    let mut queue = DiscoveryQueue::new();
    for (id, pid) in apps.into_iter().take(MAX_APPS) {
        if !snapshot.has_budget() {
            break;
        }
        let root = unsafe { AXUIElementCreateApplication(pid as i32) };
        if root.is_null() {
            snapshot.warn("An app became unavailable during discovery.");
            continue;
        }
        unsafe { AXUIElementSetMessagingTimeout(root, 0.08) };
        queue.push(&id, (Owned(root), id.clone(), String::from("app"), 0));
    }
    scan_queue(&mut snapshot, queue);
    if target_pid.is_none() {
        snapshot.warn("Desktop-wide scans omit Decider itself to avoid inspecting the inspector's results. Select Decider explicitly to inspect its controls.");
    }
    snapshot.warn("This is a snapshot of OS-exposed capabilities, not a guarantee of execution. Hidden app commands, custom canvases, protected controls, and permission-restricted apps may be absent.");
    Ok(snapshot)
}

fn scan_queue(
    snapshot: &mut DesktopSnapshot,
    mut queue: DiscoveryQueue<(Owned, String, String, usize)>,
) {
    let mut seen = HashSet::new();
    while let Some((element, app_id, path, depth)) = queue.pop() {
        if !snapshot.has_budget() {
            break;
        }
        if !seen.insert((app_id.clone(), unsafe { CFHash(element.0) })) {
            continue;
        }
        snapshot.nodes_visited += 1;
        unsafe { AXUIElementSetMessagingTimeout(element.0, 0.08) };
        let Some(role) = string_attribute(element.0, "AXRole") else {
            snapshot.warn("Some accessibility elements could not be read; apps may have changed or denied access.");
            continue;
        };
        let subrole = string_attribute(element.0, "AXSubrole").unwrap_or_default();
        if subrole.contains("Secure") || role.contains("Secure") {
            continue;
        }
        let enabled = boolean_attribute(element.0, "AXEnabled");
        let focused = boolean_attribute(element.0, "AXFocused").unwrap_or(false);
        let mut capabilities = names(element.0, true, snapshot)
            .into_iter()
            .map(|name| Capability {
                operation: name.strip_prefix("AX").unwrap_or(&name).into(),
                native_name: name,
                kind: "action",
                available: enabled == Some(true),
            })
            .collect::<Vec<_>>();
        // Discover all writable attribute names, but never read their values.
        for name in names(element.0, false, snapshot) {
            if !snapshot.has_budget() {
                break;
            }
            let key = CFString::new(&name);
            let mut settable = 0;
            if unsafe {
                AXUIElementIsAttributeSettable(element.0, key.as_concrete_TypeRef(), &mut settable)
            } == 0
                && settable != 0
            {
                capabilities.push(Capability {
                    operation: format!("Set {}", name.strip_prefix("AX").unwrap_or(&name)),
                    native_name: name,
                    kind: "writable_attribute",
                    available: enabled == Some(true),
                });
            }
        }
        if !capabilities.is_empty() {
            let label = snapshot
                .labels_included
                .then(|| {
                    string_attribute(element.0, "AXTitle")
                        .filter(|value| !value.is_empty())
                        .or_else(|| string_attribute(element.0, "AXDescription"))
                        .map(|value| text(&value))
                })
                .flatten();
            snapshot.targets.push(DesktopTarget {
                id: identifier(&format!("{}:{app_id}:{path}", snapshot.captured_at)),
                app_id: app_id.clone(),
                role,
                label,
                enabled,
                focused,
                capabilities,
            });
        }
        for attribute_name in ["AXChildren", "AXWindows", "AXMenuBar"] {
            if !snapshot.has_budget() {
                break;
            }
            let Some(value) = attribute(element.0, attribute_name) else {
                continue;
            };
            if depth >= MAX_DEPTH {
                snapshot.limit_controls();
                continue;
            }
            if attribute_name == "AXMenuBar" {
                queue.push(
                    &app_id,
                    (value, app_id.clone(), format!("{path}/menu"), depth + 1),
                );
            } else {
                let remaining = MAX_NODES.saturating_sub(snapshot.nodes_visited + queue.len());
                let (values, truncated) = children(&value, remaining);
                snapshot.truncated |= truncated;
                snapshot.controls_truncated |= truncated;
                for (index, child) in values.into_iter().enumerate() {
                    queue.push(
                        &app_id,
                        (
                            child,
                            app_id.clone(),
                            format!("{path}/{attribute_name}/{index}"),
                            depth + 1,
                        ),
                    );
                }
            }
        }
    }
}

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn LSOpenCFURLRef(
        url: core_foundation::url::CFURLRef,
        launched: *mut core_foundation::url::CFURLRef,
    ) -> i32;
}
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementPerformAction(element: CFTypeRef, action: CFStringRef) -> i32;
    fn AXUIElementSetAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> i32;
    fn AXUIElementGetPid(element: CFTypeRef, pid: *mut i32) -> i32;
}

pub fn open_app(app: &crate::commands::AppCandidate) -> Result<(), String> {
    if let Some(pid) = app.pid {
        let running = objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(
            pid as i32,
        )
        .ok_or("The selected app has closed. Speak the command again.")?;
        #[allow(deprecated)]
        let activated = running.activateWithOptions(
            objc2_app_kit::NSApplicationActivationOptions::ActivateIgnoringOtherApps,
        );
        return if activated {
            Ok(())
        } else {
            Err("macOS could not activate the app.".into())
        };
    }
    let path = app.path.as_ref().ok_or("The app has no launch target.")?;
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| "The installed app is no longer available.")?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || path.extension().is_none_or(|ext| ext != "app")
    {
        return Err("The selected target is not an application bundle.".into());
    }
    let url = core_foundation::url::CFURL::from_path(path, true)
        .ok_or("Could not resolve the app bundle.")?;
    if unsafe { LSOpenCFURLRef(url.as_concrete_TypeRef(), ptr::null_mut()) } == 0 {
        Ok(())
    } else {
        Err("macOS rejected the app launch request.".into())
    }
}

pub struct Control {
    pub description: String,
    element: Owned,
    pid: u32,
    role: String,
    label: String,
    operation: &'static str,
}
fn control_label(element: CFTypeRef) -> String {
    string_attribute(element, "AXTitle")
        .filter(|label| !label.is_empty())
        .or_else(|| string_attribute(element, "AXDescription"))
        .map(|label| text(&label))
        .unwrap_or_default()
}

pub fn command_controls(pid: u32) -> Result<(Vec<Control>, bool), String> {
    if unsafe { AXIsProcessTrusted() } == 0 {
        return Err("Grant Decider access in macOS Privacy & Security → Device Control and Data Access, then retry.".into());
    }
    let root = unsafe { AXUIElementCreateApplication(pid as i32) };
    if root.is_null() {
        return Err("The app is no longer available.".into());
    }
    unsafe { AXUIElementSetMessagingTimeout(root, 0.08) };
    let mut queue = std::collections::VecDeque::from([(Owned(root), String::new(), 0)]);
    let mut seen = HashSet::new();
    let mut snapshot = DesktopSnapshot::new(true);
    snapshot.started = Some(Instant::now());
    let mut controls = Vec::new();
    while let Some((element, context, depth)) = queue.pop_front() {
        if snapshot.nodes_visited >= MAX_NODES
            || controls.len() >= 250
            || snapshot
                .started
                .is_some_and(|start| start.elapsed() >= Duration::from_secs(8))
        {
            return Ok((controls, true));
        }
        if !seen.insert(unsafe { CFHash(element.0) }) {
            continue;
        }
        snapshot.nodes_visited += 1;
        unsafe { AXUIElementSetMessagingTimeout(element.0, 0.08) };
        let Some(role) = string_attribute(element.0, "AXRole") else {
            continue;
        };
        if role.contains("Secure")
            || string_attribute(element.0, "AXSubrole").is_some_and(|role| role.contains("Secure"))
            || boolean_attribute(element.0, "AXHidden") == Some(true)
        {
            continue;
        }
        let label = control_label(element.0);
        if boolean_attribute(element.0, "AXEnabled") == Some(true) && !label.is_empty() {
            let actions = names(element.0, true, &mut snapshot);
            for operation in ["AXPress", "AXShowMenu", "AXRaise", "AXFocused"] {
                let supported = if operation == "AXFocused" {
                    let name = CFString::new(operation);
                    let mut writable = 0;
                    unsafe {
                        AXUIElementIsAttributeSettable(
                            element.0,
                            name.as_concrete_TypeRef(),
                            &mut writable,
                        ) == 0
                            && writable != 0
                    }
                } else {
                    actions.iter().any(|name| name == operation)
                };
                if supported && controls.len() < 250 {
                    controls.push(Control {
                        description: format!(
                            "{} ‘{}’ ({role}){}",
                            if operation == "AXFocused" {
                                "Focus"
                            } else {
                                operation.strip_prefix("AX").unwrap_or(operation)
                            },
                            label,
                            if context.is_empty() {
                                String::new()
                            } else {
                                format!(" in {context}")
                            }
                        ),
                        element: Owned(unsafe { CFRetain(element.0) }),
                        pid,
                        role: role.clone(),
                        label: label.clone(),
                        operation,
                    });
                }
            }
        }
        if depth >= MAX_DEPTH {
            snapshot.limit_controls();
            continue;
        }
        let child_context = if ["AXWindow", "AXMenuBarItem", "AXMenuItem"].contains(&role.as_str())
            && !label.is_empty()
        {
            text(&format!("{context} / {label}"))
        } else {
            context
        };
        for name in ["AXChildren", "AXWindows", "AXMenuBar"] {
            if let Some(value) = attribute(element.0, name) {
                if name == "AXMenuBar" {
                    queue.push_back((value, child_context.clone(), depth + 1));
                } else {
                    let remaining = MAX_NODES.saturating_sub(snapshot.nodes_visited + queue.len());
                    let (items, partial) = children(&value, remaining);
                    snapshot.controls_truncated |= partial;
                    for child in items {
                        queue.push_back((child, child_context.clone(), depth + 1));
                    }
                }
            }
        }
    }
    Ok((controls, snapshot.controls_truncated))
}

pub fn execute_control(control: &Control) -> Result<(), String> {
    let element = control.element.0;
    let mut pid = 0;
    if unsafe { AXIsProcessTrusted() } == 0
        || unsafe { AXUIElementGetPid(element, &mut pid) } != 0
        || pid as u32 != control.pid
        || string_attribute(element, "AXRole").as_deref() != Some(control.role.as_str())
        || control_label(element) != control.label
        || boolean_attribute(element, "AXEnabled") != Some(true)
        || boolean_attribute(element, "AXHidden") == Some(true)
        || string_attribute(element, "AXSubrole").is_some_and(|role| role.contains("Secure"))
    {
        return Err(
            "The control changed, closed, or lost permission. Speak the command again.".into(),
        );
    }
    let name = CFString::new(control.operation);
    let status = if control.operation == "AXFocused" {
        let mut writable = 0;
        if unsafe {
            AXUIElementIsAttributeSettable(element, name.as_concrete_TypeRef(), &mut writable)
        } != 0
            || writable == 0
        {
            return Err("The control can no longer receive focus.".into());
        }
        unsafe {
            AXUIElementSetAttributeValue(
                element,
                name.as_concrete_TypeRef(),
                CFBoolean::true_value().as_CFTypeRef(),
            )
        }
    } else {
        let mut snapshot = DesktopSnapshot::new(false);
        if !names(element, true, &mut snapshot)
            .iter()
            .any(|action| action == control.operation)
        {
            return Err("The selected operation is no longer supported.".into());
        }
        unsafe { AXUIElementPerformAction(element, name.as_concrete_TypeRef()) }
    };
    if status == 0 {
        Ok(())
    } else {
        Err("The app rejected the control action. Nothing else was attempted.".into())
    }
}

pub struct CommandContext;
pub fn command_context() -> Result<CommandContext, String> {
    Ok(CommandContext)
}
