use super::*;
use serde_json::json;

#[test]
fn test_normalize_flat_params() {
    let input = json!({
        "tool_calls": [
            {"tool": "read", "file_path": "file1.txt"},
            {"tool": "read", "file_path": "file2.txt"}
        ]
    });

    let normalized = normalize_batch_input(input);
    let parsed: BatchInput = serde_json::from_value(normalized).unwrap();
    assert_eq!(parsed.tool_calls.len(), 2);
    assert_eq!(parsed.tool_calls[0].tool, "read");
    let params = parsed.tool_calls[0].parameters.as_ref().unwrap();
    assert_eq!(params["file_path"], "file1.txt");
}

#[test]
fn test_normalize_already_nested() {
    let input = json!({
        "tool_calls": [
            {"tool": "read", "parameters": {"file_path": "file1.txt"}}
        ]
    });

    let normalized = normalize_batch_input(input);
    let parsed: BatchInput = serde_json::from_value(normalized).unwrap();
    assert_eq!(parsed.tool_calls.len(), 1);
    let params = parsed.tool_calls[0].parameters.as_ref().unwrap();
    assert_eq!(params["file_path"], "file1.txt");
}

#[test]
fn test_normalize_name_key_to_tool() {
    let input = json!({
        "tool_calls": [
            {"name": "read", "parameters": {"file_path": "file1.txt"}},
            {"name": "grep", "pattern": "foo", "path": "src/"}
        ]
    });

    let normalized = normalize_batch_input(input);
    let parsed: BatchInput = serde_json::from_value(normalized).unwrap();
    assert_eq!(parsed.tool_calls.len(), 2);
    assert_eq!(parsed.tool_calls[0].tool, "read");
    let params0 = parsed.tool_calls[0].parameters.as_ref().unwrap();
    assert_eq!(params0["file_path"], "file1.txt");
    assert_eq!(parsed.tool_calls[1].tool, "grep");
    let params1 = parsed.tool_calls[1].parameters.as_ref().unwrap();
    assert_eq!(params1["pattern"], "foo");
}

#[test]
fn test_normalize_mixed_tool_and_name_keys() {
    let input = json!({
        "tool_calls": [
            {"tool": "read", "parameters": {"file_path": "a.rs"}},
            {"name": "read", "parameters": {"file_path": "b.rs"}},
            {"tool": "grep", "pattern": "test"}
        ]
    });

    let normalized = normalize_batch_input(input);
    let parsed: BatchInput = serde_json::from_value(normalized).unwrap();
    assert_eq!(parsed.tool_calls.len(), 3);
    assert_eq!(parsed.tool_calls[0].tool, "read");
    assert_eq!(parsed.tool_calls[1].tool, "read");
    assert_eq!(parsed.tool_calls[2].tool, "grep");
}

#[test]
fn test_normalize_arguments_aliases_to_parameters() {
    let input = json!({
        "tool_calls": [
            {"tool": "read", "arguments": {"file_path": "a.rs"}},
            {"tool": "read", "args": {"file_path": "b.rs"}},
            {"tool": "read", "input": {"file_path": "c.rs"}}
        ]
    });

    let normalized = normalize_batch_input(input);
    let parsed: BatchInput = serde_json::from_value(normalized).unwrap();

    assert_eq!(parsed.tool_calls.len(), 3);
    assert_eq!(
        parsed.tool_calls[0].parameters.as_ref().unwrap()["file_path"],
        "a.rs"
    );
    assert_eq!(
        parsed.tool_calls[1].parameters.as_ref().unwrap()["file_path"],
        "b.rs"
    );
    assert_eq!(
        parsed.tool_calls[2].parameters.as_ref().unwrap()["file_path"],
        "c.rs"
    );
}

#[test]
fn test_resolved_parameters_strips_default_api_tool_namespace() {
    let input = json!({
        "tool_calls": [
            {"tool": "default_api:bash", "parameters": {"command": "pwd"}},
            {"tool": "default_api:read", "parameters": {"file_path": "Cargo.toml"}},
            {"tool": "default_api:mcp__filesystem__list_directory", "parameters": {"path": "."}}
        ]
    });

    let normalized = normalize_batch_input(input);
    let parsed: BatchInput = serde_json::from_value(normalized).unwrap();
    let names: Vec<String> = parsed
        .tool_calls
        .into_iter()
        .map(|call| call.resolved_parameters().0)
        .collect();

    assert_eq!(
        names,
        vec!["bash", "read", "mcp__filesystem__list_directory"]
    );
}

#[test]
fn test_reject_duplicate_subcalls_blocks_exact_same_work() {
    let subcalls = vec![
        (0, "read".to_string(), json!({"file_path": "src/lib.rs"})),
        (1, "read".to_string(), json!({"file_path": "src/lib.rs"})),
    ];

    let err = reject_duplicate_subcalls(&subcalls).unwrap_err();
    let message = err.to_string();
    assert!(message.contains("Duplicate batch tool call"));
    assert!(message.contains("items 1 and 2"));
}

#[test]
fn test_reject_duplicate_subcalls_allows_same_tool_different_params() {
    let subcalls = vec![
        (0, "read".to_string(), json!({"file_path": "src/lib.rs"})),
        (1, "read".to_string(), json!({"file_path": "src/main.rs"})),
    ];

    reject_duplicate_subcalls(&subcalls).unwrap();
}

#[test]
fn test_duplicate_subcall_key_canonicalizes_object_order() {
    let a = duplicate_subcall_key("bash", &json!({"command": "cargo check", "timeout": 1}));
    let b = duplicate_subcall_key("bash", &json!({"timeout": 1, "command": "cargo check"}));
    assert_eq!(a, b);
}

#[test]
fn test_schema_only_requires_tool() {
    let schema = BatchTool::new(Registry {
        tools: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        skills: std::sync::Arc::new(tokio::sync::RwLock::new(
            crate::skill::SkillRegistry::default(),
        )),
        compaction: std::sync::Arc::new(tokio::sync::RwLock::new(
            crate::compaction::CompactionManager::new(),
        )),
        mcp_manager: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
    })
    .parameters_schema();

    assert_eq!(
        schema["properties"]["tool_calls"]["items"]["required"],
        json!(["tool"])
    );
    assert_eq!(
        schema["properties"]["tool_calls"]["items"]["additionalProperties"],
        json!(true)
    );
    assert_eq!(
        schema["properties"]["tool_calls"]["items"]["properties"]["tool"]["description"],
        json!("Tool name.")
    );
    assert!(schema["properties"]["tool_calls"]["items"]["properties"]["parameters"].is_null());
}

#[test]
fn test_schema_keeps_flat_generic_subcall_shape() {
    let schema = generic_batch_schema();

    assert!(schema["properties"]["tool_calls"]["description"].is_null());
    assert!(schema["properties"]["tool_calls"]["items"]["description"].is_null());
    assert_eq!(
        schema["properties"]["tool_calls"]["items"]["properties"]
            .as_object()
            .map(|props| props.len()),
        Some(1)
    );
    assert!(schema["properties"]["tool_calls"]["items"]["oneOf"].is_null());
}
