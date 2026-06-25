use super::{cap_command_output, MAX_COMMAND_OUTPUT_BYTES};
use super::{StartAgentResult, StartAgentVersion};

#[test]
fn deserializes_legacy_start_agent_success_without_version_as_v1() {
    let result: StartAgentResult =
        serde_json::from_value(serde_json::json!({ "Success": { "agent_id": "agent-1" } }))
            .expect("legacy start-agent success should deserialize");

    assert_eq!(
        result,
        StartAgentResult::Success {
            agent_id: "agent-1".to_string(),
            version: StartAgentVersion::V1,
        }
    );
}

#[test]
fn deserializes_legacy_start_agent_error_without_version_as_v1() {
    let result: StartAgentResult =
        serde_json::from_value(serde_json::json!({ "Error": { "error": "boom" } }))
            .expect("legacy start-agent error should deserialize");

    assert_eq!(
        result,
        StartAgentResult::Error {
            error: "boom".to_string(),
            version: StartAgentVersion::V1,
        }
    );
}

#[test]
fn deserializes_legacy_start_agent_cancelled_without_version_as_v1() {
    let result: StartAgentResult = serde_json::from_value(serde_json::json!({ "Cancelled": {} }))
        .expect("legacy start-agent cancellation should deserialize");

    assert_eq!(
        result,
        StartAgentResult::Cancelled {
            version: StartAgentVersion::V1,
        }
    );
}

#[test]
fn cap_command_output_passes_through_when_under_budget() {
    let small = "hello world\n".repeat(10);
    assert!(small.len() <= MAX_COMMAND_OUTPUT_BYTES);
    assert_eq!(cap_command_output(small.clone()), small);
}

#[test]
fn cap_command_output_truncates_when_over_budget() {
    let big = "A".repeat(MAX_COMMAND_OUTPUT_BYTES * 2);
    let capped = cap_command_output(big.clone());
    // Truncated output is smaller than the original and carries the marker.
    assert!(capped.len() < big.len());
    assert!(capped.contains("bytes truncated"));
    // Head and tail of the original content are preserved around the marker.
    assert!(capped.starts_with('A'));
    assert!(capped.ends_with('A'));
    // The retained payload stays within the budget (plus the small marker text).
    assert!(capped.len() <= MAX_COMMAND_OUTPUT_BYTES + 64);
}

#[test]
fn cap_command_output_is_char_boundary_safe_on_multibyte_input() {
    // Each '€' is 3 bytes; build an output well over the budget.
    let big: String = "€".repeat(MAX_COMMAND_OUTPUT_BYTES); // 3x the byte budget
    let capped = cap_command_output(big);
    // Must not panic and must remain valid UTF-8 (guaranteed by String), and
    // every retained char must be a complete '€'.
    let stripped: String = capped.chars().filter(|c| *c == '€').collect();
    assert!(!stripped.is_empty());
    assert!(capped.contains("bytes truncated"));
}
