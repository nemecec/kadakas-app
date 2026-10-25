# Contributing

Read `docs/dev-guide.md` first. It tells how to build, test and add features.

- Add tests with each change. Run `cargo fmt`, `cargo clippy` and `cargo test --workspace`.
- Add a line under `[Unreleased]` in `CHANGELOG.md` for every change that a user can notice (Added, Changed, Fixed, Security). Do it in the same commit or pull request. Describe the final result, not the steps.
- A security fix links the advisory and credits the reporter. Report a vulnerability in private (`SECURITY.md`).
- Add attribution rows for every new asset or dependency (`docs/attribution.md`).
- AI agents follow the same rules. They add no co-author lines. Humans may.
