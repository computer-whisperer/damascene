//! WGSL → SPIR-V compilation via `naga`.
//!
//! The same naga version `wgpu` ships internally — see Cargo.toml — so
//! a shader that parses for the wgpu backend parses identically here.
//! Validation runs *before* the SPIR-V write so bad WGSL surfaces close
//! to the `register_shader` (or pipeline-build) call site rather than
//! deferred to a vulkano validator panic later.

use naga::back::spv;
use naga::front::wgsl;
use naga::valid;

/// All errors `wgsl_to_spirv` may surface, with the shader's logical
/// name attached so a failure inside `register_shader` is traceable.
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

/// Compile a WGSL source string to a SPIR-V word stream suitable for
/// handing to `vulkano::shader::ShaderModule::new`.
///
/// `name` is the logical shader name (e.g., `"rounded_rect"`); it only
/// flows into error messages.
pub fn wgsl_to_spirv(name: &str, source: &str) -> Result<Vec<u32>, CompileError> {
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

    // Default options target SPIR-V 1.0 with no debug info — good
    // enough for vulkano's loader. We don't pin a `pipeline_options`
    // here, which leaves all entry points in the module; vulkano picks
    // the one it wants by name when it builds a stage.
    let options = spv::Options::default();
    spv::write_vec(&module, &info, &options, None).map_err(|e| CompileError::SpirVWrite {
        name: name.to_string(),
        message: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use damascene_core::shader::stock_wgsl;

    fn assert_spirv_words(words: &[u32]) {
        // SPIR-V magic number is 0x07230203; the first word of any
        // valid module must be exactly that.
        assert!(!words.is_empty(), "empty SPIR-V output");
        assert_eq!(
            words[0], 0x0723_0203,
            "first word is not the SPIR-V magic number — got {:#x}",
            words[0]
        );
    }

    #[test]
    fn rounded_rect_compiles() {
        let words = wgsl_to_spirv("rounded_rect", stock_wgsl::ROUNDED_RECT)
            .expect("rounded_rect.wgsl should compile cleanly");
        assert_spirv_words(&words);
    }

    #[test]
    fn text_compiles() {
        let words =
            wgsl_to_spirv("text", stock_wgsl::TEXT).expect("text.wgsl should compile cleanly");
        assert_spirv_words(&words);
    }

    #[test]
    fn parse_error_carries_name() {
        let err = wgsl_to_spirv("broken", "not valid wgsl @@@")
            .expect_err("garbage WGSL must not compile");
        assert!(matches!(err, CompileError::Parse { .. }));
        assert!(err.to_string().contains("broken"));
    }
}
