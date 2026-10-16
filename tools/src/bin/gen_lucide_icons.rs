//! Regenerate the built-in icon vocabulary from a Lucide checkout.
//!
//! The whole Lucide set ships in `damascene-core` — names verbatim,
//! markup verbatim — and is maintained by re-running this rather than
//! by hand. Pin a tag:
//!
//! ```text
//! git clone --depth 1 --branch 1.57.0 https://github.com/lucide-icons/lucide /tmp/lucide
//! cargo run -p damascene-tools --bin gen_lucide_icons -- /tmp/lucide 1.57.0
//! ```
//!
//! Output: `crates/damascene-core/src/icons/lucide.rs` (the `IconName`
//! enum and its name / markup / alias tables). The variants are
//! Lucide's current names only: a name upstream retires stops compiling
//! and lives on as a string alias in `IconName::parse` (ruled
//! 2026-10-16, no compatibility constants). Record the tag in
//! CREDITS.md and `docs/NAMING_ORACLE.md` when it moves.
//!
//! Lucide is ISC-licensed (`LICENSE` in the checkout); see CREDITS.md.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Names the house accepts beyond Lucide's own: the short forms agents
/// reach for first. Each maps to a current Lucide name.
const HOUSE_ALIASES: &[(&str, &str)] = &[
    ("alert", "circle-alert"),
    ("close", "x"),
    ("dashboard", "layout-dashboard"),
    ("file", "file-text"),
    ("logout", "log-out"),
    ("message", "message-square"),
    ("microphone", "mic"),
    ("more", "ellipsis"),
    ("mute", "volume-x"),
    ("refresh", "refresh-cw"),
    ("rotate", "rotate-cw"),
    ("volume", "volume-2"),
];

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let checkout = PathBuf::from(
        args.next()
            .ok_or("usage: gen_lucide_icons <lucide-checkout> <tag>")?,
    );
    let tag = args
        .next()
        .ok_or("usage: gen_lucide_icons <lucide-checkout> <tag>")?;
    let out = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("crates/damascene-core/src/icons/lucide.rs"));

    let commit = git(&checkout, &["rev-parse", "HEAD"])?;
    let date = git(&checkout, &["log", "-1", "--format=%cs"])?;

    let icons_dir = checkout.join("icons");
    let mut names: Vec<String> = fs::read_dir(&icons_dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            (p.extension()? == "svg").then(|| p.file_stem()?.to_str().map(str::to_owned))?
        })
        .collect();
    names.sort();
    names.dedup();
    if names.is_empty() {
        return Err(format!("no icons under {}", icons_dir.display()).into());
    }
    let name_set: BTreeSet<&str> = names.iter().map(String::as_str).collect();

    // Variant identifiers: kebab-case → PascalCase, checked unique and valid.
    let mut idents = BTreeMap::new();
    for name in &names {
        let ident = pascal(name);
        if !ident
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        {
            return Err(format!("icon `{name}` does not make a Rust identifier ({ident})").into());
        }
        if idents.insert(ident.clone(), name.clone()).is_some() {
            return Err(format!("two icons pascal-case to `{ident}`").into());
        }
    }
    let ident_of = |name: &str| pascal(name);

    // Markup: each file's elements, verbatim, minus the `<svg>` wrapper.
    let mut markup = Vec::with_capacity(names.len());
    for name in &names {
        let svg = fs::read_to_string(icons_dir.join(format!("{name}.svg")))?;
        let doc = roxmltree::Document::parse(&svg)?;
        let mut s = String::new();
        for child in doc.root_element().children().filter(|c| c.is_element()) {
            serialize(child, &mut s);
        }
        if s.is_empty() {
            return Err(format!("icon `{name}` has no elements").into());
        }
        if s.contains("\"##") {
            return Err(format!("icon `{name}` markup cannot be a raw string").into());
        }
        markup.push(s);
    }

    // Aliases: Lucide's deprecated names (icons/*.json) plus the house list.
    let mut aliases: BTreeMap<String, String> = BTreeMap::new();
    let mut note = |alias: &str, canonical: &str| -> Result<(), Box<dyn Error>> {
        if name_set.contains(alias) {
            // An alias that is also a live icon is that icon.
            return Ok(());
        }
        if !name_set.contains(canonical) {
            return Err(format!("alias `{alias}` points at unknown icon `{canonical}`").into());
        }
        if let Some(prev) = aliases.insert(alias.to_owned(), canonical.to_owned())
            && prev != canonical
        {
            return Err(format!("alias `{alias}` names both `{prev}` and `{canonical}`").into());
        }
        Ok(())
    };
    for name in &names {
        let json = fs::read_to_string(icons_dir.join(format!("{name}.json")))?;
        let meta: serde_json::Value = serde_json::from_str(&json)?;
        if let Some(list) = meta.get("aliases").and_then(|a| a.as_array()) {
            for a in list {
                if let Some(alias) = a.get("name").and_then(|n| n.as_str()) {
                    note(alias, name)?;
                }
            }
        }
    }
    for (alias, canonical) in HOUSE_ALIASES {
        note(alias, canonical)?;
    }

    // ---- emit ----
    let n = names.len();
    let mut o = String::new();
    writeln!(
        o,
        "//! The built-in icon vocabulary: [Lucide](https://lucide.dev/) {tag}, wholesale."
    )?;
    writeln!(o, "//!")?;
    writeln!(
        o,
        "//! GENERATED by `tools/src/bin/gen_lucide_icons.rs` from the Lucide checkout at"
    )?;
    writeln!(
        o,
        "//! tag `{tag}` (commit `{commit}`, {date}) — {n} icons, {} aliases. Do not edit;",
        aliases.len()
    )?;
    writeln!(
        o,
        "//! re-run the generator against a newer tag and record it in CREDITS.md and"
    )?;
    writeln!(
        o,
        "//! `docs/NAMING_ORACLE.md`. Names and markup are upstream's verbatim (ISC; see"
    )?;
    writeln!(
        o,
        "//! CREDITS.md). Lucide's deprecated names resolve through [`IconName::parse`];"
    )?;
    writeln!(o, "//! the variants themselves are the current names only.")?;
    writeln!(o)?;
    writeln!(
        o,
        "/// A built-in icon: one of Lucide's {n} glyphs, named in Rust case"
    )?;
    writeln!(
        o,
        "/// (`chevron-down` → [`IconName::ChevronDown`]). The string form is the"
    )?;
    writeln!(
        o,
        "/// kebab-case Lucide name, which [`IconName::parse`] accepts along with"
    )?;
    writeln!(
        o,
        "/// Lucide's deprecated aliases and a few house short forms (`close`,"
    )?;
    writeln!(o, "/// `dashboard`, `refresh`, …).")?;
    writeln!(
        o,
        "#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]"
    )?;
    writeln!(o, "#[non_exhaustive]")?;
    writeln!(o, "#[repr(u16)]")?;
    writeln!(o, "pub enum IconName {{")?;
    for name in &names {
        writeln!(o, "    /// `{name}`")?;
        writeln!(o, "    {},", ident_of(name))?;
    }
    writeln!(o, "}}")?;
    writeln!(o)?;
    writeln!(o, "impl IconName {{")?;
    writeln!(o, "    /// Number of built-in icons.")?;
    writeln!(o, "    pub const COUNT: usize = {n};")?;
    writeln!(o)?;
    writeln!(
        o,
        "    /// Every icon, in alphabetical (declaration) order — `ALL[i] as usize == i`."
    )?;
    writeln!(o, "    pub const ALL: &[IconName] = &[")?;
    for name in &names {
        writeln!(o, "        IconName::{},", ident_of(name))?;
    }
    writeln!(o, "    ];")?;
    writeln!(o)?;
    writeln!(
        o,
        "    /// The canonical kebab-case Lucide name (the inverse of [`IconName::parse`],"
    )?;
    writeln!(o, "    /// modulo aliases).")?;
    writeln!(o, "    pub fn name(self) -> &'static str {{")?;
    writeln!(o, "        NAMES[self as usize]")?;
    writeln!(o, "    }}")?;
    writeln!(o)?;
    writeln!(
        o,
        "    /// The icon's SVG elements in Lucide's 24×24 view box, verbatim, without"
    )?;
    writeln!(
        o,
        "    /// the `<svg>` wrapper (paint comes from the rendering context)."
    )?;
    writeln!(o, "    pub fn markup(self) -> &'static str {{")?;
    writeln!(o, "        MARKUP[self as usize]")?;
    writeln!(o, "    }}")?;
    writeln!(o)?;
    writeln!(
        o,
        "    /// Resolve a Lucide-style string name (`\"chevron-down\"`), one of Lucide's"
    )?;
    writeln!(
        o,
        "    /// deprecated aliases (`\"alert-circle\"`), or a house short form"
    )?;
    writeln!(
        o,
        "    /// (`\"close\"`) to its variant; `None` for anything else."
    )?;
    writeln!(o, "    pub fn parse(name: &str) -> Option<Self> {{")?;
    writeln!(o, "        if let Ok(i) = NAMES.binary_search(&name) {{")?;
    writeln!(o, "            return Some(Self::ALL[i]);")?;
    writeln!(o, "        }}")?;
    writeln!(o, "        ALIASES")?;
    writeln!(
        o,
        "            .binary_search_by(|(alias, _)| (*alias).cmp(name))"
    )?;
    writeln!(o, "            .ok()")?;
    writeln!(o, "            .map(|i| ALIASES[i].1)")?;
    writeln!(o, "    }}")?;
    writeln!(o, "}}")?;
    writeln!(o)?;
    writeln!(o, "static NAMES: [&str; {n}] = [")?;
    for name in &names {
        writeln!(o, "    \"{name}\",")?;
    }
    writeln!(o, "];")?;
    writeln!(o)?;
    writeln!(
        o,
        "static ALIASES: [(&str, IconName); {}] = [",
        aliases.len()
    )?;
    for (alias, canonical) in &aliases {
        writeln!(o, "    (\"{alias}\", IconName::{}),", ident_of(canonical))?;
    }
    writeln!(o, "];")?;
    writeln!(o)?;
    writeln!(o, "static MARKUP: [&str; {n}] = [")?;
    for m in &markup {
        writeln!(o, "    r##\"{m}\"##,")?;
    }
    writeln!(o, "];")?;

    fs::write(&out, o)?;
    println!(
        "wrote {} — {n} icons, {} aliases from Lucide {tag} @ {commit}",
        out.display(),
        aliases.len()
    );
    Ok(())
}

fn git(repo: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()?;
    if !out.status.success() {
        return Err(format!("git {} failed in {}", args.join(" "), repo.display()).into());
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_owned())
}

/// `a-arrow-down` → `AArrowDown`, `volume-2` → `Volume2`.
fn pascal(name: &str) -> String {
    name.split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Serialize an element and its descendants compactly (`<tag k="v"/>`),
/// attributes in document order, no whitespace text.
fn serialize(node: roxmltree::Node, out: &mut String) {
    out.push('<');
    out.push_str(node.tag_name().name());
    for attr in node.attributes() {
        out.push(' ');
        out.push_str(attr.name());
        out.push_str("=\"");
        out.push_str(attr.value());
        out.push('"');
    }
    let children: Vec<_> = node.children().filter(|c| c.is_element()).collect();
    if children.is_empty() {
        out.push_str("/>");
    } else {
        out.push('>');
        for child in children {
            serialize(child, out);
        }
        out.push_str("</");
        out.push_str(node.tag_name().name());
        out.push('>');
    }
}
