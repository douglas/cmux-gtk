//! Background port scanner — detects TCP listening ports per workspace.
//!
//! Periodically reads `/proc/net/tcp` and `/proc/net/tcp6` to find listening
//! sockets, resolves the owning PID via `/proc/*/fd`, and matches each PID's
//! cwd to workspace panel directories.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::app::{lock_or_recover, SharedState};

/// Scan interval in seconds.
const SCAN_INTERVAL_SECS: u64 = 5;

/// Start the background port scanner on a dedicated thread.
/// Runs every `SCAN_INTERVAL_SECS`, updating panel listening_ports via SharedState.
pub fn spawn(shared: Arc<SharedState>) {
    std::thread::Builder::new()
        .name("port-scanner".into())
        .spawn(move || scanner_loop(shared))
        .expect("Failed to spawn port scanner thread");
}

fn scanner_loop(shared: Arc<SharedState>) {
    loop {
        std::thread::sleep(std::time::Duration::from_secs(SCAN_INTERVAL_SECS));

        match scan_and_update(&shared) {
            Ok(changed) => {
                if changed {
                    shared.notify_ui_refresh();
                }
            }
            Err(e) => {
                tracing::debug!("Port scan error: {}", e);
            }
        }
    }
}

/// Run one scan cycle. Returns true if any panel's ports changed.
fn scan_and_update(shared: &Arc<SharedState>) -> Result<bool, Box<dyn std::error::Error>> {
    // Step 1: Collect workspace directories and their panel IDs.
    let workspace_dirs: Vec<(uuid::Uuid, String)> = {
        let tm = lock_or_recover(&shared.tab_manager);
        tm.iter()
            .flat_map(|ws| {
                ws.panels.values().filter_map(|panel| {
                    panel
                        .directory
                        .as_ref()
                        .map(|dir| (panel.id, dir.clone()))
                })
            })
            .collect()
    };

    if workspace_dirs.is_empty() {
        return Ok(false);
    }

    // Step 2: Read listening ports from /proc/net/tcp{,6}.
    let listening_inodes = read_listening_inodes()?;
    if listening_inodes.is_empty() {
        // Clear all ports if nothing is listening.
        return Ok(clear_all_ports(shared));
    }

    // Step 3: Map inodes → PIDs by scanning /proc/*/fd.
    let inode_to_pids = map_inodes_to_pids(&listening_inodes)?;

    // Step 4: Map PIDs → cwds.
    let pid_cwds = resolve_pid_cwds(&inode_to_pids);

    // Step 5: For each (inode, port), find matching panel via PID cwd → panel directory.
    let mut panel_ports: HashMap<uuid::Uuid, Vec<u16>> = HashMap::new();

    for (inode, port) in &listening_inodes {
        if let Some(pids) = inode_to_pids.get(inode) {
            for pid in pids {
                if let Some(cwd) = pid_cwds.get(pid) {
                    // Find any panel whose directory is a prefix of this PID's cwd.
                    for (panel_id, panel_dir) in &workspace_dirs {
                        if cwd.starts_with(panel_dir) || cwd == panel_dir {
                            panel_ports.entry(*panel_id).or_default().push(*port);
                        }
                    }
                }
            }
        }
    }

    // Step 6: Update panels and detect changes.
    let mut changed = false;
    let mut tm = lock_or_recover(&shared.tab_manager);
    for ws in tm.iter_mut() {
        for panel in ws.panels.values_mut() {
            let new_ports = panel_ports
                .get(&panel.id)
                .map(|ports| {
                    let mut sorted: Vec<u16> = ports.clone();
                    sorted.sort_unstable();
                    sorted.dedup();
                    sorted
                })
                .unwrap_or_default();

            if panel.listening_ports != new_ports {
                panel.listening_ports = new_ports;
                changed = true;
            }
        }
    }

    Ok(changed)
}

fn clear_all_ports(shared: &Arc<SharedState>) -> bool {
    let mut changed = false;
    let mut tm = lock_or_recover(&shared.tab_manager);
    for ws in tm.iter_mut() {
        for panel in ws.panels.values_mut() {
            if !panel.listening_ports.is_empty() {
                panel.listening_ports.clear();
                changed = true;
            }
        }
    }
    changed
}

/// Parse /proc/net/tcp and /proc/net/tcp6, returning (inode, port) for LISTEN sockets.
fn read_listening_inodes() -> Result<Vec<(u64, u16)>, std::io::Error> {
    let mut results = Vec::new();

    for path in &["/proc/net/tcp", "/proc/net/tcp6"] {
        if let Ok(contents) = fs::read_to_string(path) {
            for line in contents.lines().skip(1) {
                if let Some((inode, port)) = parse_tcp_line(line) {
                    results.push((inode, port));
                }
            }
        }
    }

    Ok(results)
}

/// Parse a single line from /proc/net/tcp.
/// Format: sl local_address rem_address st tx_queue:rx_queue tr:tm->when retrnsmt uid timeout inode
/// We want state=0A (LISTEN), local port, and inode.
fn parse_tcp_line(line: &str) -> Option<(u64, u16)> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 10 {
        return None;
    }

    // State is field 3 (0-indexed), must be "0A" (LISTEN)
    if fields[3] != "0A" {
        return None;
    }

    // Local address is field 1: "XXXXXXXX:PORT" (hex)
    let local_addr = fields[1];
    let port_hex = local_addr.rsplit(':').next()?;
    let port = u16::from_str_radix(port_hex, 16).ok()?;

    // Skip well-known system ports that are unlikely to be user services.
    if port == 0 {
        return None;
    }

    // Inode is field 9
    let inode = fields[9].parse::<u64>().ok()?;
    if inode == 0 {
        return None;
    }

    Some((inode, port))
}

/// Scan /proc/*/fd to find which PIDs own which socket inodes.
fn map_inodes_to_pids(
    listening_inodes: &[(u64, u16)],
) -> Result<HashMap<u64, Vec<u32>>, std::io::Error> {
    let target_inodes: HashSet<u64> = listening_inodes.iter().map(|(inode, _)| *inode).collect();
    let mut inode_to_pids: HashMap<u64, Vec<u32>> = HashMap::new();

    let proc_dir = fs::read_dir("/proc")?;
    for entry in proc_dir.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let pid: u32 = match name_str.parse() {
            Ok(pid) => pid,
            Err(_) => continue,
        };

        let fd_path = format!("/proc/{pid}/fd");
        let fd_dir = match fs::read_dir(&fd_path) {
            Ok(dir) => dir,
            Err(_) => continue,
        };

        for fd_entry in fd_dir.flatten() {
            let link = match fs::read_link(fd_entry.path()) {
                Ok(l) => l,
                Err(_) => continue,
            };
            let link_str = link.to_string_lossy();
            // Socket links look like "socket:[12345]"
            if let Some(inode_str) = link_str
                .strip_prefix("socket:[")
                .and_then(|s| s.strip_suffix(']'))
            {
                if let Ok(inode) = inode_str.parse::<u64>() {
                    if target_inodes.contains(&inode) {
                        inode_to_pids.entry(inode).or_default().push(pid);
                    }
                }
            }
        }
    }

    Ok(inode_to_pids)
}

/// Resolve PID cwds from /proc/*/cwd symlinks.
fn resolve_pid_cwds(inode_to_pids: &HashMap<u64, Vec<u32>>) -> HashMap<u32, String> {
    let all_pids: HashSet<u32> = inode_to_pids.values().flatten().copied().collect();
    let mut result = HashMap::new();

    for pid in all_pids {
        let cwd_link = format!("/proc/{pid}/cwd");
        if let Ok(cwd) = fs::read_link(Path::new(&cwd_link)) {
            result.insert(pid, cwd.to_string_lossy().into_owned());
        }
    }

    result
}
