# Version 2 cases

A case file contains provider-neutral evidence and questions. Reuse the same file
with any client; there is no provider-specific state or response format.

```yaml
version: 2
cases:
  - id: ticket-001
    input:
      subject: Duplicate payment
      body: Please refund the second charge.
    questions:
      refund:
        type: predicate
        instructions: Does the customer request a refund?
        criteria:
          yes: Explicitly asks for money back.
          no: Asks a question without requesting money back.
      team:
        type: choice
        instructions: Which team should help?
        options:
          - key: billing
            description: Payments and refunds.
          - key: other
            description: Other requests.
      urgency:
        type: score
        instructions: How urgent is the request?
        levels:
          - key: low
            description: Can wait for ordinary support.
          - key: high
            description: Immediate help is required.
```

The equivalent JSON has the same fields. `input` accepts nonblank text or an
object/array canonicalized by `Input::json`. Predicate criteria are optional but
must include both yes and no. Choice and score use stable key/description pairs.
Question declaration order and level order are retained; JSON evidence object keys
are sorted independently. Case IDs are unique ASCII letters/digits/underscore/hyphen,
1–256 bytes. Optional names are nonblank natural-language text up to 256 bytes.

`CaseSet::from_json_str` is always available. The `yaml` feature adds
`from_yaml_str`; `from_file` selects format by `.json`, `.yaml` or `.yml` and bounds
reads to 16 MiB with a ten-second deadline. Files require `version: 2` and 1–10000
cases. Duplicate/unknown keys, YAML aliases/anchors/merge keys and old versions are
rejected. Parser budgets bound the envelope to 40 levels and one million nodes;
each evidence object separately obeys 32 levels and 10000 nodes.

```rust,no_run
# #[cfg(all(feature = "batch", feature = "yaml"))]
# async fn run() -> Result<(), decision_rs::Error> {
let cases = decision_rs::CaseSet::from_file("cases.yaml").await?;
let client = decision_rs::Client::from_env()?;
for result in decision_rs::run_cases(&client, &cases, 4).await? {
    let completed = result?;
    tracing::info!(id = %completed.id, tokens = completed.response.meta.usage.input_tokens, "case completed");
}
# Ok(())
# }
```

`Client::run_case` uses the ordinary request path. Batch helpers validate size and
concurrency (1–128), preserve input order and return individual errors. Cancellation
drops unfinished futures without detached background requests. `decide_all` takes
prepared `Input` values, avoiding repeated serialization of shared evidence.
