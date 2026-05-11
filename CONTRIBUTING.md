# Contributing to Terminal Studio

Terminal Studio is **alpha** software. The architecture is still evolving, so contributions are most useful in this priority order:

1. **Bug reports** — the most valuable thing right now
2. **Compatibility fixes** — escape sequences or TUI apps that don't work
3. **Small, focused code changes** — please open an issue first for anything non-trivial

---

## Reporting a Bug

Open an issue using the [bug report template](https://github.com/dpkay-io/terminal-studio/issues/new?template=bug_report.yml).

Good bug reports include:
- OS and shell (e.g. Windows 11 / PowerShell 7, Ubuntu 24.04 / bash)
- The application or command that triggered the problem
- What you expected vs. what happened
- Steps to reproduce, or a minimal escape sequence if it's a rendering issue

---

## Suggesting a Feature

Open an issue using the [feature request template](https://github.com/dpkay-io/terminal-studio/issues/new?template=feature_request.yml).

---

## Sending a Pull Request

1. Fork the repository and create a branch from `master`.
2. Keep the change focused — one fix or feature per PR.
3. Run `cargo test` and `cargo clippy` before submitting; the PR should not introduce new warnings.
4. Write a clear PR description explaining what changed and why.

For anything beyond a small bug fix, open an issue first so we can agree on the approach before you invest time in the implementation.

---

## Building and Testing

```
cargo build          # dev build (opt-level 1)
cargo build --release
cargo test           # all unit tests
cargo clippy         # lints
RUST_LOG=debug cargo run   # with debug logging (Linux/macOS)
$env:RUST_LOG="debug"; cargo run   # Windows PowerShell
```

See [README.md](README.md) for platform-specific prerequisites.

---

## Code Conventions

- **Colors**: always use constants from `src/theme.rs`. Never hardcode `Color32::from_rgb(…)` in `app.rs`.
- **Platform guards**: wrap Windows-specific code with `#[cfg(target_os = "windows")]`.
- **No new dependencies** without discussion — keep the build lightweight.
- **Tests**: new terminal emulator behavior should come with a test in `src/terminal/tests.rs`.
