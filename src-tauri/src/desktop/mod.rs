use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Default)]
pub struct DesktopState(Arc<AtomicBool>);
struct ScanLease(DesktopState);
impl Drop for ScanLease {
    fn drop(&mut self) {
        self.0.0.store(false, Ordering::Release);
    }
}

#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(target_os = "windows")]
pub(crate) mod windows;

const MAX_NODES: usize = 3000;
const MAX_TARGETS: usize = 1500;
const MAX_APPS: usize = 40;
const MAX_DEPTH: usize = 24;
const SCAN_TIME: Duration = Duration::from_secs(12);

/// Visit one element per app in turn, so a large UI cannot consume the entire
/// desktop-wide budget before smaller apps get inspected.
struct DiscoveryQueue<T> {
    groups: std::collections::HashMap<String, usize>,
    queues: Vec<std::collections::VecDeque<T>>,
    cursor: usize,
    pending: usize,
}
impl<T> DiscoveryQueue<T> {
    fn new() -> Self {
        Self {
            groups: Default::default(),
            queues: vec![],
            cursor: 0,
            pending: 0,
        }
    }
    fn push(&mut self, group: &str, value: T) {
        let index = *self.groups.entry(group.into()).or_insert_with(|| {
            self.queues.push(Default::default());
            self.queues.len() - 1
        });
        self.queues[index].push_back(value);
        self.pending += 1;
    }
    fn pop(&mut self) -> Option<T> {
        for _ in 0..self.queues.len() {
            let index = self.cursor;
            self.cursor = (self.cursor + 1) % self.queues.len();
            if let Some(value) = self.queues[index].pop_front() {
                self.pending -= 1;
                return Some(value);
            }
        }
        None
    }
    fn len(&self) -> usize {
        self.pending
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopApp {
    id: String,
    name: String,
    pid: u32,
    foreground: bool,
}

#[derive(Serialize)]
pub struct InstalledApp {
    #[serde(skip)]
    pub(crate) path: std::path::PathBuf,
    id: String,
    name: String,
    source: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    native_name: String,
    operation: String,
    kind: &'static str,
    available: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopTarget {
    id: String,
    app_id: String,
    role: String,
    label: Option<String>,
    enabled: Option<bool>,
    focused: bool,
    capabilities: Vec<Capability>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSnapshot {
    platform: &'static str,
    captured_at: u64,
    accessibility_available: bool,
    labels_included: bool,
    apps: Vec<DesktopApp>,
    installed_apps: Vec<InstalledApp>,
    targets: Vec<DesktopTarget>,
    nodes_visited: usize,
    truncated: bool,
    inventory_truncated: bool,
    controls_truncated: bool,
    warnings: Vec<String>,
    execution_available: bool,
    #[serde(skip)]
    started: Option<Instant>,
}

impl DesktopSnapshot {
    fn new(labels_included: bool) -> Self {
        Self {
            platform: std::env::consts::OS,
            captured_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            accessibility_available: false,
            labels_included,
            apps: Vec::new(),
            installed_apps: Vec::new(),
            targets: Vec::new(),
            nodes_visited: 0,
            truncated: false,
            inventory_truncated: false,
            controls_truncated: false,
            warnings: Vec::new(),
            execution_available: false,
            started: None,
        }
    }

    fn has_budget(&mut self) -> bool {
        let available = self.nodes_visited < MAX_NODES
            && self.targets.len() < MAX_TARGETS
            && self.started.is_none_or(|start| start.elapsed() < SCAN_TIME);
        if !available {
            self.limit_controls();
        }
        available
    }

    fn limit_controls(&mut self) {
        self.truncated = true;
        self.controls_truncated = true;
    }

    fn warn(&mut self, warning: &str) {
        if !self.warnings.iter().any(|existing| existing == warning) {
            self.warnings.push(warning.into());
        }
    }
}

fn identifier(value: &str) -> String {
    Sha256::digest(value.as_bytes())[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn text(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(160)
        .collect()
}

fn installed_in_folders(
    folders: &[std::path::PathBuf],
    extension: &str,
) -> (Vec<InstalledApp>, bool) {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let mut queue: std::collections::VecDeque<_> =
        folders.iter().cloned().map(|path| (path, 0)).collect();
    let mut visited = 0;
    let mut truncated = false;
    while let Some((folder, depth)) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > 15_000 || result.len() >= 1500 {
                return (result, true);
            }
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if path
                .extension()
                .is_some_and(|value| value.eq_ignore_ascii_case(extension))
                && ((extension == "app" && kind.is_dir()) || (extension == "lnk" && kind.is_file()))
            {
                let key = path.to_string_lossy();
                if seen.insert(key.to_lowercase()) {
                    result.push(InstalledApp {
                        id: identifier(&key),
                        path: path.clone(),
                        name: text(&path.file_stem().unwrap_or_default().to_string_lossy()),
                        source: if extension == "app" {
                            "Application bundle"
                        } else {
                            "Start menu shortcut"
                        }
                        .into(),
                    });
                }
            } else if kind.is_dir() && depth < 5 {
                queue.push_back((path, depth + 1));
            } else if kind.is_dir() {
                truncated = true;
            }
        }
    }
    result.sort_by_key(|app| app.name.to_lowercase());
    (result, truncated)
}

#[tauri::command]
pub async fn inspect_desktop(
    state: tauri::State<'_, DesktopState>,
    target_pid: Option<u32>,
    include_labels: bool,
) -> Result<DesktopSnapshot, String> {
    state
        .0
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .map_err(|_| "A desktop scan is already running. Wait for it to finish.")?;
    let lease = ScanLease(state.inner().clone());
    tauri::async_runtime::spawn_blocking(move || {
        let _lease = lease;
        #[cfg(target_os = "macos")]
        {
            macos::inspect(target_pid, include_labels, false)
        }
        #[cfg(target_os = "windows")]
        {
            windows::inspect(target_pid, include_labels, false)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (target_pid, include_labels);
            Err("Desktop discovery currently supports macOS and Windows.".into())
        }
    })
    .await
    .map_err(|_| "Desktop discovery stopped unexpectedly. Please retry.".to_string())?
}

#[tauri::command]
pub async fn request_desktop_access() -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        Ok(macos::request_access())
    }
    #[cfg(target_os = "windows")]
    {
        Ok(true)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err("Desktop discovery currently supports macOS and Windows.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_inventory_keeps_duplicate_installs_and_does_not_serialize_launch_paths() {
        let mut snapshot = DesktopSnapshot::new(false);
        for path in [
            "/Applications/Example.app",
            "/Users/example/Applications/Example.app",
        ] {
            snapshot.installed_apps.push(InstalledApp {
                id: identifier(path),
                name: "Example".into(),
                source: "Application bundle".into(),
                path: path.into(),
            });
        }
        let serialized = serde_json::to_string(&snapshot).unwrap();
        assert!(!serialized.contains("/Applications"));
        assert_eq!(
            command_apps(snapshot).len(),
            2,
            "Duplicate names must remain visible for ambiguity rejection"
        );
    }

    #[test]
    fn discovery_limits_are_explicit_and_ids_do_not_expose_paths() {
        let id = identifier("/Users/example/Applications/Example.app");
        assert_eq!(id.len(), 24);
        assert!(!id.contains("example"));
        let mut snapshot = DesktopSnapshot::new(false);
        snapshot.nodes_visited = MAX_NODES;
        assert!(!snapshot.has_budget());
        assert!(snapshot.truncated);
        assert!(!snapshot.execution_available);
    }

    #[test]
    fn app_inventory_does_not_follow_symlinks_or_scan_inside_bundles() {
        let root = std::env::temp_dir().join(format!("decider-inventory-{}", std::process::id()));
        std::fs::create_dir_all(root.join("Example.app/Contents/Hidden.app")).unwrap();
        std::fs::create_dir_all(root.join("Utilities/Other.app")).unwrap();
        std::fs::write(root.join("NotAnApplication.app"), "plain file").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("Utilities"), root.join("Alias")).unwrap();
        let (apps, truncated) = installed_in_folders(std::slice::from_ref(&root), "app");
        std::fs::remove_dir_all(root).unwrap();
        assert!(!truncated);
        assert_eq!(apps.len(), 2);
        assert!(apps.iter().any(|app| app.name == "Example"));
        assert!(!apps.iter().any(|app| app.name == "Hidden"));
    }

    #[test]
    fn dropping_scan_lease_allows_retry() {
        let state = DesktopState::default();
        state.0.store(true, Ordering::Release);
        drop(ScanLease(state.clone()));
        assert!(
            state
                .0
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
        );
    }

    #[test]
    fn large_app_cannot_starve_other_apps() {
        let mut queue = DiscoveryQueue::new();
        queue.push("large", 1);
        queue.push("large", 2);
        queue.push("large", 3);
        queue.push("small", 4);
        assert_eq!(queue.pop(), Some(1));
        queue.push("large", 5);
        assert_eq!(queue.pop(), Some(4));
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.len(), 2);
    }
}

#[cfg(target_os = "macos")]
pub(crate) use macos as command_native;
#[cfg(target_os = "windows")]
pub(crate) use windows as command_native;

pub(crate) fn command_apps(snapshot: DesktopSnapshot) -> Vec<crate::commands::AppCandidate> {
    let mut apps: Vec<_> = snapshot
        .apps
        .into_iter()
        .filter(|app| app.pid != std::process::id())
        .map(|app| crate::commands::AppCandidate {
            name: app.name,
            pid: Some(app.pid),
            path: None,
        })
        .collect();
    for installed in snapshot.installed_apps {
        if !apps
            .iter()
            .any(|app| app.pid.is_some() && app.name.eq_ignore_ascii_case(&installed.name))
        {
            apps.push(crate::commands::AppCandidate {
                name: installed.name,
                pid: None,
                path: Some(installed.path),
            });
        }
    }
    apps
}
