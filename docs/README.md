# Damascene Docs

These docs are for agents and maintainers working on Damascene itself. Public
crate-facing guidance should live in crate READMEs and rustdoc because
that survives crates.io packaging.

- `SHADER_VISION.md` — rendering-layer architecture and backend boundary.
- `LIBRARY_VISION.md` — application/widget-layer architecture and public API
  stability questions.
- `COLOR_MANAGEMENT.md` — HDR and color-management architecture: working
  color space, surface negotiation, white-level anchoring, image remaster.
- `SCENE3D_PLAN.md` — the Scene3D (`chart3d`) design note and milestone log.
- `MATH_VISION.md` — native math rendering architecture, current first slice,
  and next work packages.
- `HTML_VISION.md` — HTML → `El` transformer architecture and fidelity
  boundaries.
- `MOBILE_VISION.md` — touch input and small-viewport architecture.
- `TOUCH_PLAN.md` — the multi-touch gesture grammar (contact registry,
  pinch, plot touch semantics) and the touch-affordance arc rulings.
- `POLISH_CALIBRATION.md` — visual-quality calibration program and gates before
  serious app ports.
- `RELEASING.md` — the release procedure.
