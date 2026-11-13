//! Finds the line ranges of `#[cfg(test)]` items with `syn`, so test code is excluded
//! from the file-size count exactly rather than by a line pattern (design §12).

use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Attribute, ImplItem, Item, Meta, Token, TraitItem};

/// Inclusive 1-based line ranges covered by `#[cfg(test)]` items, attributes included.
pub fn test_line_ranges(source: &str) -> Result<Vec<(usize, usize)>, String> {
    let file = syn::parse_file(source).map_err(|e| {
        let start = e.span().start();
        format!("parse error at {}:{}: {e}", start.line, start.column)
    })?;
    let mut ranges = Vec::new();
    collect_items(&file.items, &mut ranges);
    Ok(ranges)
}

fn collect_items(items: &[Item], ranges: &mut Vec<(usize, usize)>) {
    for item in items {
        if is_cfg_test(item_attrs(item)) {
            ranges.push(line_range(item));
            continue;
        }
        match item {
            Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    collect_items(inner, ranges);
                }
            }
            Item::Impl(i) => {
                for it in &i.items {
                    if is_cfg_test(impl_item_attrs(it)) {
                        ranges.push(line_range(it));
                    }
                }
            }
            Item::Trait(t) => {
                for it in &t.items {
                    if is_cfg_test(trait_item_attrs(it)) {
                        ranges.push(line_range(it));
                    }
                }
            }
            _ => {}
        }
    }
}

/// `true` for `#[cfg(test)]` and `#[cfg(all(test, …))]`: code that exists only in
/// test builds. `any(test, …)` and `not(test)` also exist outside tests and count.
fn is_cfg_test(attrs: &[Attribute]) -> bool {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("cfg"))
        .any(|a| a.parse_args::<Meta>().is_ok_and(|m| implies_test(&m)))
}

fn implies_test(meta: &Meta) -> bool {
    match meta {
        Meta::Path(p) => p.is_ident("test"),
        Meta::List(l) if l.path.is_ident("all") => l
            .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            .is_ok_and(|inner| inner.iter().any(implies_test)),
        _ => false,
    }
}

/// Outer attributes are part of the item's tokens, so the span starts at `#[cfg(test)]`.
/// Outside a proc macro, proc-macro2 joins first and last token span exactly.
fn line_range(node: &impl Spanned) -> (usize, usize) {
    let span = node.span();
    (span.start().line, span.end().line)
}

fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(i) => &i.attrs,
        Item::Enum(i) => &i.attrs,
        Item::ExternCrate(i) => &i.attrs,
        Item::Fn(i) => &i.attrs,
        Item::ForeignMod(i) => &i.attrs,
        Item::Impl(i) => &i.attrs,
        Item::Macro(i) => &i.attrs,
        Item::Mod(i) => &i.attrs,
        Item::Static(i) => &i.attrs,
        Item::Struct(i) => &i.attrs,
        Item::Trait(i) => &i.attrs,
        Item::TraitAlias(i) => &i.attrs,
        Item::Type(i) => &i.attrs,
        Item::Union(i) => &i.attrs,
        Item::Use(i) => &i.attrs,
        _ => &[],
    }
}

fn impl_item_attrs(item: &ImplItem) -> &[Attribute] {
    match item {
        ImplItem::Const(i) => &i.attrs,
        ImplItem::Fn(i) => &i.attrs,
        ImplItem::Type(i) => &i.attrs,
        ImplItem::Macro(i) => &i.attrs,
        _ => &[],
    }
}

fn trait_item_attrs(item: &TraitItem) -> &[Attribute] {
    match item {
        TraitItem::Const(i) => &i.attrs,
        TraitItem::Fn(i) => &i.attrs,
        TraitItem::Type(i) => &i.attrs,
        TraitItem::Macro(i) => &i.attrs,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_test_module_with_attribute_line() {
        let src = "fn a() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n";
        assert_eq!(test_line_ranges(src).unwrap(), vec![(3, 7)]);
    }

    #[test]
    fn finds_nested_and_impl_items() {
        let src = "mod m {\n    #[cfg(all(test, unix))]\n    fn h() {}\n}\nstruct S;\nimpl S {\n    #[cfg(test)]\n    fn x() {}\n}\n";
        assert_eq!(test_line_ranges(src).unwrap(), vec![(2, 3), (7, 8)]);
    }

    #[test]
    fn any_and_not_test_still_count() {
        let src = "#[cfg(any(test, unix))]\nfn a() {}\n#[cfg(not(test))]\nfn b() {}\n";
        assert!(test_line_ranges(src).unwrap().is_empty());
    }

    #[test]
    fn parse_error_is_reported() {
        assert!(test_line_ranges("fn (").is_err());
    }
}
