use crate::ast::CharRange;
use regex_syntax::{
    hir::{Class, Hir, HirKind, Literal},
    parse,
};

pub(crate) fn resolve_unicode_property_class(
    name: &str,
    negated: bool,
) -> Result<Vec<CharRange>, String> {
    let property_pattern = if negated {
        format!(r"\P{{{name}}}")
    } else {
        format!(r"\p{{{name}}}")
    };

    let hir = parse(&property_pattern).map_err(|err| {
        format!(
            "invalid Unicode property class '{}': {}",
            property_pattern, err
        )
    })?;

    hir_to_ranges(&hir).ok_or_else(|| {
        format!(
            "Unicode property class '{}' did not resolve to a character class",
            property_pattern
        )
    })
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
