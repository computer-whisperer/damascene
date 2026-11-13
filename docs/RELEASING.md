# Releasing Damascene

The release procedure, as practiced since 0.3.x. There is no CHANGELOG
file — the GitHub release notes are the changelog.

1. **Prepare commit.** One commit titled `Prepare X.Y.Z release` that:
   - bumps `version` in every workspace crate's `Cargo.toml` (published
     and unpublished alike — internal path deps carry `version =`
     requirements, so they move together) and refreshes `Cargo.lock`;
   - updates anything user-visible that names the version: the two
     dependency lines in the top-level README's quick start, and the
     version placeholder in `.github/ISSUE_TEMPLATE/bug_report.yml`;
   - regenerates the README images that changed (`render_hero`,
     `render_liquid_glass_lab`, and `render_showcase_sections`, whose
     default-theme output is the `assets/showcase_*.png` set);
   - lands any README/doc edits that should ship in the release.
2. **Verify.** `cargo test --workspace` green; `cargo clippy
   --workspace --all-targets` clean. Then package the publishable set
   in one invocation — `cargo package -p damascene-fonts -p
   damascene-core …`, every crate `tools/publish_crates.sh` lists
   (cargo 1.90 or newer). That builds each crate from its tarball
   against the others' packaged versions, so it covers what a
   per-crate `cargo publish --dry-run` cannot: a crate that uses an
   API its in-tree dependency gained since the last release. Read its
   warnings; 0.9.0's run reported two yanked versions in `Cargo.lock`,
   fixed with `cargo update -p <crate>` before the tag.
3. **Publish to crates.io** with `tools/publish_crates.sh`, from the
   pushed release commit once CI is green on it. The script goes in
   dependency order (`damascene-fonts` → `damascene-core` →
   transformers (`-html`, `-markdown`) → backends (`-wgpu`,
   `-vulkano`, `-ash`) → `damascene-winit` (host input mappers) →
   hosts (`-winit-wgpu`, `-web`) → mobile shells (`-android`,
   `-ios`)) and skips a crate already published at this version, so
   it can be re-run after a failure. The `damascene-fonts-*` asset
   crates are published by hand, before it, only when a font changes.
4. **Tag** the commit the release ships from `vX.Y.Z` and push the
   tag. That is the prepare commit when nothing landed after it; when
   release-day work (README, docs, assets) followed the prepare commit,
   tag the release-day HEAD instead (0.8.0 and 0.9.0 did).
5. **GitHub release** on the tag, titled `damascene X.Y.Z`, with
   hand-written notes summarizing what changed since the previous
   release (issue references welcome). Publishing the release is what
   makes the version official; `pages.yml` deploys the web showcase
   for released versions.
