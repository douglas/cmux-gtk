# Contributing to cmux-gtk

## Getting Started

```bash
git clone --recursive https://github.com/douglas/cmux-gtk.git
cd cmux-gtk
cargo build          # Stub mode (no ghostty)
cargo test --workspace
```

To build with the terminal (requires Zig):
```bash
cargo build --features cmux/link-ghostty
```

## Code Style

- Run `cargo clippy --workspace -- -D warnings` before submitting
- Run `cargo fmt --all` to format code
- Keep changes minimal — one logical change per commit

## Security

Security is a top priority. Before submitting changes:

- Run `/rs-sec-review` (Claude Code skill) or review the checklist in `docs/security.md`
- All `unsafe` blocks must have `// SAFETY:` comments
- All file writes must use `OpenOptions` with `.mode(0o600)` — never `std::fs::write` for config/session files
- All socket inputs must be truncated to documented limits
- No `sh -c` with interpolated user input — use `Command::new().arg()` directly

## Testing

```bash
cargo test --workspace
```

Add tests for any new socket auth, file I/O, or security-critical code paths.

## Reporting Security Issues

See [docs/security.md](docs/security.md#reporting-security-issues) for responsible disclosure.
