//! Remote daemon bootstrap — probe, build, upload, and verify cmuxd-remote.
//!
//! Flow:
//! 1. SSH probe: detect remote OS/arch via `uname`
//! 2. Check if daemon binary exists at versioned path on remote
//! 3. If missing: build locally from Go source (or use pre-built), upload via scp
//! 4. Verify: start daemon, run hello handshake

use std::process::Command;

/// Platform info detected from a remote host.
#[derive(Debug, Clone)]
pub struct RemotePlatform {
    pub go_os: String,
    pub go_arch: String,
}

/// Probe the remote host to detect OS and architecture.
pub fn probe_platform(ssh_args: &[String]) -> Result<RemotePlatform, String> {
    let output = Command::new("ssh")
        .args(["-T", "-S", "none", "-o", "ConnectTimeout=6"])
        .args(ssh_args)
        .arg("uname -s && uname -m")
        .output()
        .map_err(|e| format!("Failed to run SSH probe: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("SSH probe failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    if lines.len() < 2 {
        return Err(format!("Unexpected probe output: {}", stdout.trim()));
    }

    let go_os = match lines[0].trim() {
        "Linux" => "linux",
        "Darwin" => "darwin",
        "FreeBSD" => "freebsd",
        other => return Err(format!("Unsupported remote OS: {}", other)),
    };

    let go_arch = match lines[1].trim() {
        "x86_64" | "amd64" => "amd64",
        "aarch64" | "arm64" => "arm64",
        "armv7l" => "arm",
        other => return Err(format!("Unsupported remote architecture: {}", other)),
    };

    Ok(RemotePlatform {
        go_os: go_os.to_string(),
        go_arch: go_arch.to_string(),
    })
}

/// Versioned path where the daemon binary is installed on the remote host.
pub fn remote_daemon_path(version: &str, platform: &RemotePlatform) -> String {
    format!(
        "~/.cmux/bin/cmuxd-remote/{}/{}-{}/cmuxd-remote",
        version, platform.go_os, platform.go_arch,
    )
}

/// Check if the daemon binary exists on the remote host.
pub fn check_remote_binary(ssh_args: &[String], remote_path: &str) -> bool {
    let output = Command::new("ssh")
        .args(["-T", "-S", "none", "-o", "ConnectTimeout=6"])
        .args(ssh_args)
        .arg(format!(
            "test -x {} && echo OK",
            shell_escape::escape(remote_path.into())
        ))
        .output();

    match output {
        Ok(out) => out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "OK",
        Err(_) => false,
    }
}

/// Build the daemon binary locally from Go source.
///
/// Requires Go toolchain installed. Builds for the target platform.
/// Returns the path to the built binary.
pub fn build_daemon_locally(
    platform: &RemotePlatform,
    go_source_dir: &str,
) -> Result<String, String> {
    let output_path = format!("/tmp/cmuxd-remote-{}-{}", platform.go_os, platform.go_arch);

    tracing::info!(
        go_os = %platform.go_os,
        go_arch = %platform.go_arch,
        source = go_source_dir,
        output = %output_path,
        "Building cmuxd-remote from Go source"
    );

    let status = Command::new("go")
        .arg("build")
        .arg("-o")
        .arg(&output_path)
        .arg("./cmd/cmuxd-remote")
        .env("GOOS", &platform.go_os)
        .env("GOARCH", &platform.go_arch)
        .env("CGO_ENABLED", "0")
        .current_dir(go_source_dir)
        .status()
        .map_err(|e| format!("Failed to run go build: {}", e))?;

    if !status.success() {
        return Err("go build failed".to_string());
    }

    Ok(output_path)
}

/// Upload a local binary to the remote host via scp.
pub fn upload_daemon(
    ssh_args: &[String],
    local_path: &str,
    remote_path: &str,
) -> Result<(), String> {
    // Create remote directory
    let dir = remote_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("~");
    let mkdir_status = Command::new("ssh")
        .args(["-T", "-S", "none", "-o", "ConnectTimeout=6"])
        .args(ssh_args)
        .arg(format!("mkdir -p {}", shell_escape::escape(dir.into())))
        .status()
        .map_err(|e| format!("Failed to create remote directory: {}", e))?;

    if !mkdir_status.success() {
        return Err("Failed to create remote directory".to_string());
    }

    // Build scp destination from ssh_args (extract destination)
    let destination = ssh_args.last().ok_or("No SSH destination in args")?;

    // Extract port if present
    let mut scp_args: Vec<String> = Vec::new();
    let mut i = 0;
    while i < ssh_args.len() - 1 {
        if ssh_args[i] == "-p" && i + 1 < ssh_args.len() - 1 {
            scp_args.push("-P".to_string()); // scp uses -P not -p
            scp_args.push(ssh_args[i + 1].clone());
            i += 2;
        } else if ssh_args[i] == "-i" && i + 1 < ssh_args.len() - 1 {
            scp_args.push("-i".to_string());
            scp_args.push(ssh_args[i + 1].clone());
            i += 2;
        } else {
            i += 1;
        }
    }

    let scp_dest = format!("{}:{}", destination, remote_path);

    tracing::info!(
        local = local_path,
        remote = %scp_dest,
        "Uploading daemon binary"
    );

    let status = Command::new("scp")
        .args(&scp_args)
        .arg(local_path)
        .arg(&scp_dest)
        .status()
        .map_err(|e| format!("scp failed: {}", e))?;

    if !status.success() {
        return Err("scp upload failed".to_string());
    }

    // Make executable
    let chmod_status = Command::new("ssh")
        .args(["-T", "-S", "none", "-o", "ConnectTimeout=6"])
        .args(ssh_args)
        .arg(format!(
            "chmod +x {}",
            shell_escape::escape(remote_path.into())
        ))
        .status()
        .map_err(|e| format!("chmod failed: {}", e))?;

    if !chmod_status.success() {
        return Err("Failed to chmod daemon binary".to_string());
    }

    Ok(())
}

/// Full bootstrap flow: probe → check → build → upload → verify path.
///
/// Returns the remote daemon path on success.
pub fn bootstrap_daemon(ssh_args: &[String]) -> Result<String, String> {
    let version = daemon_version();

    // Step 1: Probe platform
    tracing::info!("Probing remote platform...");
    let platform = probe_platform(ssh_args)?;
    tracing::info!(os = %platform.go_os, arch = %platform.go_arch, "Remote platform detected");

    // Step 2: Check if binary exists
    let remote_path = remote_daemon_path(&version, &platform);
    if check_remote_binary(ssh_args, &remote_path) {
        tracing::info!("Remote daemon binary already exists at {}", remote_path);
        return Ok(remote_path);
    }

    // Step 3: Find or build local binary
    let local_binary = find_or_build_local_binary(&platform)?;

    // Step 4: Upload
    upload_daemon(ssh_args, &local_binary, &remote_path)?;
    tracing::info!("Daemon uploaded to {}", remote_path);

    Ok(remote_path)
}

/// Find a pre-built binary or build from Go source.
fn find_or_build_local_binary(platform: &RemotePlatform) -> Result<String, String> {
    // Priority 1: CMUX_REMOTE_DAEMON_BINARY env var
    if let Ok(path) = std::env::var("CMUX_REMOTE_DAEMON_BINARY") {
        if std::path::Path::new(&path).exists() {
            tracing::info!("Using explicit daemon binary: {}", path);
            return Ok(path);
        }
    }

    // Priority 2: Pre-built binary in cache
    let version = daemon_version();
    if let Some(cache_dir) = dirs::cache_dir() {
        let cached = cache_dir
            .join("cmux")
            .join("remote-daemons")
            .join(&version)
            .join(format!("{}-{}", platform.go_os, platform.go_arch))
            .join("cmuxd-remote");
        if cached.exists() {
            tracing::info!("Using cached daemon binary: {}", cached.display());
            return Ok(cached.to_string_lossy().to_string());
        }
    }

    // Priority 3: Build from Go source
    let go_source = find_go_source_dir()?;
    build_daemon_locally(platform, &go_source)
}

/// Find the Go source directory for cmuxd-remote.
fn find_go_source_dir() -> Result<String, String> {
    // Check relative to the cmux-gtk project
    let candidates = [
        // Sibling directory (~/src/cmux/daemon/remote)
        dirs::home_dir()
            .map(|h| h.join("src/cmux/daemon/remote"))
            .unwrap_or_default(),
    ];

    for candidate in &candidates {
        let go_mod = candidate.join("go.mod");
        if go_mod.exists() {
            return Ok(candidate.to_string_lossy().to_string());
        }
    }

    Err(
        "Cannot find cmuxd-remote Go source. Set CMUX_REMOTE_DAEMON_BINARY or \
         ensure ~/src/cmux/daemon/remote/ exists with go.mod"
            .to_string(),
    )
}

/// The daemon version string used for binary caching and remote paths.
fn daemon_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
