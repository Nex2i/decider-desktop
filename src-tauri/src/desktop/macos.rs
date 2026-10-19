use super::*;
use core_foundation::array::{
    CFArrayGetCount, CFArrayGetTypeID, CFArrayGetValueAtIndex, CFArrayRef,
};
use core_foundation::base::{CFGetTypeID, CFHash, CFRelease, CFRetain, CFTypeRef, TCFType};
use core_foundation::boolean::{CFBoolean, CFBooleanGetTypeID};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringGetTypeID, CFStringRef};
use objc2_app_kit::{NSApplicationActivationPolicy, NSWorkspace};
use std::collections::VecDeque;
use std::ptr;

// Only discovery APIs are linked. No AX action-performing or value-setting API
// is exposed by this adapter.
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

fn names(element: CFTypeRef, actions: bool) -> Vec<String> {
    let mut array = ptr::null();
    let result = unsafe {
        if actions {
            AXUIElementCopyActionNames(element, &mut array)
        } else {
            AXUIElementCopyAttributeNames(element, &mut array)
        }
    };
    if result != 0 || array.is_null() {
        return vec![];
    }
    let owned = Owned(array.cast());
    children(&owned, 256)
        .0
        .iter()
        .filter_map(|value| string(value.0))
        .collect()
}

pub fn request_access() -> bool {
    let options = CFDictionary::from_CFType_pairs(&[(
        CFString::new("AXTrustedCheckOptionPrompt"),
        CFBoolean::true_value(),
    )]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) != 0 }
}

pub fn inspect(target_pid: Option<u32>, include_labels: bool) -> Result<DesktopSnapshot, String> {
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
    snapshot.accessibility_available = unsafe { AXIsProcessTrusted() != 0 };
    if !snapshot.accessibility_available {
        snapshot.warn("Accessibility access is required to discover other apps' controls, menus, and writable attributes. Enable Decider Desktop in System Settings → Privacy & Security → Accessibility, then scan again.");
        return Ok(snapshot);
    }
    snapshot.started = Some(Instant::now());
    let apps: Vec<_> = snapshot
        .apps
        .iter()
        .filter(|app| target_pid.is_none_or(|pid| app.pid == pid))
        .map(|app| (app.id.clone(), app.pid))
        .collect();
    if apps.len() > MAX_APPS {
        snapshot.truncated = true;
    }
    for (id, pid) in apps.into_iter().take(MAX_APPS) {
        if !snapshot.has_budget() {
            break;
        }
        scan_app(&mut snapshot, &id, pid);
    }
    snapshot.warn("This is a snapshot of OS-exposed capabilities, not a guarantee of execution. Hidden app commands, custom canvases, protected controls, and permission-restricted apps may be absent.");
    Ok(snapshot)
}

fn scan_app(snapshot: &mut DesktopSnapshot, app_id: &str, pid: u32) {
    let root = unsafe { AXUIElementCreateApplication(pid as i32) };
    if root.is_null() {
        snapshot.warn("An app became unavailable during discovery.");
        return;
    }
    let root = Owned(root);
    // Bound each remote accessibility message, as well as the total scan.
    unsafe { AXUIElementSetMessagingTimeout(root.0, 0.08) };
    let mut queue = VecDeque::from([(root, String::from("app"), 0)]);
    let mut seen = HashSet::new();
    while let Some((element, path, depth)) = queue.pop_front() {
        if !snapshot.has_budget() {
            break;
        }
        if !seen.insert(unsafe { CFHash(element.0) }) {
            continue;
        }
        snapshot.nodes_visited += 1;
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
        let mut capabilities = names(element.0, true)
            .into_iter()
            .map(|name| Capability {
                operation: name.strip_prefix("AX").unwrap_or(&name).into(),
                native_name: name,
                kind: "action",
                available: enabled == Some(true),
            })
            .collect::<Vec<_>>();
        // Discover all writable attribute names, but never read their values.
        for name in names(element.0, false) {
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
                app_id: app_id.into(),
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
                snapshot.truncated = true;
                continue;
            }
            if attribute_name == "AXMenuBar" {
                queue.push_back((value, format!("{path}/menu"), depth + 1));
            } else {
                let remaining = MAX_NODES.saturating_sub(snapshot.nodes_visited + queue.len());
                let (values, truncated) = children(&value, remaining);
                snapshot.truncated |= truncated;
                for (index, child) in values.into_iter().enumerate() {
                    queue.push_back((child, format!("{path}/{attribute_name}/{index}"), depth + 1));
                }
            }
        }
    }
}
