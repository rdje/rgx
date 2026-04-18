use crate::ast::CharRange;
use regex_syntax::{
    hir::{Class, Hir, HirKind, Literal},
    parse,
};

pub(crate) fn resolve_unicode_property_class(
    name: &str,
    negated: bool,
) -> Result<Vec<CharRange>, String> {
    // PCRE2 allows a leading `^` inside `\p{...}` / `\P{...}` as an
    // in-class negation (e.g. `\p{^Lu}` = same as `\P{Lu}`), with
    // tolerant whitespace around the marker. Strip the `^` and flip
    // the `negated` flag so the rest of the resolver sees a clean
    // property name.
    let trimmed = name.trim();
    let (name, negated) = if let Some(rest) = trimmed.strip_prefix('^') {
        (rest.trim(), !negated)
    } else {
        (trimmed, negated)
    };

    // PCRE2 recognises several property names that either are synthetic
    // (no Unicode counterpart) or use short-aliases that `regex_syntax`
    // does not accept verbatim. We intercept those first and route the
    // rest through the general resolver.
    if let Some(ranges) = resolve_pcre2_alias(name) {
        return Ok(if negated { complement(&ranges) } else { ranges });
    }

    // `sc:Arabic`, `scx:Thai` — PCRE2 script-prefix syntax. Strip the
    // prefix and delegate to the regular resolver, which accepts a bare
    // script name.
    let core_name = if let Some(rest) = name
        .strip_prefix("sc:")
        .or_else(|| name.strip_prefix("scx:"))
    {
        rest
    } else if let Some(rest) = name.strip_prefix("script:") {
        rest
    } else {
        name
    };

    let property_pattern = if negated {
        format!(r"\P{{{core_name}}}")
    } else {
        format!(r"\p{{{core_name}}}")
    };

    let hir = parse(&property_pattern)
        .map_err(|err| format!("invalid Unicode property class '{property_pattern}': {err}"))?;

    hir_to_ranges(&hir).ok_or_else(|| {
        format!("Unicode property class '{property_pattern}' did not resolve to a character class")
    })
}

/// Resolve PCRE2-specific property aliases and synthetic classes that
/// do not exist in the Unicode property database. Returns `None` if the
/// name is not a recognized PCRE2 alias; callers then fall back to the
/// standard Unicode resolver.
///
/// References: pcre2pattern(3) §"Generic character types" and §"Unicode
/// character properties".
fn resolve_pcre2_alias(name: &str) -> Option<Vec<CharRange>> {
    match name {
        // PCRE2 `L&` — "cased letter" = Lu | Ll | Lt. Identical to Unicode's
        // `Lc` ("Cased_Letter") but regex_syntax rejects `L&` as a name.
        "L&" | "Lc" | "Cased_Letter" => Some(merge_properties(&["Lu", "Ll", "Lt"])),

        // Unicode `Cs` (Surrogate) — regex_syntax rejects this because
        // surrogate codepoints (U+D800..U+DFFF) aren't valid Rust
        // `char` scalar values. For any well-formed Rust `&str` subject
        // the match can never succeed, so an empty range set is the
        // correct lowering.
        "Cs" | "Surrogate" => Some(Vec::new()),

        // PCRE2 synthetic: Xan = alphanumeric (letter or decimal digit).
        "Xan" => Some(merge_properties(&["L", "Nd"])),

        // PCRE2 synthetic: Xsp = Perl-style whitespace
        // (HT LF VT FF CR SP). Matches PCRE2's \s in non-UCP mode.
        "Xsp" => Some(vec![
            CharRange::single('\u{09}'),
            CharRange::single('\u{0A}'),
            CharRange::single('\u{0B}'),
            CharRange::single('\u{0C}'),
            CharRange::single('\u{0D}'),
            CharRange::single(' '),
        ]),

        // PCRE2 synthetic: Xps = POSIX space — same characters as Xsp.
        "Xps" => Some(vec![
            CharRange::single('\u{09}'),
            CharRange::single('\u{0A}'),
            CharRange::single('\u{0B}'),
            CharRange::single('\u{0C}'),
            CharRange::single('\u{0D}'),
            CharRange::single(' '),
        ]),

        // PCRE2 synthetic: Xwd = Perl word character — [A-Za-z0-9_].
        "Xwd" => Some(vec![
            CharRange::range('0', '9'),
            CharRange::range('A', 'Z'),
            CharRange::single('_'),
            CharRange::range('a', 'z'),
        ]),

        // PCRE2 synthetic: Xuc = "universal character name" allowed in
        // C++11: `$`, `@`, backtick, plus every codepoint ≥ U+00A0.
        "Xuc" => Some(vec![
            CharRange::single('$'),
            CharRange::single('@'),
            CharRange::single('`'),
            CharRange::range('\u{00A0}', char::MAX),
        ]),

        // PCRE2 aliases for Bidi_Control that regex_syntax does not
        // accept in lowercase/short form.
        "bidicontrol" | "bidi_c" | "bidi_control" => {
            // Bidi_Control = LRM RLM ALM LRE RLE PDF LRI RLI FSI PDI
            Some(vec![
                CharRange::single('\u{061C}'),
                CharRange::single('\u{200E}'),
                CharRange::single('\u{200F}'),
                CharRange::range('\u{202A}', '\u{202E}'),
                CharRange::range('\u{2066}', '\u{2069}'),
            ])
        }

        _ => None,
    }
}

/// PCRE2 `\d` range set under `PCRE2_UCP` — the Unicode decimal-digit
/// category (`\p{Nd}`).
pub(crate) fn ucp_digit_ranges() -> Vec<CharRange> {
    resolve_unicode_property_class("Nd", false).unwrap_or_default()
}

/// PCRE2 `\w` range set under `PCRE2_UCP` — any character in `\p{L}` or
/// `\p{N}`, plus `_`. Per pcre2pattern(3) §"Generic character types".
pub(crate) fn ucp_word_ranges() -> Vec<CharRange> {
    let mut ranges = merge_properties(&["L", "N"]);
    ranges.push(CharRange::single('_'));
    ranges.sort_by_key(|r| r.start);
    ranges
}

/// PCRE2 `\s` range set under `PCRE2_UCP` — any character in the
/// Unicode `White_Space` property.
pub(crate) fn ucp_space_ranges() -> Vec<CharRange> {
    resolve_unicode_property_class("White_Space", false).unwrap_or_default()
}

/// Merge range-sets for several Unicode property names into a single
/// sorted-disjoint range vector.
fn merge_properties(names: &[&str]) -> Vec<CharRange> {
    let mut all: Vec<CharRange> = Vec::new();
    for n in names {
        if let Ok(r) = resolve_unicode_property_class(n, false) {
            all.extend(r);
        }
    }
    all.sort_by_key(|r| r.start);
    all
}

/// Return the codepoint-space complement of `ranges` (assumed sorted).
fn complement(ranges: &[CharRange]) -> Vec<CharRange> {
    let mut sorted: Vec<CharRange> = ranges.to_vec();
    sorted.sort_by_key(|r| r.start);
    // Merge overlapping/adjacent ranges first.
    let mut merged: Vec<CharRange> = Vec::with_capacity(sorted.len());
    for r in sorted {
        if let Some(last) = merged.last_mut() {
            if (r.start as u32) <= (last.end as u32).saturating_add(1) {
                if (r.end as u32) > (last.end as u32) {
                    last.end = r.end;
                }
                continue;
            }
        }
        merged.push(r);
    }
    let mut out = Vec::new();
    let mut cursor: u32 = 0;
    for r in merged {
        let rs = r.start as u32;
        let re = r.end as u32;
        if cursor < rs {
            out.push(CharRange::range(
                char::from_u32(cursor).unwrap_or('\0'),
                char::from_u32(rs - 1).unwrap_or('\0'),
            ));
        }
        cursor = re.saturating_add(1);
    }
    if cursor <= 0x10FFFF {
        out.push(CharRange::range(
            char::from_u32(cursor).unwrap_or('\0'),
            char::MAX,
        ));
    }
    out
}

fn hir_to_ranges(hir: &Hir) -> Option<Vec<CharRange>> {
    match hir.kind() {
        HirKind::Class(Class::Unicode(class)) => Some(
            class
                .ranges()
                .iter()
                .map(|range| CharRange::range(range.start(), range.end()))
                .collect(),
        ),
        HirKind::Class(Class::Bytes(class)) => Some(
            class
                .ranges()
                .iter()
                .map(|range| CharRange::range(char::from(range.start()), char::from(range.end())))
                .collect(),
        ),
        HirKind::Literal(Literal(bytes)) => {
            let literal = std::str::from_utf8(bytes).ok()?;
            let mut chars = literal.chars();
            let ch = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            Some(vec![CharRange::single(ch)])
        }
        _ => None,
    }
}
