//! `#[derive(Decision)]`: the struct derive that turns doc comments into `instructions`
//! / criteria and generates `impl Decision`.

use darling::FromField;
use heck::ToSnakeCase;
use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Fields, GenericArgument, PathArguments, Type};

use crate::docs::doc_comment;

/// Mirrors `jev::question`'s runtime bounds; see `crate::options` for the full
/// rationale (kept in sync by `tests/derive.rs`).
const MAX_GUIDANCE_BYTES: usize = 4096;
const MAX_KEY_BYTES: usize = 64;

/// Per-field `#[jev(...)]` attributes.
#[derive(Debug, Default, FromField)]
#[darling(attributes(jev), default)]
struct FieldAttrs {
    key: Option<String>,
    instructions: Option<String>,
    yes: Option<String>,
    no: Option<String>,
    optional: bool,
}

/// The primitive a field maps to, after unwrapping an outer `Option<_>`.
enum FieldKind<'a> {
    Noul,
    Choice(&'a Type),
    Score(&'a Type),
}

/// One resolved field: its wire key, instructions, primitive, and whether a missing
/// answer should be tolerated (`#[jev(optional)]`).
struct ResolvedField<'a> {
    field_ident: &'a Ident,
    key: String,
    instructions: String,
    yes: Option<String>,
    no: Option<String>,
    kind: FieldKind<'a>,
    optional: bool,
}

/// Derives `impl Decision` for a struct whose fields are `Noul`, `Choice<T>`,
/// `Score<T>`, or `Option<_>` of those.
///
/// # Clippy
///
/// `too_many_lines`: this is one linear validation pass over a struct's fields
/// (key, instructions, criteria, shape), each step independently short; splitting it
/// would scatter one cohesive algorithm across several functions for no readability
/// gain.
#[allow(clippy::too_many_lines)]
pub(crate) fn expand_decision(input: &DeriveInput) -> TokenStream {
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

        let (inner_type, field_optional) = unwrap_option(&field.ty);
        let Some(kind) = classify_field_type(inner_type) else {
            errors.push(syn::Error::new_spanned(
                &field.ty,
                "unsupported `Decision` field type; expected `Noul`, `Choice<T>`, `Score<T>`, or \
                 `Option<_>` of one of those",
            ));
            continue;
        };

        if field_optional && !attrs.optional {
            errors.push(syn::Error::new_spanned(
                field_ident,
                "an `Option<_>` field requires `#[jev(optional)]`, so a missing answer is a \
                 deliberate, documented choice rather than an accident",
            ));
            continue;
        }
        if attrs.optional && !field_optional {
            errors.push(syn::Error::new_spanned(
                field_ident,
                "`#[jev(optional)]` requires the field type to be `Option<_>`",
            ));
            continue;
        }

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
                "field has no doc comment and no `#[jev(instructions = \"...\")]`; an \
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

        if matches!(kind, FieldKind::Noul) {
            match (&attrs.yes, &attrs.no) {
                (Some(_), None) | (None, Some(_)) => {
                    errors.push(syn::Error::new_spanned(
                        field_ident,
                        "`#[jev(yes = ...)]` and `#[jev(no = ...)]` must both be present or both \
                         absent",
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
                "`#[jev(yes = ..., no = ...)]` only applies to `Noul` fields",
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
            optional: field_optional,
        });
    }

    if fields.named.is_empty() {
        errors.push(syn::Error::new_spanned(
            &input.ident,
            "`Decision` requires at least one field (one question)",
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

/// If `ty` is `Option<Inner>`, returns `(Inner, true)`; otherwise `(ty, false)`.
fn unwrap_option(ty: &Type) -> (&Type, bool) {
    if let Type::Path(type_path) = ty
        && let Some(segment) = type_path.path.segments.last()
        && segment.ident == "Option"
        && let PathArguments::AngleBracketed(args) = &segment.arguments
        && let Some(GenericArgument::Type(inner)) = args.args.first()
    {
        return (inner, true);
    }
    (ty, false)
}

/// Classifies `ty` as `Noul`, `Choice<T>`, or `Score<T>`, extracting `T` for the
/// latter two.
fn classify_field_type(ty: &Type) -> Option<FieldKind<'_>> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    match segment.ident.to_string().as_str() {
        "Noul" => Some(FieldKind::Noul),
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

/// Validates a question key against `jev::question`'s runtime charset/length bound.
fn validate_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.len() > MAX_KEY_BYTES {
        return Err(format!(
            "key length must be 1..={MAX_KEY_BYTES} bytes, got {}",
            key.len()
        ));
    }
    let valid = key
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if !valid {
        return Err(format!(
            "key `{key}` must match `^[a-z0-9_]{{1,{MAX_KEY_BYTES}}}$`"
        ));
    }
    Ok(())
}

/// Validates guidance text against `jev::question`'s runtime bound.
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
    Ok(())
}

/// Generates the `impl Decision` block: a `LazyLock<Questions>` and `from_answers`.
fn generate_decision_impl(struct_ident: &Ident, fields: &[ResolvedField<'_>]) -> TokenStream {
    let question_entries = fields.iter().map(build_question_entry);

    let from_answers_fields = fields.iter().map(build_from_answers_field);
    let field_idents: Vec<&Ident> = fields.iter().map(|f| f.field_ident).collect();

    quote! {
        impl ::jev::Decision for #struct_ident {
            fn questions() -> &'static ::jev::Questions {
                static QUESTIONS: ::std::sync::LazyLock<::jev::Questions> = ::std::sync::LazyLock::new(|| {
                    ::jev::Questions::from_validated_entries(::std::vec![
                        #( #question_entries ),*
                    ])
                });
                &QUESTIONS
            }

            fn from_answers(answers: &::jev::Answers) -> ::std::result::Result<Self, ::jev::Error> {
                #( #from_answers_fields )*
                ::std::result::Result::Ok(#struct_ident {
                    #( #field_idents ),*
                })
            }
        }
    }
}

/// Builds one `(key, Question)` tuple literal for the `questions()` initializer.
fn build_question_entry(field: &ResolvedField<'_>) -> TokenStream {
    let key = &field.key;
    let instructions = &field.instructions;

    match &field.kind {
        FieldKind::Noul => {
            let criteria = match (&field.yes, &field.no) {
                (Some(yes), Some(no)) => quote! {
                    ::std::option::Option::Some(::jev::NoulCriteria {
                        r#true: ::jev::Guidance::from(#yes),
                        r#false: ::jev::Guidance::from(#no),
                    })
                },
                _ => quote! { ::std::option::Option::None },
            };
            quote! {
                (
                    #key.to_string(),
                    ::jev::Question::Noul {
                        instructions: ::jev::Guidance::from(#instructions),
                        criteria: #criteria,
                    },
                )
            }
        }
        FieldKind::Choice(ty) => quote! {
            (
                #key.to_string(),
                ::jev::Question::Choice {
                    instructions: ::jev::Guidance::from(#instructions),
                    criteria: <#ty as ::jev::ChoiceOptions>::options()
                        .iter()
                        .map(|(k, v)| ((*k).to_string(), ::jev::Guidance::from(*v)))
                        .collect(),
                },
            )
        },
        FieldKind::Score(ty) => quote! {
            (
                #key.to_string(),
                ::jev::Question::Score {
                    instructions: ::jev::Guidance::from(#instructions),
                    criteria: <#ty as ::jev::ScoreLevels>::levels()
                        .iter()
                        .map(|v| ::jev::Guidance::from(*v))
                        .collect(),
                },
            )
        },
    }
}

/// Builds the `let #field_ident = ...;` statement reading one answer in
/// `from_answers`.
fn build_from_answers_field(field: &ResolvedField<'_>) -> TokenStream {
    let ident = field.field_ident;
    let key = &field.key;

    let build_value = match &field.kind {
        FieldKind::Noul => quote! { ::jev::Noul::try_new(answers.noul(#key)?)? },
        FieldKind::Choice(ty) => quote! {
            {
                let __answer = answers.choice(#key)?;
                ::jev::Choice::<#ty>::try_new(
                    #key,
                    &__answer.choice,
                    __answer.confidence,
                    __answer.probabilities.clone(),
                )?
            }
        },
        FieldKind::Score(ty) => quote! {
            {
                let __answer = answers.score(#key)?;
                ::jev::Score::<#ty>::try_new(
                    #key,
                    __answer.score,
                    __answer.confidence,
                    __answer.probabilities.clone(),
                )?
            }
        },
    };

    if field.optional {
        quote! {
            let #ident = match answers.get(#key) {
                ::std::option::Option::None => ::std::option::Option::None,
                ::std::option::Option::Some(_) => ::std::option::Option::Some(#build_value),
            };
        }
    } else {
        quote! {
            let #ident = #build_value;
        }
    }
}
