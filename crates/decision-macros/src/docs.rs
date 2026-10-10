//! Doc-comment extraction: the `///` on an item becomes the exact prompt text sent
//! to the model. This module has one job — pull that text out of `syn::Attribute`s
//! and normalize it the same way `cargo doc` would read it.

use syn::{Attribute, Expr, Lit, Meta};

/// Extracts and joins every `#[doc = "..."]` attribute on `attrs`, trimming each
/// line and joining with a single space.
///
/// Returns `None` when there are no doc comments at all, so callers can distinguish
/// "no doc comment" (usually a compile error) from "doc comment present but blank"
/// (also invalid, but a different message).
pub(crate) fn doc_comment(attrs: &[Attribute]) -> Option<String> {
    let mut lines = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let Meta::NameValue(nv) = &attr.meta
            && let Expr::Lit(expr_lit) = &nv.value
            && let Lit::Str(s) = &expr_lit.lit
        {
            lines.push(s.value());
        }
    }
    if lines.is_empty() {
        return None;
    }
    let joined = lines
        .iter()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    Some(joined)
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::parse_quote;

    use super::*;

    #[test]
    fn test_should_extract_single_line_doc_comment() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[doc = " Is this a bug? "])];
        assert_eq!(doc_comment(&attrs), Some("Is this a bug?".to_string()));
    }

    #[test]
    fn test_should_join_multi_line_doc_comment_with_single_space() {
        let item: syn::DeriveInput = parse_quote! {
            /// Line one.
            /// Line two.
            struct S;
        };
        assert_eq!(
            doc_comment(&item.attrs),
            Some("Line one. Line two.".to_string())
        );
    }

    #[test]
    fn test_should_collapse_indentation_in_doc_comment() {
        let item: syn::DeriveInput = parse_quote! {
            /// Line one.
            ///     Indented line two.
            struct S;
        };
        assert_eq!(
            doc_comment(&item.attrs),
            Some("Line one. Indented line two.".to_string())
        );
    }

    #[test]
    fn test_should_return_none_when_no_doc_comment_present() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[allow(dead_code)])];
        assert_eq!(doc_comment(&attrs), None);
    }

    #[test]
    fn test_should_ignore_non_doc_attributes_while_extracting() {
        let item: syn::DeriveInput = parse_quote! {
            #[allow(dead_code)]
            /// The real doc.
            #[derive(Debug)]
            struct S;
        };
        assert_eq!(doc_comment(&item.attrs), Some("The real doc.".to_string()));
    }

    #[test]
    fn test_should_be_usable_with_tokenstream_parsed_attrs() {
        let tokens = quote! {
            /// hello
            struct S;
        };
        let item: syn::DeriveInput = syn::parse2(tokens).unwrap();
        assert_eq!(doc_comment(&item.attrs), Some("hello".to_string()));
    }
}
