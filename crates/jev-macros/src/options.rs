//! Shared parsing and validation for `#[derive(ChoiceOptions)]` and
//! `#[derive(ScoreLevels)]`: both derive from a fieldless enum whose variants carry
//! the option/level text in their doc comments.

use darling::FromDeriveInput;
use heck::{ToKebabCase, ToSnakeCase};
use proc_macro2::Ident;
use syn::{Data, DeriveInput, Fields};

use crate::docs::doc_comment;

/// Mirrors `jev::question::{MIN,MAX}_CHOICE_OPTIONS`. Kept in sync by
/// `tests/derive.rs`'s cross-check against the runtime builder's bounds; see that
/// module's `test_should_match_builder_bounds` for the enforcement.
pub(crate) const MIN_CHOICE_OPTIONS: usize = 2;
pub(crate) const MAX_CHOICE_OPTIONS: usize = 64;
/// Mirrors `jev::question::{MIN,MAX}_SCORE_LEVELS`.
pub(crate) const MIN_SCORE_LEVELS: usize = 2;
pub(crate) const MAX_SCORE_LEVELS: usize = 32;
/// Mirrors `jev::question::MAX_KEY_BYTES`.
pub(crate) const MAX_KEY_BYTES: usize = 64;
/// Mirrors `jev::question::MAX_GUIDANCE_BYTES`.
pub(crate) const MAX_GUIDANCE_BYTES: usize = 4096;

/// Enum-level `#[jev(...)]` attributes shared by both derives.
#[derive(Debug, FromDeriveInput)]
#[darling(attributes(jev))]
pub(crate) struct EnumAttrs {
    pub(crate) ident: Ident,
    #[darling(default)]
    pub(crate) rename_all: Option<String>,
}

/// One resolved variant: its identifier, wire key (for `ChoiceOptions`), and the
/// criteria/level text taken from its doc comment.
pub(crate) struct ResolvedVariant {
    pub(crate) ident: Ident,
    pub(crate) key: String,
    pub(crate) text: String,
}

/// Per-variant `#[jev(key = "...")]` override, parsed independently of the doc
/// comment (which `syn::Attribute::parse_nested_meta` does not see, since doc
/// comments are `#[doc = ...]`, not `#[jev(...)]`).
fn variant_key_override(attrs: &[syn::Attribute]) -> syn::Result<Option<String>> {
    let mut key = None;
    for attr in attrs {
        if !attr.path().is_ident("jev") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("key") {
                let value: syn::LitStr = meta.value()?.parse()?;
                key = Some(value.value());
                Ok(())
            } else {
                Err(meta.error("unsupported `jev` attribute here; expected `key = \"...\"`"))
            }
        })?;
    }
    Ok(key)
}

/// Applies a `rename_all` casing rule to a `PascalCase` variant identifier.
///
/// Only the three values the derive documents are accepted: `snake_case` (the
/// default), `kebab-case`, and `lowercase`. Explicit key overrides can use the
/// full ASCII allowlist `^[a-zA-Z0-9_-]{1,64}$`.
fn apply_rename_all(name: &str, rule: &str) -> Result<String, String> {
    match rule {
        "snake_case" => Ok(name.to_snake_case()),
        "kebab-case" => Ok(name.to_kebab_case()),
        "lowercase" => Ok(name.to_lowercase()),
        other => Err(format!(
            "unknown `rename_all` value `{other}`; expected one of: snake_case, kebab-case, \
             lowercase"
        )),
    }
}

/// Validates a wire key against the same allowlist `jev::question` enforces at
/// runtime for `choice` option keys: `^[a-zA-Z0-9_-]{1,64}$`.
pub(crate) fn validate_choice_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.len() > MAX_KEY_BYTES {
        return Err(format!(
            "key length must be 1..={MAX_KEY_BYTES} bytes, got {}",
            key.len()
        ));
    }
    let valid = key
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !valid {
        return Err(format!(
            "key `{key}` must match `^[a-zA-Z0-9_-]{{1,{MAX_KEY_BYTES}}}$`"
        ));
    }
    Ok(())
}

/// Validates criteria/instructions text against the same bound `jev::question`
/// enforces at runtime: non-empty, `<= 4096` bytes.
pub(crate) fn validate_guidance_text(field: &str, text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err(format!("{field} must not be empty (missing doc comment?)"));
    }
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(format!("{field} contains abnormal control characters"));
    }
    if text.len() > MAX_GUIDANCE_BYTES {
        return Err(format!(
            "{field} length must be <= {MAX_GUIDANCE_BYTES} bytes, got {}",
            text.len()
        ));
    }
    Ok(())
}

/// Parses `input` as a fieldless enum and resolves every variant's wire key and
/// doc-comment text, accumulating every violation before returning (so a caller
/// with five missing doc comments sees five errors, not one).
///
/// `key_kind` names what a key is called in error messages (`"option"` for
/// `ChoiceOptions`, `"level"` for `ScoreLevels`). Both have stable keys.
/// `(min, max)` bounds the variant count, mirroring `jev::question`'s runtime
/// bounds for the corresponding primitive.
///
/// # Clippy
///
/// `too_many_lines`: one linear validation pass over an enum's variants (shape, doc
/// comment, key resolution); splitting it would scatter one cohesive algorithm
/// across several functions for no readability gain.
#[allow(clippy::too_many_lines)]
pub(crate) fn resolve_variants(
    input: &DeriveInput,
    key_kind: &str,
    (min, max): (usize, usize),
) -> syn::Result<Vec<ResolvedVariant>> {
    let enum_attrs = <EnumAttrs as FromDeriveInput>::from_derive_input(input).map_err(|e| {
        syn::Error::new(proc_macro2::Span::call_site(), e.write_errors().to_string())
    })?;

    let Data::Enum(data_enum) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "`Decision`-adjacent derive macros require an enum (a fieldless enum of \
             options/levels)",
        ));
    };

    if data_enum.variants.len() < min {
        return Err(syn::Error::new_spanned(
            &input.ident,
            format!(
                "`{}` must have at least 2 variants; a single-variant enum cannot express a \
                 choice or a scale",
                enum_attrs.ident
            ),
        ));
    }
    if data_enum.variants.len() > max {
        return Err(syn::Error::new_spanned(
            &input.ident,
            format!(
                "`{}` has {} variants; at most {max} {key_kind}s are supported",
                enum_attrs.ident,
                data_enum.variants.len()
            ),
        ));
    }

    let mut errors: Vec<syn::Error> = Vec::new();
    let mut resolved = Vec::with_capacity(data_enum.variants.len());
    let mut seen_keys = std::collections::HashSet::new();

    for variant in &data_enum.variants {
        if !matches!(variant.fields, Fields::Unit) {
            errors.push(syn::Error::new_spanned(
                variant,
                "variants must be fieldless (data-carrying variants cannot be sent as criteria \
                 text)",
            ));
            continue;
        }

        let Some(text) = doc_comment(&variant.attrs) else {
            errors.push(syn::Error::new_spanned(
                variant,
                format!(
                    "variant `{}` has no doc comment; its `///` text is the {key_kind}'s criteria \
                     and is required",
                    variant.ident
                ),
            ));
            continue;
        };
        if let Err(reason) = validate_guidance_text(&format!("variant `{}`", variant.ident), &text)
        {
            errors.push(syn::Error::new_spanned(variant, reason));
            continue;
        }

        let key = {
            let override_key = match variant_key_override(&variant.attrs) {
                Ok(k) => k,
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            };
            let key = if let Some(k) = override_key {
                k
            } else {
                let base = variant.ident.to_string();
                match enum_attrs.rename_all.as_deref() {
                    Some(rule) => match apply_rename_all(&base, rule) {
                        Ok(k) => k,
                        Err(reason) => {
                            errors.push(syn::Error::new_spanned(&input.ident, reason));
                            continue;
                        }
                    },
                    None => base.to_snake_case(),
                }
            };
            if let Err(reason) = validate_choice_key(&key) {
                errors.push(syn::Error::new_spanned(&variant.ident, reason));
                continue;
            }
            if !seen_keys.insert(key.clone()) {
                errors.push(syn::Error::new_spanned(
                    &variant.ident,
                    format!(
                        "duplicate {key_kind} key `{key}`; every variant must have a distinct \
                         wire key"
                    ),
                ));
                continue;
            }
            key
        };

        resolved.push(ResolvedVariant {
            ident: variant.ident.clone(),
            key,
            text,
        });
    }

    if let Some(combined) = errors.into_iter().reduce(|mut acc, e| {
        acc.combine(e);
        acc
    }) {
        return Err(combined);
    }

    Ok(resolved)
}

/// Generates `impl ChoiceOptions for #ident` from a fieldless enum whose variants'
/// doc comments are the option criteria.
pub(crate) fn expand_choice_options(input: &DeriveInput) -> proc_macro2::TokenStream {
    let variants = match resolve_variants(input, "option", (MIN_CHOICE_OPTIONS, MAX_CHOICE_OPTIONS))
    {
        Ok(v) => v,
        Err(e) => return e.to_compile_error(),
    };

    let ident = &input.ident;
    let keys: Vec<&str> = variants.iter().map(|v| v.key.as_str()).collect();
    let texts: Vec<&str> = variants.iter().map(|v| v.text.as_str()).collect();

    let from_key_arms = variants.iter().map(|v| {
        let key = &v.key;
        let variant_ident = &v.ident;
        quote::quote! { #key => ::std::option::Option::Some(#ident::#variant_ident), }
    });

    let key_arms = variants.iter().map(|v| {
        let key = &v.key;
        let variant_ident = &v.ident;
        quote::quote! { #ident::#variant_ident => #key, }
    });

    quote::quote! {
        impl ::jev::ChoiceOptions for #ident {
            fn options() -> &'static [(&'static str, &'static str)] {
                &[#( (#keys, #texts) ),*]
            }

            fn from_key(key: &str) -> ::std::option::Option<Self> {
                match key {
                    #( #from_key_arms )*
                    _ => ::std::option::Option::None,
                }
            }

            fn key(&self) -> &'static str {
                match self {
                    #( #key_arms )*
                }
            }
        }
    }
}

/// Generates `impl ScoreLevels for #ident` from a fieldless enum whose variants'
/// doc comments are the level criteria, lowest first in declaration order.
pub(crate) fn expand_score_levels(input: &DeriveInput) -> proc_macro2::TokenStream {
    let variants = match resolve_variants(input, "level", (MIN_SCORE_LEVELS, MAX_SCORE_LEVELS)) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error(),
    };

    let ident = &input.ident;
    let texts: Vec<&str> = variants.iter().map(|v| v.text.as_str()).collect();

    let keys: Vec<&str> = variants.iter().map(|v| v.key.as_str()).collect();
    let from_index_arms = variants.iter().enumerate().map(|(i, v)| {
        let variant_ident = &v.ident;
        quote::quote! { #i => ::std::option::Option::Some(#ident::#variant_ident), }
    });

    let index_arms = variants.iter().enumerate().map(|(i, v)| {
        let variant_ident = &v.ident;
        quote::quote! { #ident::#variant_ident => #i, }
    });

    quote::quote! {
        impl ::jev::ScoreLevels for #ident {
            fn levels() -> &'static [::jev::LevelSpec] {
                const LEVELS: &[::jev::LevelSpec] = &[#( ::jev::LevelSpec::new(#keys, #texts) ),*];
                LEVELS
            }

            fn from_index(index: usize) -> ::std::option::Option<Self> {
                match index {
                    #( #from_index_arms )*
                    _ => ::std::option::Option::None,
                }
            }

            fn index(&self) -> usize {
                match self {
                    #( #index_arms )*
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_apply_snake_case_rename() {
        assert_eq!(
            apply_rename_all("FrontEnd", "snake_case").unwrap(),
            "front_end"
        );
    }

    #[test]
    fn test_should_apply_kebab_case_rename() {
        assert_eq!(
            apply_rename_all("FrontEnd", "kebab-case").unwrap(),
            "front-end"
        );
    }

    #[test]
    fn test_should_apply_lowercase_rename() {
        assert_eq!(
            apply_rename_all("FrontEnd", "lowercase").unwrap(),
            "frontend"
        );
    }

    #[test]
    fn test_should_reject_unknown_rename_rule() {
        assert!(apply_rename_all("FrontEnd", "bogus").is_err());
    }

    #[test]
    fn test_should_validate_choice_key_charset() {
        assert!(validate_choice_key("front-end_1").is_ok());
        assert!(validate_choice_key("FrontEnd").is_ok());
        assert!(validate_choice_key("").is_err());
        assert!(validate_choice_key(&"x".repeat(MAX_KEY_BYTES + 1)).is_err());
    }

    #[test]
    fn test_should_validate_guidance_text_bounds() {
        assert!(validate_guidance_text("field", "some text").is_ok());
        assert!(validate_guidance_text("field", "   ").is_err());
        assert!(validate_guidance_text("field", &"x".repeat(MAX_GUIDANCE_BYTES + 1)).is_err());
    }

    #[test]
    fn test_should_resolve_variants_with_default_snake_case_keys() {
        let input: DeriveInput = syn::parse_quote! {
            enum Team {
                /// Billing issues.
                Payments,
                /// UI issues.
                FrontEnd,
            }
        };
        let resolved =
            resolve_variants(&input, "option", (MIN_CHOICE_OPTIONS, MAX_CHOICE_OPTIONS)).unwrap();
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].key, "payments");
        assert_eq!(resolved[1].key, "front_end");
        assert_eq!(resolved[1].text, "UI issues.");
    }

    #[test]
    fn test_should_resolve_stable_keys_for_score_levels() {
        let input: DeriveInput = syn::parse_quote! {
            enum Urgency {
                /// Low.
                Low,
                /// High.
                High,
            }
        };
        let resolved =
            resolve_variants(&input, "level", (MIN_SCORE_LEVELS, MAX_SCORE_LEVELS)).unwrap();
        assert_eq!(resolved[0].key, "low");
        assert_eq!(resolved[0].text, "Low.");
    }

    #[test]
    fn test_should_reject_single_variant_enum() {
        let input: DeriveInput = syn::parse_quote! {
            enum Solo {
                /// Only one.
                Only,
            }
        };
        assert!(
            resolve_variants(&input, "option", (MIN_CHOICE_OPTIONS, MAX_CHOICE_OPTIONS)).is_err()
        );
    }

    #[test]
    fn test_should_reject_data_carrying_variant() {
        let input: DeriveInput = syn::parse_quote! {
            enum Mixed {
                /// A.
                A,
                /// B.
                B(u32),
            }
        };
        assert!(
            resolve_variants(&input, "option", (MIN_CHOICE_OPTIONS, MAX_CHOICE_OPTIONS)).is_err()
        );
    }

    #[test]
    fn test_should_reject_variant_missing_doc_comment() {
        let input: DeriveInput = syn::parse_quote! {
            enum Mixed {
                /// A.
                A,
                B,
            }
        };
        assert!(
            resolve_variants(&input, "option", (MIN_CHOICE_OPTIONS, MAX_CHOICE_OPTIONS)).is_err()
        );
    }

    #[test]
    fn test_should_reject_duplicate_keys_via_explicit_override() {
        let input: DeriveInput = syn::parse_quote! {
            enum Mixed {
                /// A.
                #[jev(key = "same")]
                A,
                /// B.
                #[jev(key = "same")]
                B,
            }
        };
        assert!(
            resolve_variants(&input, "option", (MIN_CHOICE_OPTIONS, MAX_CHOICE_OPTIONS)).is_err()
        );
    }

    #[test]
    fn test_should_honor_explicit_key_override() {
        let input: DeriveInput = syn::parse_quote! {
            enum Team {
                /// Billing.
                #[jev(key = "pay")]
                Payments,
                /// UI.
                Frontend,
            }
        };
        let resolved =
            resolve_variants(&input, "option", (MIN_CHOICE_OPTIONS, MAX_CHOICE_OPTIONS)).unwrap();
        assert_eq!(resolved[0].key, "pay");
    }

    #[test]
    fn test_should_reject_non_enum_input() {
        let input: DeriveInput = syn::parse_quote! {
            struct NotAnEnum;
        };
        assert!(
            resolve_variants(&input, "option", (MIN_CHOICE_OPTIONS, MAX_CHOICE_OPTIONS)).is_err()
        );
    }
}
