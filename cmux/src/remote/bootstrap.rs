//! Remote daemon bootstrap — probe, build, upload, and verify cmuxd-remote.
//!
//! Flow:
//! 1. SSH probe: detect remote OS/arch via `uname`
//! 2. Check if daemon binary exists at versioned path on remote
//! 3. If missing: find/download/build locally, then upload via scp
//!    Priority order for obtaining the binary:
//!    a. CMUX_REMOTE_DAEMON_BINARY env var
//!    b. Pre-built binary in XDG cache
//!    c. Download from GitHub Releases
//!    d. Build from Go source (developer fallback)
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

    // Priority 3: Download from GitHub Releases
    let version = daemon_version();
    match download_from_github_releases(&version, platform) {
        Ok(path) => return Ok(path),
        Err(e) => tracing::warn!("GitHub Releases download failed (non-fatal): {e}"),
    }

    // Priority 4: Build from Go source (developer fallback)
    let go_source = find_go_source_dir()?;
    build_daemon_locally(platform, &go_source)
}

/// Download the daemon binary for the target platform from GitHub Releases.
///
/// On success, writes to the XDG cache directory (same path that Priority 2
/// checks), so subsequent runs avoid the download entirely.
fn download_from_github_releases(
    version: &str,
    platform: &RemotePlatform,
) -> Result<String, String> {
    let asset_name = format!("cmuxd-remote-{}-{}", platform.go_os, platform.go_arch);
    let url = format!(
        "https://github.com/manaflow-ai/cmux-gtk/releases/download/v{version}/{asset_name}"
    );

    tracing::info!(%url, "Downloading cmuxd-remote from GitHub Releases");

    let response = ureq::get(&url)
        .call()
        .map_err(|e| format!("HTTP GET {url}: {e}"))?;

    let body = response
        .into_body()
        .read_to_vec()
        .map_err(|e| format!("Failed to read response body: {e}"))?;

    if body.is_empty() {
        return Err("Downloaded binary is empty".to_string());
    }

    tracing::info!(bytes = body.len(), "Downloaded {asset_name}");

    // Verify SHA-256 — mandatory except when the manifest itself is unreachable
    // (offline / air-gapped environments).  Any other failure (hash mismatch,
    // asset not listed) is a hard error to prevent MITM binary substitution.
    match verify_sha256(version, &asset_name, &body) {
        Ok(()) => {}
        Err(e)
            if e.starts_with("Failed to fetch checksums")
                || e.starts_with("Failed to read manifest") =>
        {
            tracing::error!(
                "SHA-256 manifest unreachable, proceeding without verification: {e}"
            );
        }
        Err(e) => {
            return Err(format!("SHA-256 verification failed: {e}"));
        }
    }

    // Write to cache so subsequent runs hit Priority 2 instead.
    let cache_dir = dirs::cache_dir()
        .ok_or("Cannot determine XDG cache directory")?
        .join("cmux")
        .join("remote-daemons")
        .join(version)
        .join(format!("{}-{}", platform.go_os, platform.go_arch));

    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Cannot create cache dir: {e}"))?;

    let final_path = cache_dir.join("cmuxd-remote");
    let tmp_path = cache_dir.join("cmuxd-remote.tmp");

    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o755)
            .open(&tmp_path)
            .map_err(|e| format!("Cannot write temp file: {e}"))?;
        f.write_all(&body)
            .map_err(|e| format!("Cannot write binary data: {e}"))?;
    }

    std::fs::rename(&tmp_path, &final_path)
        .map_err(|e| format!("Cannot rename temp to final: {e}"))?;

    tracing::info!(path = %final_path.display(), "Cached cmuxd-remote binary");
    Ok(final_path.to_string_lossy().to_string())
}

/// Verify the binary against the SHA-256 manifest published with the release.
fn verify_sha256(version: &str, asset_name: &str, data: &[u8]) -> Result<(), String> {
    let manifest_url = format!(
        "https://github.com/manaflow-ai/cmux-gtk/releases/download/v{version}/checksums-sha256.txt"
    );

    let response = ureq::get(&manifest_url)
        .call()
        .map_err(|e| format!("Failed to fetch checksums manifest: {e}"))?;

    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Failed to read manifest: {e}"))?;

    for line in body.lines() {
        // Format: "<sha256>  <filename>"
        let mut parts = line.splitn(2, |c: char| c.is_whitespace());
        let hash = parts.next().unwrap_or("").trim();
        let name = parts.next().unwrap_or("").trim();
        if name == asset_name {
            use sha2::Digest;
            let actual = sha2::Sha256::digest(data)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            if actual != hash {
                return Err(format!(
                    "SHA-256 mismatch for {asset_name}: expected {hash}, got {actual}"
                ));
            }
            tracing::info!("SHA-256 verified for {asset_name}");
            return Ok(());
        }
    }

    Err(format!(
        "{asset_name} not found in checksums manifest for v{version}"
    ))
}

/// Find the Go source directory for cmuxd-remote.
fn find_go_source_dir() -> Result<String, String> {
    let candidates: &[std::path::PathBuf] = &[
        // 1. Vendored source in this repository (daemon/remote/ at repo root).
        //    CARGO_MANIFEST_DIR is cmux-gtk/cmux at compile time, so go up two levels.
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../daemon/remote"),
        // 2. Legacy sibling repo location (~/src/cmux/daemon/remote).
        dirs::home_dir()
            .map(|h| h.join("src/cmux/daemon/remote"))
            .unwrap_or_default(),
    ];

    for candidate in candidates {
        if candidate.join("go.mod").exists() {
            return Ok(candidate.to_string_lossy().to_string());
        }
    }

    Err(
        "Cannot find cmuxd-remote Go source. Set CMUX_REMOTE_DAEMON_BINARY, \
         ensure daemon/remote/ exists in the repository, or connect to the internet \
         so the binary can be downloaded from GitHub Releases."
            .to_string(),
    )
}

/// The daemon version string used for binary caching and remote paths.
fn daemon_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_daemon_path_format() {
        let platform = RemotePlatform {
            go_os: "linux".to_string(),
            go_arch: "amd64".to_string(),
        };
        let path = remote_daemon_path("1.2.3", &platform);
        assert_eq!(
            path,
            "~/.cmux/bin/cmuxd-remote/1.2.3/linux-amd64/cmuxd-remote"
        );
    }

    #[test]
    fn test_remote_daemon_path_darwin_arm64() {
        let platform = RemotePlatform {
            go_os: "darwin".to_string(),
            go_arch: "arm64".to_string(),
        };
        let path = remote_daemon_path("0.62.0-alpha.8", &platform);
        assert_eq!(
            path,
            "~/.cmux/bin/cmuxd-remote/0.62.0-alpha.8/darwin-arm64/cmuxd-remote"
        );
    }
}
