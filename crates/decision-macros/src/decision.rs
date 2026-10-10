//! `#[derive(Decision)]`: the struct derive that turns doc comments into `instructions`
//! / criteria and generates `impl Decision`.

use darling::FromField;
use heck::ToSnakeCase;
use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Fields, GenericArgument, PathArguments, Type};

use crate::docs::doc_comment;

/// Mirrors `decision_rs::question`'s runtime bounds; see `crate::options` for the full
/// rationale (kept in sync by `tests/derive.rs`).
const MAX_GUIDANCE_BYTES: usize = 4096;
const MAX_KEY_BYTES: usize = 64;

/// Per-field `#[decision(...)]` attributes.
#[derive(Debug, Default, FromField)]
#[darling(attributes(decision), default)]
struct FieldAttrs {
    key: Option<String>,
    instructions: Option<String>,
    yes: Option<String>,
    no: Option<String>,
}

/// The primitive a field maps to, after unwrapping an outer `Outcome<_>`.
enum FieldKind<'a> {
    Predicate,
    Choice(&'a Type),
    Score(&'a Type),
}

/// One resolved field: its wire key, instructions, primitive, and whether refusal is
/// explicitly retained in `Outcome`.
struct ResolvedField<'a> {
    field_ident: &'a Ident,
    key: String,
    instructions: String,
    yes: Option<String>,
    no: Option<String>,
    kind: FieldKind<'a>,
    outcome: bool,
}

/// Derives `impl Decision` for a struct whose fields are `Predicate`, `Choice<T>`,
/// `Score<T>`, or `Outcome<_>` of those.
///
/// # Clippy
///
/// `too_many_lines`: this is one linear validation pass over a struct's fields
/// (key, instructions, criteria, shape), each step independently short; splitting it
/// would scatter one cohesive algorithm across several functions for no readability
/// gain.
#[allow(clippy::too_many_lines)]
pub(crate) fn expand_decision(input: &DeriveInput) -> TokenStream {
    if !input.generics.params.is_empty() {
        return syn::Error::new_spanned(
            &input.generics,
            "Decision requires a concrete struct without generic parameters",
        )
        .to_compile_error();
    }
    let Data::Struct(data_struct) = &input.data else {
        return syn::Error::new_spanned(
            &input.ident,
            "`Decision` can only be derived for a struct (one field per question)",
        )
        .to_compile_error();
    };
    let Fields::Named(fields) = &data_struct.fields else {
        return syn::Error::new_spanned(
            &input.ident,
            "`Decision` requires named fields (tuple and unit structs have no field to name a \
             question after)",
        )
        .to_compile_error();
    };

    let mut errors: Vec<syn::Error> = Vec::new();
    let mut resolved = Vec::with_capacity(fields.named.len());
    let mut seen_keys = std::collections::HashSet::new();

    for field in &fields.named {
        let Some(field_ident) = field.ident.as_ref() else {
            continue;
        };

        let attrs = match FieldAttrs::from_field(field) {
            Ok(a) => a,
            Err(e) => {
                errors.push(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    e.write_errors().to_string(),
                ));
                continue;
            }
        };

        let (inner_type, field_outcome) = unwrap_outcome(&field.ty);
        let Some(kind) = classify_field_type(inner_type) else {
            errors.push(syn::Error::new_spanned(
                &field.ty,
                "unsupported `Decision` field type; expected `Predicate`, `Choice<T>`, \
                 `Score<T>`, or `Outcome<_>` of one of those",
            ));
            continue;
        };

        let key = match &attrs.key {
            Some(k) => k.clone(),
            None => field_ident.to_string().to_snake_case(),
        };
        if let Err(reason) = validate_key(&key) {
            errors.push(syn::Error::new_spanned(field_ident, reason));
            continue;
        }
        if !seen_keys.insert(key.clone()) {
            errors.push(syn::Error::new_spanned(
                field_ident,
                format!("duplicate question key `{key}`; every field must have a distinct key"),
            ));
            continue;
        }

        let Some(instructions) = attrs
            .instructions
            .clone()
            .or_else(|| doc_comment(&field.attrs))
        else {
            errors.push(syn::Error::new_spanned(
                field_ident,
                "field has no doc comment and no `#[decision(instructions = \"...\")]`; an \
                 instruction-less question is a guaranteed HTTP 400",
            ));
            continue;
        };
        if let Err(reason) = validate_guidance(
            &format!("field `{field_ident}` instructions"),
            &instructions,
        ) {
            errors.push(syn::Error::new_spanned(field_ident, reason));
            continue;
        }

        if matches!(kind, FieldKind::Predicate) {
            match (&attrs.yes, &attrs.no) {
                (Some(_), None) | (None, Some(_)) => {
                    errors.push(syn::Error::new_spanned(
                        field_ident,
                        "`#[decision(yes = ...)]` and `#[decision(no = ...)]` must both be \
                         present or both absent",
                    ));
                    continue;
                }
                (Some(yes), Some(no)) => {
                    if let Err(reason) =
                        validate_guidance(&format!("field `{field_ident}` yes"), yes)
                    {
                        errors.push(syn::Error::new_spanned(field_ident, reason));
                        continue;
                    }
                    if let Err(reason) = validate_guidance(&format!("field `{field_ident}` no"), no)
                    {
                        errors.push(syn::Error::new_spanned(field_ident, reason));
                        continue;
                    }
                }
                (None, None) => {}
            }
        } else if attrs.yes.is_some() || attrs.no.is_some() {
            errors.push(syn::Error::new_spanned(
                field_ident,
                "`#[decision(yes = ..., no = ...)]` only applies to `Predicate` fields",
            ));
            continue;
        }

        resolved.push(ResolvedField {
            field_ident,
            key,
            instructions,
            yes: attrs.yes.clone(),
            no: attrs.no.clone(),
            kind,
            outcome: field_outcome,
        });
    }

    if fields.named.is_empty() || fields.named.len() > 64 {
        errors.push(syn::Error::new_spanned(
            &input.ident,
            "`Decision` requires 1..=64 fields (one question per field)",
        ));
    }

    if let Some(combined) = errors.into_iter().reduce(|mut acc, e| {
        acc.combine(e);
        acc
    }) {
        return combined.to_compile_error();
    }

    generate_decision_impl(&input.ident, &resolved)
}

/// If `ty` is `Outcome<Inner>`, returns `(Inner, true)`; otherwise `(ty, false)`.
fn unwrap_outcome(ty: &Type) -> (&Type, bool) {
    if let Type::Path(type_path) = ty
        && let Some(segment) = type_path.path.segments.last()
        && segment.ident == "Outcome"
        && let PathArguments::AngleBracketed(args) = &segment.arguments
        && let Some(GenericArgument::Type(inner)) = args.args.first()
    {
        return (inner, true);
    }
    (ty, false)
}

/// Classifies `ty` as `Predicate`, `Choice<T>`, or `Score<T>`, extracting `T` for the
/// latter two.
fn classify_field_type(ty: &Type) -> Option<FieldKind<'_>> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    match segment.ident.to_string().as_str() {
        "Predicate" => Some(FieldKind::Predicate),
        "Choice" => generic_arg(segment).map(FieldKind::Choice),
        "Score" => generic_arg(segment).map(FieldKind::Score),
        _ => None,
    }
}

/// Extracts `T` from `Choice<T>` / `Score<T>`'s single generic argument.
fn generic_arg(segment: &syn::PathSegment) -> Option<&Type> {
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    match args.args.first()? {
        GenericArgument::Type(t) => Some(t),
        _ => None,
    }
}

/// Validates a question key against `decision_rs::question`'s runtime charset/length bound.
fn validate_key(key: &str) -> Result<(), String> {
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

/// Validates guidance text against `decision_rs::question`'s runtime bound.
fn validate_guidance(field: &str, text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if text.len() > MAX_GUIDANCE_BYTES {
        return Err(format!(
            "{field} length must be <= {MAX_GUIDANCE_BYTES} bytes, got {}",
            text.len()
        ));
    }
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(format!("{field} contains abnormal control characters"));
    }
    Ok(())
}

/// Generate a safely cached schema and checked typed decoder.
fn generate_decision_impl(struct_ident: &Ident, fields: &[ResolvedField<'_>]) -> TokenStream {
    let additions = fields.iter().map(build_question_entry);
    let required = fields.iter().filter(|f| !f.outcome).map(|f| &f.key);
    let decoded = fields.iter().map(|f| {
        let ident = f.field_ident;
        let key = &f.key;
        quote! { let #ident = answers.decode(#key)?; }
    });
    let idents = fields.iter().map(|f| f.field_ident);
    quote! {
        impl ::decision_rs::Decision for #struct_ident {
            fn questions() -> ::std::result::Result<&'static ::decision_rs::Questions, ::decision_rs::Error> {
                static QUESTIONS: ::std::sync::LazyLock<::std::result::Result<::decision_rs::Questions, ::decision_rs::SchemaError>> =
                    ::std::sync::LazyLock::new(|| ::decision_rs::Questions::builder() #(#additions)* .build());
                match &*QUESTIONS {
                    ::std::result::Result::Ok(questions) => ::std::result::Result::Ok(questions),
                    ::std::result::Result::Err(error) => ::std::result::Result::Err(error.clone().into()),
                }
            }
            fn from_answers(answers: &::decision_rs::Answers) -> ::std::result::Result<Self, ::decision_rs::DecodeError> {
                answers.require(&[#(#required),*])?;
                #(#decoded)*
                ::std::result::Result::Ok(Self { #(#idents),* })
            }
        }
    }
}
fn build_question_entry(field: &ResolvedField<'_>) -> TokenStream {
    let key = &field.key;
    let instructions = &field.instructions;
    match &field.kind {
        FieldKind::Predicate => {
            if let (Some(yes), Some(no)) = (&field.yes, &field.no) {
                quote! { .predicate_with(#key, #instructions, #yes, #no) }
            } else {
                quote! { .predicate(#key, #instructions) }
            }
        }
        FieldKind::Choice(ty) => {
            quote! { .choice(#key, #instructions, <#ty as ::decision_rs::ChoiceOptions>::options().iter().copied()) }
        }
        FieldKind::Score(ty) => {
            quote! { .score(#key, #instructions, <#ty as ::decision_rs::ScoreLevels>::levels().iter().map(|level| (level.key, level.description))) }
        }
    }
}
