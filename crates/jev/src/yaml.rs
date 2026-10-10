//! Strict YAML with parser-level budgets, no aliases, merge keys or includes.

use serde::de::DeserializeOwned;
use serde_saphyr::{
    from_str_with_options,
    options::{DuplicateKeyPolicy, MergeKeyPolicy},
};

use crate::Error;

pub(crate) fn parse<T: DeserializeOwned>(text: &str, cap: usize) -> Result<T, Error> {
    if text.len() > cap {
        return Err(Error::InvalidInput("YAML byte limit"));
    }
    let options = serde_saphyr::options! {
        budget: serde_saphyr::budget! {
            max_documents: 1, max_depth: 40, flow_nesting_limit: 40,
            max_nodes: 1_000_000, max_events: 2_000_000,
            max_total_scalar_bytes: cap, max_total_comment_bytes: cap,
            max_aliases: 0, max_anchors: 0, max_merge_keys: 0,
            max_inclusion_depth: 0,
        },
        duplicate_keys: DuplicateKeyPolicy::Error, merge_keys: MergeKeyPolicy::Error,
        strict_booleans: true, reject_unsupported_tags: true, with_snippet: false,
    };
    from_str_with_options(text, options).map_err(|_| Error::InvalidInput("invalid YAML document"))
}
