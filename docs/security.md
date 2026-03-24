# Security

cmux-gtk takes security seriously. This document describes the security architecture, hardening measures, and audit history.

## Threat Model

cmux-gtk is a desktop terminal multiplexer with an embedded browser. The primary attack surfaces are:

1. **Unix socket API** — Local IPC with 210+ commands including terminal keystroke injection and browser automation
2. **Embedded WebKit browser** — Full web engine with JavaScript execution
3. **Ghostty FFI** — C/Zig foreign function interface with raw pointer handling
4. **Shell integration** — Auto-injected scripts that report CWD, git branch, and other metadata
5. **Remote SSH workspaces** — Daemon bootstrapping and proxy tunneling to remote hosts
6. **Session persistence** — Terminal scrollback and browser state saved to disk

## Socket Authentication

The socket server uses kernel-level `SO_PEERCRED` authentication on every connection, verifying the connecting process's PID, UID, and GID. Five control modes are available:

| Mode | Authentication | Use case |
|------|---------------|----------|
| `LocalUser` (default) | Same UID via SO_PEERCRED | Normal desktop use |
| `CmuxOnly` | Same UID + PID descendant check (walks /proc) | Locked-down environments |
| `Automation` | Same UID | CI/scripting |
| `AllowAll` | None (logs warning at startup) | Development only |
| `Off` | Socket disabled | Maximum isolation |

Connection limits: 64 concurrent clients, 1 MB max request size, 300s idle timeout per client.

## Cryptography

- **HMAC-SHA256** for relay authentication uses the `hmac` + `sha2` Rust crates (RustCrypto). No subprocess invocations.
- **Constant-time comparison** for HMAC verification (XOR reduction).
- **UUIDv4** tokens for relay authentication use `getrandom` (cryptographically secure).

## File Permissions

All sensitive files are written with restrictive permissions:

| File | Permissions | Content |
|------|------------|---------|
| `session.json` | 0o600, dir 0o700 | Terminal scrollback, browser URLs |
| `settings.json` | 0o600, dir 0o700 | HTTP allowlist, custom commands |
| `shortcuts.json` | 0o600, dir 0o700 | Keyboard shortcut config |
| `browser-history.json` | 0o600, dir 0o700 | Browsing history |
| `browser-profiles.json` | 0o600 | Profile configuration |
| `cmux.sock` | 0o600 (via umask 0o177) | Unix socket |
| Scrollback temp files | 0o600, dir 0o700, O_EXCL | Terminal scrollback capture |
| WebKit profile dirs | 0o700 | Cookie/cache storage |
| PID lockfile | O_EXCL create | cmuxd-remote daemon coordination |

All writes use atomic temp-file + rename with `create_new` (O_EXCL) to prevent symlink attacks. Config directories are created with explicit `set_permissions(0o700)` after `create_dir_all` to prevent world-readable directory creation.

## Input Validation

All socket inputs are truncated to prevent resource exhaustion:

- Directory paths: 4,096 chars
- Titles: 1,024 chars
- URLs: 1,024 chars
- Branch names: 256 chars
- Method names: 200 chars
- Surface input: 128 KB
- Browser eval/automation: 1 MB, 30s timeout
- Console messages: 64 KB per entry
- Browser history: 50,000 entries max
- Ports array: 256 entries max

## Browser Security

- **Permission denial**: Camera, microphone, and geolocation requests are denied by default for both browser and markdown panels.
- **JavaScript injection prevention**: Browser automation event types (`mouse`, `keyboard`, `touch`) are validated against whitelists. All user-supplied values in JavaScript templates use `serde_json::to_string()` escaping.
- **Download safety**: Filenames extracted via `Path::file_name()` (prevents path traversal). No overwrite allowed.
- **Deep link scheme whitelist**: Only known-safe schemes (`mailto`, `tel`, `ssh`, `vscode`, etc.) are forwarded to `xdg-open`. Unknown schemes are blocked. The `javascript:` scheme is not in the allowlist and is blocked.
- **HTTP interstitial XSS prevention**: The "Proceed Anyway" button uses a `data-href` attribute and event listener rather than an inline `onclick` handler, eliminating the HTML/JS nested escaping context that allows `&#39;` → `'` XSS.
- **Markdown panel navigation policy**: External `http://`/`https://` links open in xdg-open rather than navigating the embedded WebView. All other external navigations are silently blocked.
- **Cookie isolation**: Per-profile `NetworkSession` instances with separate data/cache directories.
- **User agent**: Overridden to prevent fingerprinting of the embedded engine.

## Terminal Security

- **Title/PWD sanitization**: Strings from terminal escape sequences (OSC 0/2, OSC 7) have C0/C1 control characters stripped before display in GTK widgets.
- **Environment hygiene**: `CMUX_SOCKET_PASSWORD` is removed from the environment at startup so child terminal processes cannot read it.
- **Scrollback privacy**: The `persist_scrollback` setting (default: `true`) controls whether terminal scrollback is included in session snapshots. When `false`, the `scrollback` field is omitted entirely — no sensitive terminal history is written to `session.json`. Temporary scrollback files under `~/.cache/cmux/scrollback/` are automatically deleted at the start of the next session restore.
- **Scrollback sensitivity**: When `persist_scrollback` is enabled, session files may contain terminal scrollback (up to 4,000 lines per terminal). File permissions (0o600) protect at rest.

## SSH / Remote Workspace Security

- **Disabled by default**: Remote SSH workspaces require `remote_ssh_enabled = true` in settings. This is off by default to minimize attack surface.
- **No shell wrapping**: Remote daemon paths are passed as direct SSH arguments, not embedded in `sh -c` strings.
- **Shell escaping**: All user-supplied values in SSH commands use `shell-escape` crate.
- **Host key policy**: `StrictHostKeyChecking=accept-new` (TOFU — trusts new keys, rejects changed keys).
- **SSH option validation**: Options restored from session files must be `Key=Value` format (no flag injection).
- **Relay authentication**: HMAC-SHA256 challenge-response with per-session tokens (UUIDv4 from CSPRNG).
- **Proxy tunnel**: Binds to `127.0.0.1` only, 32-connection limit, panic-guarded handler.
- **SSH stderr logging**: Captured and logged (not discarded) so host key warnings are visible.
- **Daemon bootstrap**: Remote daemon binary uploaded via SCP with verified path. Versioned at `~/.cmux/bin/cmuxd-remote/{version}/`. Download temp files use O_EXCL creation and embed the UID in the build path to prevent `/tmp` races.
- **SSRF denylist**: The proxy tunnel's `proxy.open` handler resolves hostnames via `net.LookupHost` and checks every resolved IP against a CIDR denylist before connecting. Blocked ranges: `127.0.0.0/8` (loopback), `::1/128` (IPv6 loopback), `169.254.0.0/16` (link-local / cloud metadata), `fe80::/10` (IPv6 link-local), `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16` (RFC-1918 private), `fc00::/7` (IPv6 unique-local). Set `CMUXD_PROXY_ALLOW_PRIVATE=1` on the remote host to bypass the denylist when proxying to a local dev server.
- **RPC mutex hardening**: The JSON-RPC client's `pending` and `stream_subs` bookkeeping mutexes use lock-or-recover semantics — a poisoned lock is recovered automatically, at worst dropping one pending response. The `stdin` write mutex is treated as fatal: poison means a partial JSON line was written and the protocol is corrupt, so the connection is immediately marked dead and the caller receives an error.

## FFI Safety

- **Panic guards**: All 6 `extern "C"` callback trampolines wrap their body in `std::panic::catch_unwind` to prevent undefined behavior from panics unwinding across the FFI boundary.
- **Null checks**: Every FFI pointer is checked for null before dereferencing.
- **Safety documentation**: All `unsafe` blocks have `// SAFETY:` comments documenting the invariant.
- **Thread safety**: `SendSurfacePtr` and `SendAppPtr` implement `Send` (not `Sync`). Pointers are sent via channels and only dereferenced on the GTK main thread.
- **Integer overflow**: `overflow-checks = true` in release profile prevents silent wrapping.

## Build Security

- **`cargo audit`** runs in CI via `rustsec/audit-check` on every push and PR.
- **Dependency review**: All direct dependencies are well-maintained crates with millions of downloads. Crypto uses RustCrypto (`hmac`, `sha2`) — pure Rust, no C dependencies.
- **Feature gating**: WebKit browser support is optional (`--features webkit`, default on). Building without it eliminates the WebKit attack surface entirely.

## Audit History

| Date | Round | Findings | Fixes |
|------|-------|----------|-------|
| 2026-03-22 | Initial hardening | Shell injection, XSS, path traversal, weak HMAC, file permissions | 12 fixes |
| 2026-03-24 | Hardening II | HMAC bypass, JS injection, file perms, SSH shell wrapping, input validation | 18 fixes |
| 2026-03-24 | Hardening III | Overflow checks, title sanitization, env cleanup, shortcuts perms | 7 fixes |
| 2026-03-24 | Hardening IV | Safety documentation, proxy panic guard | Documentation + 1 fix |
| 2026-03-24 | Hardening V | Interstitial XSS (data-href), `javascript:` scheme blocked, markdown panel security, O_EXCL temp files, config dir 0o700, dead code removal (Password mode) | 10 fixes |
| 2026-03-24 | Hardening VI | RPC mutex poison recovery, SSRF denylist for proxy.open | 2 fixes |
| 2026-03-24 | Hardening VII | Scrollback privacy setting, temp file cleanup on restore | 2 fixes |

## Reporting Security Issues

If you find a security vulnerability, please report it privately via GitHub's security advisory feature at https://github.com/douglas/cmux-gtk/security/advisories rather than opening a public issue.
