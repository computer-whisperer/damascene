//! WGSL to SPIR-V compilation via `naga`.
//!
//! The dependency is pinned to the same major version as the other
//! Aetna GPU backends so custom WGSL parses consistently across
//! `wgpu`, `vulkano`, and `ash`.

use naga::back::spv;
use naga::front::wgsl;
use naga::valid;

/// Errors surfaced while compiling a WGSL shader to SPIR-V.
#[derive(Debug)]
pub enum CompileError {
    Parse { name: String, message: String },
    Validate { name: String, message: String },
    SpirVWrite { name: String, message: String },
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::Parse { name, message } => {
                write!(f, "WGSL parse error in `{name}`: {message}")
            }
            CompileError::Validate { name, message } => {
                write!(f, "WGSL validation error in `{name}`: {message}")
            }
            CompileError::SpirVWrite { name, message } => {
                write!(f, "SPIR-V write error in `{name}`: {message}")
            }
        }
    }
}

impl std::error::Error for CompileError {}

/// Compile a WGSL source string to SPIR-V words suitable for
/// `ash::Device::create_shader_module`.
pub fn wgsl_to_spirv(name: &str, source: &str) -> std::result::Result<Vec<u32>, CompileError> {
    let module = wgsl::parse_str(source).map_err(|e| CompileError::Parse {
        name: name.to_string(),
        message: e.emit_to_string(source),
    })?;

    let info = valid::Validator::new(valid::ValidationFlags::all(), valid::Capabilities::all())
        .validate(&module)
        .map_err(|e| CompileError::Validate {
            name: name.to_string(),
            message: e.emit_to_string(source),
        })?;

    let options = spv::Options::default();
    spv::write_vec(&module, &info, &options, None).map_err(|e| CompileError::SpirVWrite {
        name: name.to_string(),
        message: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aetna_core::shader::stock_wgsl;

    #[test]
    fn rounded_rect_compiles() {
        let words = wgsl_to_spirv("rounded_rect", stock_wgsl::ROUNDED_RECT)
            .expect("rounded_rect WGSL should compile");
        assert_eq!(words.first().copied(), Some(0x0723_0203));
    }

    #[test]
    fn surface_compiles() {
        let words =
            wgsl_to_spirv("surface", stock_wgsl::SURFACE).expect("surface WGSL should compile");
        assert_eq!(words.first().copied(), Some(0x0723_0203));
    }

    #[test]
    fn parse_error_carries_name() {
        let err =
            wgsl_to_spirv("broken", "not valid wgsl @@@").expect_err("invalid WGSL must fail");
        assert!(matches!(err, CompileError::Parse { .. }));
        assert!(err.to_string().contains("broken"));
    }
}
