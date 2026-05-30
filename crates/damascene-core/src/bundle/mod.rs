//! Artifact pipeline — the agent loop's feedback channel.
//!
//! [`artifact`] is the orchestrator (`render_bundle` entry point that
//! runs layout, draw-ops, inspect, manifest, lint, and svg in one
//! call). The four siblings — [`inspect`] (tree dump), [`lint`]
//! (provenance-tracked findings), [`manifest`] (shader manifest +
//! draw-op text), [`svg`] (approximate SVG fallback) — are individual
//! emitters; `artifact` composes them.

pub mod artifact;
pub mod inspect;
pub mod lint;
pub mod manifest;
pub mod svg;
