//! Versioned case/config documents share domain validation.

use jev::CaseSet;
#[cfg(feature = "yaml")]
use jev::{Config, Provider};
use serde_json::json;

fn document() -> serde_json::Value {
    json!({"version":2,"cases":[{"id":"ticket-1","input":{"z":2,"a":1},"questions":{
        "refund":{"type":"predicate","instructions":"Refund?","criteria":{"yes":"Money back.","no":"No money back."}},
        "team":{"type":"choice","instructions":"Team?","options":[{"key":"billing","description":"Payments."},{"key":"other","description":"Other."}]},
        "urgency":{"type":"score","instructions":"Urgency?","levels":[{"key":"low","description":"Low."},{"key":"high","description":"High."}]}
    }}]})
}
#[test]
fn test_should_load_canonical_version_two_cases() {
    let cases = CaseSet::from_json_str(&document().to_string()).unwrap();
    let case = cases.iter().next().unwrap();
    assert_eq!(case.id(), "ticket-1");
    assert_eq!(case.input().as_str(), r#"{"a":1,"z":2}"#);
    assert_eq!(
        case.questions()
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>(),
        ["refund", "team", "urgency"]
    );
}
#[test]
fn test_should_reject_old_version_and_unknown_fields() {
    let mut body = document();
    body["version"] = json!(1);
    assert!(CaseSet::from_json_str(&body.to_string()).is_err());
    body["version"] = json!(2);
    body["cases"][0]["state"] = json!("old");
    assert!(CaseSet::from_json_str(&body.to_string()).is_err());
    let mut body = document();
    body["cases"][0]["questions"]["team"]["criteria"] = json!({});
    assert!(CaseSet::from_json_str(&body.to_string()).is_err());
    let mut body = document();
    let duplicate = body["cases"][0].clone();
    body["cases"].as_array_mut().unwrap().push(duplicate);
    assert!(CaseSet::from_json_str(&body.to_string()).is_err());
    assert!(CaseSet::from_json_str(r#"{"version":2,"version":2,"cases":[]}"#).is_err());
}
#[cfg(feature = "yaml")]
#[test]
fn test_should_load_yaml_equivalently_and_validate_config() {
    let text =
        "version: 2\ncases:\n  - id: ticket-1\n    input: {z: 2, a: 1}\n    questions:\n      \
         refund:\n        type: predicate\n        instructions: Refund?\n        criteria: {yes: \
         Money back., no: No money back.}\n      team:\n        type: choice\n        \
         instructions: Team?\n        options:\n          - {key: billing, description: \
         Payments.}\n          - {key: other, description: Other.}\n      urgency:\n        type: \
         score\n        instructions: Urgency?\n        levels:\n          - {key: low, \
         description: Low.}\n          - {key: high, description: High.}\n";
    let yaml = CaseSet::from_yaml_str(text).unwrap();
    let json = CaseSet::from_json_str(&document().to_string()).unwrap();
    assert_eq!(
        yaml.iter().next().unwrap().input(),
        json.iter().next().unwrap().input()
    );
    assert_eq!(
        yaml.iter().next().unwrap().questions(),
        json.iter().next().unwrap().questions()
    );
    let config = Config::from_yaml_str(
        "provider: openai\nmodel: default\ntimeoutMs: 10000\nthresholds: {route: 0.9}\n",
    )
    .unwrap();
    assert_eq!(config.provider, Provider::OpenAi);
    assert_eq!(
        config.settings.thresholds.require("route").unwrap().get(),
        0.9
    );
    assert!(Config::from_yaml_str("provider: openai\nunknown: true").is_err());
    assert!(Config::from_yaml_str("provider: openai\ntimeoutMs: 0").is_err());
    assert!(Config::from_yaml_str("provider: openai\nthresholds: {route: 1.1}").is_err());
    assert!(Config::from_yaml_str("provider: openai\nprovider: typesafe").is_err());
}
#[cfg(feature = "yaml")]
#[test]
fn test_should_reject_yaml_aliases_and_excessive_nesting() {
    for text in [
        "version: 2\ncases: &a []\nother: *a",
        "version: 2\ncases: []\n<<: {}",
        "version: 2\ncases: []\n---\nversion: 2",
    ] {
        assert!(CaseSet::from_yaml_str(text).is_err());
    }
    let text = format!("version: 2\ncases: {}0{}", "[".repeat(45), "]".repeat(45));
    assert!(CaseSet::from_yaml_str(&text).is_err());
}
#[tokio::test]
async fn test_should_report_case_file_io_and_format_errors() {
    assert!(
        CaseSet::from_file("/nonexistent-jev-case-file.json")
            .await
            .is_err()
    );
}
