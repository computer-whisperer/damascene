//! Serializable MSDF atlas snapshots — bake once, load many.
//!
//! Per-glyph MTSDF generation is deterministic and dominates renderer
//! cold start, so re-running it every launch is wasted work. A snapshot
//! captures the baked bitmaps + metrics in a flat binary blob that an
//! atlas can re-load in a memcpy/repack instead of regenerating.
//!
//! ## Portability
//!
//! [`crate::text::msdf_atlas::MsdfGlyphKey`] is keyed by `fontdb::ID`,
//! which is assigned at font-load time and is **not** stable across
//! processes — a build-time bake and the runtime have different IDs for
//! the same font. So a snapshot keys each glyph run by a caller-chosen
//! `u64` *font token* instead, and the atlas's export/import map runtime
//! IDs to/from tokens via caller-supplied closures.
//!
//! ## Staleness guard
//!
//! The header records the bake parameters ([`SnapshotHeader`]). Import
//! rejects a snapshot whose parameters don't match the importing atlas
//! (or whose magic/version is wrong, or that's truncated), so the caller
//! falls back to live generation. A stale snapshot is therefore always
//! *safe* — ignored, never used to paint wrong glyphs.
//!
//! This module depends only on external crates and [`super::msdf`]
//! (itself leaf), so a `build.rs` can share it via `#[path]` to bake the
//! default-font snapshot at compile time.

use ttf_parser::Face;

use super::msdf::{MsdfGlyph, build_glyph_msdf, glyph_advance};

const MAGIC: &[u8; 4] = b"DMSF";
const FORMAT_VERSION: u16 = 1;

/// Bake parameters a snapshot was produced with. Import compares these
/// against the importing atlas and rejects on any mismatch — a snapshot
/// baked at a different `base_em`/`spread` holds wrong-sized bitmaps for
/// the live atlas, so it must not be loaded.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SnapshotHeader {
    /// Em size (atlas px) the MTSDFs were generated at.
    pub base_em: u32,
    /// SDF spread radius in atlas px.
    pub spread: f64,
    /// Whether fdsm's error-correction pass was run during the bake.
    pub error_correction: bool,
}

/// One glyph in a snapshot section.
#[derive(Clone, Debug, PartialEq)]
pub enum SnapshotGlyph {
    /// An outline glyph with a baked MTSDF bitmap + metrics.
    Outline(MsdfGlyph),
    /// Whitespace / `.notdef` without contours — only the advance is
    /// meaningful (mirrors `MsdfEntry::Empty`).
    Empty {
        /// Horizontal advance width in base-em px.
        advance: f32,
    },
}

/// A per-font run: a stable caller-chosen token and the font's glyphs
/// keyed by glyph id.
pub type SnapshotSection = (u64, Vec<(u16, SnapshotGlyph)>);

/// Why a snapshot could not be loaded. All variants mean "ignore this
/// snapshot and generate live"; none indicate a correctness hazard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    /// Leading bytes aren't the snapshot magic — not a snapshot blob.
    BadMagic,
    /// Snapshot format version this build can't read.
    UnsupportedVersion(u16),
    /// Bake parameters differ from the importing atlas (`base_em`,
    /// `spread`, or `error_correction`).
    ParamMismatch,
    /// Blob ended mid-record.
    Truncated,
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::BadMagic => write!(f, "not an MSDF snapshot (bad magic)"),
            SnapshotError::UnsupportedVersion(v) => {
                write!(f, "unsupported MSDF snapshot version {v}")
            }
            SnapshotError::ParamMismatch => {
                write!(f, "snapshot bake parameters differ from the atlas")
            }
            SnapshotError::Truncated => write!(f, "MSDF snapshot is truncated"),
        }
    }
}

impl std::error::Error for SnapshotError {}

// --- encoding ---------------------------------------------------------

fn push_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn push_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn push_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn push_f32(buf: &mut Vec<u8>, v: f32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

/// Serialize a snapshot from per-font sections. Pure — usable from a
/// build script. The on-disk layout is little-endian:
///
/// ```text
/// magic[4] version:u16 base_em:u32 spread_bits:u64 error_correction:u8
/// section_count:u32
///   { token:u64 glyph_count:u32
///     { glyph_id:u16 tag:u8
///       tag=0 (empty):   advance:f32
///       tag=1 (outline): w:u32 h:u32 bearing_x:f32 bearing_y:f32
///                        advance:f32 spread:f32 rgba_len:u32 rgba[len] } }
/// ```
pub fn encode_snapshot(header: SnapshotHeader, sections: &[SnapshotSection]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(MAGIC);
    push_u16(&mut buf, FORMAT_VERSION);
    push_u32(&mut buf, header.base_em);
    push_u64(&mut buf, header.spread.to_bits());
    buf.push(header.error_correction as u8);
    push_u32(&mut buf, sections.len() as u32);
    for (token, glyphs) in sections {
        push_u64(&mut buf, *token);
        push_u32(&mut buf, glyphs.len() as u32);
        for (glyph_id, g) in glyphs {
            push_u16(&mut buf, *glyph_id);
            match g {
                SnapshotGlyph::Empty { advance } => {
                    buf.push(0);
                    push_f32(&mut buf, *advance);
                }
                SnapshotGlyph::Outline(m) => {
                    buf.push(1);
                    push_u32(&mut buf, m.width);
                    push_u32(&mut buf, m.height);
                    push_f32(&mut buf, m.bearing_x);
                    push_f32(&mut buf, m.bearing_y);
                    push_f32(&mut buf, m.advance);
                    push_f32(&mut buf, m.spread);
                    push_u32(&mut buf, m.rgba.len() as u32);
                    buf.extend_from_slice(&m.rgba);
                }
            }
        }
    }
    buf
}

// --- decoding ---------------------------------------------------------

/// Bounds-checked little-endian cursor; every read returns `Truncated`
/// rather than panicking past the end of the blob.
struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], SnapshotError> {
        let end = self.pos.checked_add(n).ok_or(SnapshotError::Truncated)?;
        let s = self.b.get(self.pos..end).ok_or(SnapshotError::Truncated)?;
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, SnapshotError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, SnapshotError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, SnapshotError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, SnapshotError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, SnapshotError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

/// Decode and validate a snapshot against the importing atlas's
/// parameters. Returns the per-font sections, or a [`SnapshotError`]
/// (the caller then generates live).
pub fn decode_snapshot(
    bytes: &[u8],
    expected: SnapshotHeader,
) -> Result<Vec<SnapshotSection>, SnapshotError> {
    let mut r = Reader { b: bytes, pos: 0 };
    if r.take(4)? != MAGIC {
        return Err(SnapshotError::BadMagic);
    }
    let version = r.u16()?;
    if version != FORMAT_VERSION {
        return Err(SnapshotError::UnsupportedVersion(version));
    }
    let base_em = r.u32()?;
    let spread = f64::from_bits(r.u64()?);
    let error_correction = r.u8()? != 0;
    if base_em != expected.base_em
        || spread != expected.spread
        || error_correction != expected.error_correction
    {
        return Err(SnapshotError::ParamMismatch);
    }
    let section_count = r.u32()? as usize;
    let mut sections = Vec::with_capacity(section_count);
    for _ in 0..section_count {
        let token = r.u64()?;
        let glyph_count = r.u32()? as usize;
        let mut glyphs = Vec::with_capacity(glyph_count);
        for _ in 0..glyph_count {
            let glyph_id = r.u16()?;
            let tag = r.u8()?;
            let g = match tag {
                0 => SnapshotGlyph::Empty { advance: r.f32()? },
                1 => {
                    let width = r.u32()?;
                    let height = r.u32()?;
                    let bearing_x = r.f32()?;
                    let bearing_y = r.f32()?;
                    let advance = r.f32()?;
                    let spread = r.f32()?;
                    let rgba_len = r.u32()? as usize;
                    let rgba = r.take(rgba_len)?.to_vec();
                    SnapshotGlyph::Outline(MsdfGlyph {
                        rgba,
                        width,
                        height,
                        bearing_x,
                        bearing_y,
                        advance,
                        spread,
                    })
                }
                _ => return Err(SnapshotError::Truncated),
            };
            glyphs.push((glyph_id, g));
        }
        sections.push((token, glyphs));
    }
    Ok(sections)
}

/// Stable content hash of a font's bytes, suitable as a snapshot font
/// token (FNV-1a 64). Deterministic and dependency-free, so the same
/// font produces the same token across processes and builds — the basis
/// for the app-facing export/import path, which keys glyph runs by the
/// hash of each font's data rather than its runtime `fontdb::ID`.
pub fn font_token_hash(font_bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for &b in font_bytes {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

// --- baking -----------------------------------------------------------

/// Bake one font's glyphs (resolved from `chars`) into a snapshot
/// section. Outline glyphs carry their MTSDF; whitespace/`.notdef`
/// without contours carry just the advance. Chars the face has no glyph
/// for are skipped.
///
/// Pure and leaf-only, so a `build.rs` can call it to bake the
/// default-font snapshot at compile time.
pub fn bake_font_section(
    face: &Face<'_>,
    token: u64,
    chars: &[char],
    base_em: u32,
    spread: f64,
    correct_error: bool,
) -> SnapshotSection {
    let mut glyphs = Vec::new();
    for &ch in chars {
        let Some(glyph_id) = face.glyph_index(ch) else {
            continue;
        };
        let gid = glyph_id.0;
        let g = match build_glyph_msdf(face, gid, base_em, spread, correct_error) {
            Some(m) => SnapshotGlyph::Outline(m),
            None => SnapshotGlyph::Empty {
                advance: glyph_advance(face, gid, base_em),
            },
        };
        glyphs.push((gid, g));
    }
    (token, glyphs)
}

#[cfg(all(test, feature = "inter"))]
mod tests {
    use super::*;

    fn header() -> SnapshotHeader {
        SnapshotHeader {
            base_em: 48,
            spread: 6.0,
            error_correction: false,
        }
    }

    #[test]
    fn round_trips_a_baked_section() {
        let face = Face::parse(damascene_fonts::INTER_VARIABLE, 0).unwrap();
        let chars: Vec<char> = "Ag @".chars().collect(); // outline + whitespace
        let section = bake_font_section(&face, 7, &chars, 48, 6.0, false);
        let bytes = encode_snapshot(header(), &[section]);

        let decoded = decode_snapshot(&bytes, header()).expect("decodes");
        assert_eq!(decoded.len(), 1);
        let (token, glyphs) = &decoded[0];
        assert_eq!(*token, 7);
        // 'A','g','@' outline + ' ' empty = 4 entries.
        assert_eq!(glyphs.len(), 4);
        let empties = glyphs
            .iter()
            .filter(|(_, g)| matches!(g, SnapshotGlyph::Empty { .. }))
            .count();
        assert_eq!(empties, 1, "space is the one empty glyph");
        for (_, g) in glyphs {
            if let SnapshotGlyph::Outline(m) = g {
                assert_eq!(m.rgba.len() as u32, m.width * m.height * 4);
            }
        }
    }

    #[test]
    fn font_token_hash_is_stable_and_distinct() {
        let a = font_token_hash(damascene_fonts::INTER_VARIABLE);
        assert_eq!(
            a,
            font_token_hash(damascene_fonts::INTER_VARIABLE),
            "deterministic"
        );
        assert_ne!(a, font_token_hash(b"not a font"), "distinct inputs differ");
    }

    #[test]
    fn rejects_param_mismatch() {
        let face = Face::parse(damascene_fonts::INTER_VARIABLE, 0).unwrap();
        let section = bake_font_section(&face, 0, &['A'], 48, 6.0, false);
        let bytes = encode_snapshot(header(), &[section]);
        let wrong = SnapshotHeader {
            base_em: 32,
            ..header()
        };
        assert_eq!(
            decode_snapshot(&bytes, wrong),
            Err(SnapshotError::ParamMismatch)
        );
    }

    #[test]
    fn rejects_bad_magic_and_truncation() {
        assert_eq!(
            decode_snapshot(b"xxxx....", header()),
            Err(SnapshotError::BadMagic)
        );
        let face = Face::parse(damascene_fonts::INTER_VARIABLE, 0).unwrap();
        let bytes = encode_snapshot(
            header(),
            &[bake_font_section(&face, 0, &['A'], 48, 6.0, false)],
        );
        assert_eq!(
            decode_snapshot(&bytes[..bytes.len() - 4], header()),
            Err(SnapshotError::Truncated)
        );
    }
}
