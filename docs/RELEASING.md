# Releasing Damascene

The release procedure, as practiced since 0.3.x. There is no CHANGELOG
file — the GitHub release notes are the changelog.

1. **Prepare commit.** One commit titled `Prepare X.Y.Z release` that:
   - bumps `version` in every workspace crate's `Cargo.toml` (published
     and unpublished alike — internal path deps carry `version =`
     requirements, so they move together) and refreshes `Cargo.lock`;
   - updates anything user-visible that names the version (e.g. the
     hero fixture's release-gate label) and regenerates the hero render
     if it changed;
   - lands any README/doc edits that should ship in the release.
2. **Verify.** `cargo test --workspace` green; `cargo clippy
   --workspace --all-targets` clean; `cargo publish --dry-run` for the
   publishable set if the crate list changed. A dry-run resolves
   in-tree dependencies against crates.io, so a crate that uses a
   feature or API its dependency gained since the last release can
   only dry-run after that dependency is published — for those,
   `cargo package --no-verify --list` checks the manifest and file
   set, and the workspace build stands in for the verify step.
3. **Publish to crates.io** in dependency order (fonts asset crates →
   `damascene-fonts` → `damascene-core` → transformers (`-html`,
   `-markdown`) → backends (`-wgpu`, `-vulkano`, `-ash`) →
   `damascene-winit` (host input mappers) → hosts (`-winit-wgpu`,
   `-web`) → mobile shells (`-android`, `-ios`)).
4. **Tag** the prepare commit `vX.Y.Z` and push the tag.
5. **GitHub release** on the tag, titled `damascene X.Y.Z`, with
   hand-written notes summarizing what changed since the previous
   release (issue references welcome). Publishing the release is what
   makes the version official; `pages.yml` deploys the web showcase
   for released versions.
