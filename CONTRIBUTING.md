# Contributing to Damascene

Damascene is young and issues are the most valuable contribution:
concrete pushback — "this invariant fails at X, here's why" — beats
incremental polish. This file covers the mechanics.

## Filing issues

Use the issue templates; the short version is: name the backend and
platform, and for rendering or layout problems attach a
[bundle dump](README.md#per-app-artifact-dumps) of the affected scene
— the dump reproduces the tree without your app. Issues in this repo
are also a working queue: some are filed by coding agents, and those
open by naming the model and workspace they came from.

## Building and testing

`cargo test --workspace` is the baseline. CI additionally gates, and a
pull request must pass:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`,
  with the **newest stable** toolchain (CI tracks `stable`, so a new
  clippy lint can fail a PR that was clean locally on an older one)
- doctests (`cargo test --workspace --doc`)
- rustdoc with warnings denied
  (`RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`)
- target builds: `damascene-web` on `wasm32-unknown-unknown`,
  `damascene-android` on `aarch64-linux-android`, `damascene-ios`
  cross-checked for `aarch64-apple-ios` (all from Linux)
- MSRV: the workspace builds on Rust **1.89** (`rust-version` in
  `Cargo.toml`; it moves only when a dependency requires it)

## Code expectations

- **The symmetry invariant.** Stock widgets under
  `crates/damascene-core/src/widgets/` compose only public surface —
  no `pub(crate)` reach-through, no `#[doc(hidden)]`, no library-side
  matching on decorative `Kind` variants. If your widget needs a
  private hook, the public surface is what needs to change.
  `crates/damascene-core/src/widget_kit.md` is the contract.
- **Controlled widgets.** Text inputs and friends follow the
  controlled pattern: the app owns the state and calls the public
  `apply_event` helpers. New stateful widgets should too.
- **API shape.** Prefer HTML/DOM-shaped surfaces over invented
  abstractions; when a CSS or DOM behavior exists for the problem,
  mirror it and cite it.
- Match the surrounding file's comment density and idiom; put tests
  beside the change.

## Provenance

Nothing lands that was copied from another codebase without a
license-compatible source recorded in [CREDITS.md](CREDITS.md).
Most of Damascene is model-written, and the project states its
borrowings affirmatively rather than claiming clean-room purity —
contributions are held to the same rule. AI-assisted PRs are welcome
on the same terms as any other: say so in the description, and review
and test what you submit as if you had typed it.

## Releases

`docs/RELEASING.md` documents the release procedure. Versions move in
lockstep across all published crates; the GitHub release notes are the
changelog.
