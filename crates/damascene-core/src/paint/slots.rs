//! Named instance-attribute routing for custom shaders (issue #99).
//!
//! A custom quad shader receives its per-draw parameters through five
//! generic `vec4` instance attributes (`@location(2..=4, 6, 7)` — see
//! [`QuadInstance`](super::QuadInstance)). Historically the Rust side
//! addressed them only by the positional aliases `vec_a`..`vec_e`, and
//! agreement between the app's packing and the WGSL's unpacking was
//! enforced by nothing but mirrored comments — reorder either side and
//! it compiles fine and renders garbage.
//!
//! The fix: the *names the WGSL declares* become the contract. At
//! registration the backend introspects the module
//! (`introspect_wgsl`, behind the `shader-introspect` feature) and
//! hands the resulting [`ShaderSlotMap`] to
//! the runtime; [`ShaderBinding`](super::shader::ShaderBinding)
//! uniforms are then routed by WGSL field name, and a key that matches
//! no declared attribute warns (once) instead of silently landing in a
//! zeroed slot:
//!
//! ```wgsl
//! struct InstanceInput {
//!     @location(1) rect:          vec4<f32>,
//!     @location(2) xyz_to_rgb_c0: vec4<f32>,
//!     @location(3) xyz_to_rgb_c1: vec4<f32>,
//!     @location(4) xyz_to_rgb_c2: vec4<f32>,
//!     @location(6) view_bounds:   vec4<f32>,
//! }
//! ```
//!
//! ```ignore
//! ShaderBinding::custom("chroma_field")
//!     .mat3(["xyz_to_rgb_c0", "xyz_to_rgb_c1", "xyz_to_rgb_c2"], xyz_to_rgb)
//!     .vec4("view_bounds", bounds)
//! ```
//!
//! Renaming or reordering the WGSL fields now either keeps working
//! (the map follows the names, locations are irrelevant to the app) or
//! warns at the first draw. The positional aliases `vec_a`..`vec_e`
//! remain valid forever, so existing shaders are unaffected.

use std::collections::BTreeMap;

/// Locations of the five free `vec4` slots in
/// [`QuadInstance`](super::QuadInstance) order
/// (`slot_a`..`slot_e`). Locations 0 (`corner_uv`), 1 (`rect`), and
/// 5 (`inner_rect`) are library-owned.
pub const FREE_SLOT_LOCATIONS: [u32; 5] = [2, 3, 4, 6, 7];

/// Positional alias for each free slot, in the same order as
/// [`FREE_SLOT_LOCATIONS`]. These names always route, whatever the
/// shader declares.
pub const SLOT_ALIASES: [&str; 5] = ["vec_a", "vec_b", "vec_c", "vec_d", "vec_e"];

/// `QuadInstance` slot index (0 = `slot_a` .. 4 = `slot_e`) for a
/// vertex-shader location, if it is one of the free slots.
pub fn slot_index_for_location(location: u32) -> Option<usize> {
    FREE_SLOT_LOCATIONS.iter().position(|&l| l == location)
}

/// Instance-attribute names a custom shader declared, mapped to their
/// locations. Built by `introspect_wgsl` at registration (backends
/// do this automatically) and consumed by the runtime's quad fold to
/// route named uniforms.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShaderSlotMap {
    /// WGSL field name → `@location`. Free slots only.
    names: BTreeMap<String, u32>,
}

impl ShaderSlotMap {
    /// Build from explicit `(name, location)` pairs. Pairs whose
    /// location is not a free slot are ignored. Primarily for tests
    /// and hosts that compile shaders out-of-band.
    pub fn from_pairs<I, S>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (S, u32)>,
        S: Into<String>,
    {
        let names = pairs
            .into_iter()
            .filter(|(_, loc)| slot_index_for_location(*loc).is_some())
            .map(|(n, loc)| (n.into(), loc))
            .collect();
        Self { names }
    }

    /// The location a declared name maps to, if the shader declared it.
    pub fn location_of(&self, name: &str) -> Option<u32> {
        self.names.get(name).copied()
    }

    /// All declared `(name, location)` pairs, in name order.
    pub fn declared(&self) -> impl Iterator<Item = (&str, u32)> {
        self.names.iter().map(|(n, l)| (n.as_str(), *l))
    }

    /// True when the shader declared no named free slots (e.g. it only
    /// reads `rect`, or introspection was skipped).
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

/// Errors [`introspect_wgsl`] can report. All of them mean the
/// registered shader cannot make the named-slot contract — backends
/// log and fall back to positional `vec_a`..`vec_e` routing.
#[cfg(feature = "shader-introspect")]
#[derive(Debug)]
pub enum IntrospectError {
    /// The WGSL failed to parse. The backend's own compile will fail
    /// loudly right after with a better message; this exists so the
    /// introspector never panics first.
    Parse(String),
    /// No `@vertex` entry point in the module.
    NoVertexEntry,
    /// A free-slot instance attribute is not `vec4<f32>` — the
    /// pipeline's vertex layout feeds every free slot as `Float32x4`,
    /// so any other type fails pipeline creation with a cryptic
    /// backend error; this catches it with the field name attached.
    NotVec4 { name: String, location: u32 },
}

#[cfg(feature = "shader-introspect")]
impl std::fmt::Display for IntrospectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IntrospectError::Parse(e) => write!(f, "WGSL parse error: {e}"),
            IntrospectError::NoVertexEntry => write!(f, "no @vertex entry point"),
            IntrospectError::NotVec4 { name, location } => write!(
                f,
                "instance attribute `{name}` (@location({location})) must be vec4<f32> — \
                 the quad instance layout feeds every free slot as Float32x4",
            ),
        }
    }
}

/// Parse a custom shader's WGSL and extract the free-slot instance
/// attributes its vertex entry point declares — names included. The
/// backends run this in `register_shader_with` and feed the result to
/// `RunnerCore::register_shader_slots`; apps don't call it directly.
///
/// Library-owned locations (0, 1, 5) and builtins are skipped. The
/// positional aliases keep working whether or not a name map exists.
#[cfg(feature = "shader-introspect")]
pub fn introspect_wgsl(wgsl: &str) -> Result<ShaderSlotMap, IntrospectError> {
    let module = naga::front::wgsl::parse_str(wgsl)
        .map_err(|e| IntrospectError::Parse(e.message().to_string()))?;
    let vertex = module
        .entry_points
        .iter()
        .find(|ep| ep.stage == naga::ShaderStage::Vertex)
        .ok_or(IntrospectError::NoVertexEntry)?;

    let mut names = BTreeMap::new();
    let mut visit = |name: Option<&String>,
                     ty: naga::Handle<naga::Type>,
                     binding: Option<&naga::Binding>|
     -> Result<(), IntrospectError> {
        let Some(naga::Binding::Location { location, .. }) = binding else {
            return Ok(()); // builtin or unbound
        };
        if slot_index_for_location(*location).is_none() {
            return Ok(()); // library-owned location
        }
        let name = name.cloned().unwrap_or_default();
        let is_vec4 = matches!(
            module.types[ty].inner,
            naga::TypeInner::Vector {
                size: naga::VectorSize::Quad,
                scalar: naga::Scalar {
                    kind: naga::ScalarKind::Float,
                    width: 4,
                },
            }
        );
        if !is_vec4 {
            return Err(IntrospectError::NotVec4 {
                name,
                location: *location,
            });
        }
        if !name.is_empty() {
            names.insert(name, *location);
        }
        Ok(())
    };

    for arg in &vertex.function.arguments {
        match &module.types[arg.ty].inner {
            naga::TypeInner::Struct { members, .. } => {
                for member in members {
                    visit(member.name.as_ref(), member.ty, member.binding.as_ref())?;
                }
            }
            _ => visit(arg.name.as_ref(), arg.ty, arg.binding.as_ref())?,
        }
    }
    Ok(ShaderSlotMap { names })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_indices_match_quad_instance_order() {
        assert_eq!(slot_index_for_location(2), Some(0)); // slot_a
        assert_eq!(slot_index_for_location(4), Some(2)); // slot_c
        assert_eq!(slot_index_for_location(6), Some(3)); // slot_d
        assert_eq!(slot_index_for_location(7), Some(4)); // slot_e
        assert_eq!(slot_index_for_location(1), None); // rect — library-owned
        assert_eq!(slot_index_for_location(5), None); // inner_rect
    }

    #[test]
    fn from_pairs_drops_library_owned_locations() {
        let map = ShaderSlotMap::from_pairs([("fill", 2u32), ("rect", 1u32)]);
        assert_eq!(map.location_of("fill"), Some(2));
        assert_eq!(map.location_of("rect"), None);
    }

    #[cfg(feature = "shader-introspect")]
    #[test]
    fn introspect_extracts_named_free_slots() {
        let wgsl = r#"
            struct VertexInput { @location(0) corner_uv: vec2<f32> };
            struct InstanceInput {
                @location(1) rect:          vec4<f32>,
                @location(2) xyz_to_rgb_c0: vec4<f32>,
                @location(3) xyz_to_rgb_c1: vec4<f32>,
                @location(4) xyz_to_rgb_c2: vec4<f32>,
                @location(6) view_bounds:   vec4<f32>,
            };
            struct VertexOutput { @builtin(position) pos: vec4<f32> };
            @vertex
            fn vs_main(v: VertexInput, i: InstanceInput) -> VertexOutput {
                var out: VertexOutput;
                out.pos = vec4<f32>(v.corner_uv * i.rect.zw + i.xyz_to_rgb_c0.xy
                    + i.xyz_to_rgb_c1.xy + i.xyz_to_rgb_c2.xy + i.view_bounds.xy, 0.0, 1.0);
                return out;
            }
            @fragment
            fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }
        "#;
        let map = introspect_wgsl(wgsl).expect("introspect");
        assert_eq!(map.location_of("xyz_to_rgb_c0"), Some(2));
        assert_eq!(map.location_of("xyz_to_rgb_c2"), Some(4));
        assert_eq!(map.location_of("view_bounds"), Some(6));
        assert_eq!(map.location_of("rect"), None, "library-owned");
        assert_eq!(map.declared().count(), 4);
    }

    #[cfg(feature = "shader-introspect")]
    #[test]
    fn introspect_rejects_non_vec4_free_slot() {
        let wgsl = r#"
            struct InstanceInput {
                @location(2) radius: f32,
            };
            @vertex
            fn vs_main(i: InstanceInput) -> @builtin(position) vec4<f32> {
                return vec4<f32>(i.radius, 0.0, 0.0, 1.0);
            }
            @fragment
            fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }
        "#;
        match introspect_wgsl(wgsl) {
            Err(IntrospectError::NotVec4 { name, location }) => {
                assert_eq!(name, "radius");
                assert_eq!(location, 2);
            }
            other => panic!("expected NotVec4, got {other:?}"),
        }
    }
}
