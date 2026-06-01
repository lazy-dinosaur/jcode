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
            {"tool": "read", "parameter": {"file_path": "singular.rs"}},
            {"tool": "read", "arguments": {"file_path": "a.rs"}},
            {"tool": "read", "args": {"file_path": "b.rs"}},
            {"tool": "read", "input": {"file_path": "c.rs"}}
        ]
    });

    let normalized = normalize_batch_input(input);
    let parsed: BatchInput = serde_json::from_value(normalized).unwrap();

    assert_eq!(parsed.tool_calls.len(), 4);
    assert_eq!(
        parsed.tool_calls[0].parameters.as_ref().unwrap()["file_path"],
        "singular.rs"
    );
    assert_eq!(
        parsed.tool_calls[1].parameters.as_ref().unwrap()["file_path"],
        "a.rs"
    );
    assert_eq!(
        parsed.tool_calls[2].parameters.as_ref().unwrap()["file_path"],
        "b.rs"
    );
    assert_eq!(
        parsed.tool_calls[3].parameters.as_ref().unwrap()["file_path"],
        "c.rs"
    );
}

#[test]
fn test_normalize_singular_parameter_alias_for_real_failed_shapes() {
    let input = json!({
        "tool_calls": [
            {"tool": "agentgrep", "parameter": {"mode": "grep", "path": ".lazy-harness", "query": "휴가|vacation", "max_regions": 20}},
            {"tool": "Bash", "parameter": {"command": "git diff --stat"}},
            {"tool": "Read", "parameter": {"file_path": ".lazy-harness/ssot/calendar-permission-policy.md"}}
        ]
    });

    let normalized = normalize_batch_input(input);
    let parsed: BatchInput = serde_json::from_value(normalized).unwrap();
    let calls: Vec<(String, Value)> = parsed
        .tool_calls
        .into_iter()
        .map(|call| call.resolved_parameters())
        .collect();

    assert_eq!(calls[0].0, "agentgrep");
    assert_eq!(calls[0].1["query"], "휴가|vacation");
    assert_eq!(calls[0].1["mode"], "grep");
    assert_eq!(calls[1].0, "bash");
    assert_eq!(calls[1].1["command"], "git diff --stat");
    assert_eq!(calls[2].0, "read");
    assert_eq!(
        calls[2].1["file_path"],
        ".lazy-harness/ssot/calendar-permission-policy.md"
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
fn test_resolved_parameters_accepts_claude_style_pascal_case_tool_names() {
    let input = json!({
        "tool_calls": [
            {"tool": "Bash", "parameters": {"command": "pwd"}},
            {"tool": "Read", "parameters": {"file_path": "Cargo.toml"}},
            {"tool": "ToolSearch", "parameters": {"query": "tokio::spawn"}}
        ]
    });

    let normalized = normalize_batch_input(input);
    let parsed: BatchInput = serde_json::from_value(normalized).unwrap();
    let calls: Vec<(String, Value)> = parsed
        .tool_calls
        .into_iter()
        .map(|call| call.resolved_parameters())
        .collect();

    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "pwd");
    assert_eq!(calls[1].0, "read");
    assert_eq!(calls[1].1["file_path"], "Cargo.toml");
    assert_eq!(calls[2].0, "codesearch");
    assert_eq!(calls[2].1["query"], "tokio::spawn");
}

#[test]
fn test_normalize_merges_sibling_args_into_existing_parameters() {
    let input = json!({
        "tool_calls": [
            {"tool": "bash", "parameters": {}, "command": "pwd"},
            {"tool": "agentgrep", "parameters": {"mode": "grep"}, "query": "RoomCell"}
        ]
    });

    let normalized = normalize_batch_input(input);
    let parsed: BatchInput = serde_json::from_value(normalized).unwrap();
    let calls: Vec<(String, Value)> = parsed
        .tool_calls
        .into_iter()
        .map(|call| call.resolved_parameters())
        .collect();

    assert_eq!(calls[0].0, "bash");
    assert_eq!(calls[0].1["command"], "pwd");
    assert_eq!(calls[1].0, "agentgrep");
    assert_eq!(calls[1].1["mode"], "grep");
    assert_eq!(calls[1].1["query"], "RoomCell");
}

#[test]
fn test_normalize_accepts_object_valued_tool_names() {
    let input = json!({
        "tool_calls": [
            {"tool": {"name": "bash"}, "parameters": {"command": "pwd"}},
            {"tool": {"recipient_name": "agentgrep"}, "parameters": {"mode": "grep", "query": "MemoCard"}}
        ]
    });

    let normalized = normalize_batch_input(input);
    let parsed: BatchInput = serde_json::from_value(normalized).unwrap();
    let names: Vec<String> = parsed
        .tool_calls
        .into_iter()
        .map(|call| call.resolved_parameters().0)
        .collect();

    assert_eq!(names, vec!["bash", "agentgrep"]);
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
fn test_reject_stateful_parallel_subcalls_blocks_cwd_set() {
    let subcalls = vec![
        (
            0,
            "cwd".to_string(),
            json!({"action": "set", "path": "/tmp"}),
        ),
        (1, "bash".to_string(), json!({"command": "pwd"})),
    ];

    let err = reject_stateful_parallel_subcalls(&subcalls).unwrap_err();
    let message = err.to_string();
    assert!(message.contains("Cannot run cwd action='set' inside batch"));
    assert!(message.contains("item 1"));
}

#[test]
fn test_reject_stateful_parallel_subcalls_blocks_cwd_path_default_set() {
    let subcalls = vec![(0, "cwd".to_string(), json!({"path": "/tmp"}))];

    let err = reject_stateful_parallel_subcalls(&subcalls).unwrap_err();
    assert!(err.to_string().contains("action='set'"));
}

#[test]
fn test_reject_stateful_parallel_subcalls_allows_cwd_show() {
    let subcalls = vec![
        (0, "cwd".to_string(), json!({"action": "show"})),
        (1, "bash".to_string(), json!({"command": "pwd"})),
    ];

    reject_stateful_parallel_subcalls(&subcalls).unwrap();
}

#[test]
fn test_reject_parallel_scratch_read_after_bash_blocks_tmp_read_race() {
    let subcalls = vec![
        (
            0,
            "bash".to_string(),
            json!({"command": "rg foo src > /tmp/gl.txt"}),
        ),
        (1, "read".to_string(), json!({"file_path": "/tmp/gl.txt"})),
    ];

    let err = reject_parallel_scratch_read_after_bash(&subcalls).unwrap_err();
    let message = err.to_string();
    assert!(message.contains("Cannot read scratch file '/tmp/gl.txt'"));
    assert!(message.contains("batch subcalls run in parallel"));
    assert!(message.contains("bash item 1"));
    assert!(message.contains("item 2"));
}

#[test]
fn test_reject_parallel_scratch_read_after_bash_blocks_mcp_tmp_read_race() {
    let subcalls = vec![
        (
            0,
            "Bash".to_string(),
            json!({"command": "printf ok > /var/tmp/output.txt"}),
        ),
        (
            1,
            "mcp__filesystem__read_text_file".to_string(),
            json!({"path": "/var/tmp/output.txt"}),
        ),
    ];

    let err = reject_parallel_scratch_read_after_bash(&subcalls).unwrap_err();
    assert!(err.to_string().contains("/var/tmp/output.txt"));
}

#[test]
fn test_reject_parallel_scratch_read_after_bash_allows_independent_reads() {
    let subcalls = vec![
        (
            0,
            "bash".to_string(),
            json!({"command": "printf ok > /tmp/other.txt"}),
        ),
        (1, "read".to_string(), json!({"file_path": "src/lib.rs"})),
    ];

    reject_parallel_scratch_read_after_bash(&subcalls).unwrap();
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
    assert_eq!(
        schema["properties"]["tool_calls"]["items"]["properties"]["parameters"]["type"],
        json!("object")
    );
    assert_eq!(
        schema["properties"]["tool_calls"]["items"]["properties"]["parameters"]["additionalProperties"],
        json!(true)
    );
}

#[test]
fn test_schema_keeps_flat_generic_subcall_shape() {
    let schema = generic_batch_schema();

    assert!(schema["properties"]["tool_calls"]["description"].is_null());
    assert!(schema["properties"]["tool_calls"]["items"]["description"].is_null());
    let props = schema["properties"]["tool_calls"]["items"]["properties"]
        .as_object()
        .expect("batch subcall properties should be an object");
    assert!(props.contains_key("tool"));
    assert!(props.contains_key("parameters"));
    assert!(schema["properties"]["tool_calls"]["items"]["oneOf"].is_null());
}
