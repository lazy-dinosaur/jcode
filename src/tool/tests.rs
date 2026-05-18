use super::*;
use crate::mcp::{McpConfig, McpManager, McpServerConfig};
use crate::message::{Message, ToolDefinition};
use crate::provider::{EventStream, Provider};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::RwLock;

struct MockProvider;

#[async_trait]
impl Provider for MockProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> anyhow::Result<EventStream> {
        Err(anyhow::anyhow!(
            "Mock provider should not be used for streaming completions in tool registry tests"
        ))
    }

    fn name(&self) -> &str {
        "mock"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(MockProvider)
    }
}

#[tokio::test]
async fn test_tool_definitions_are_sorted() {
    // Create registry with mock provider
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let registry = Registry::new(provider).await;

    // Get definitions multiple times and verify they're always in the same order
    let defs1 = registry.definitions(None).await;
    let defs2 = registry.definitions(None).await;

    // Should have the same order
    assert_eq!(defs1.len(), defs2.len());
    for (d1, d2) in defs1.iter().zip(defs2.iter()) {
        assert_eq!(d1.name, d2.name);
    }

    // Verify they're sorted alphabetically
    let names: Vec<&str> = defs1.iter().map(|d| d.name.as_str()).collect();
    let mut sorted_names = names.clone();
    sorted_names.sort();
    assert_eq!(
        names, sorted_names,
        "Tool definitions should be sorted alphabetically"
    );
}

#[test]
fn test_resolve_skill_aliases_to_skill_manage() {
    assert_eq!(Registry::resolve_tool_name("skill"), "skill_manage");
    assert_eq!(Registry::resolve_tool_name("Skill"), "skill_manage");
    assert_eq!(Registry::resolve_tool_name("skill_manage"), "skill_manage");
}

struct BareSchemaTool;

#[async_trait]
impl Tool for BareSchemaTool {
    fn name(&self) -> &str {
        "bare_schema"
    }

    fn description(&self) -> &str {
        "Test tool without an explicit intent property."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "required": ["command"],
            "properties": {
                "command": {"type": "string"}
            }
        })
    }

    async fn execute(&self, _input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        Ok(ToolOutput::new("ok"))
    }
}

#[test]
fn tool_definitions_do_not_auto_inject_intent() {
    let def = BareSchemaTool.to_definition();
    assert!(def.input_schema["properties"]["intent"].is_null());
}

#[tokio::test]
async fn first_party_tool_definitions_include_optional_intent_explicitly() {
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let registry = Registry::new(provider).await;
    registry.register_ambient_tools().await;

    let defs = registry.definitions(None).await;
    assert!(!defs.is_empty());

    for def in defs {
        let schema = &def.input_schema;
        if schema["type"] != "object" {
            continue;
        }

        assert_eq!(
            schema["properties"]["intent"]["type"], "string",
            "{} should explicitly define optional intent in its schema",
            def.name
        );
        assert!(
            schema["properties"]["intent"]["description"]
                .as_str()
                .unwrap_or_default()
                .contains("display only"),
            "{} intent description should say it is display-only",
            def.name
        );
        let required = schema["required"].as_array().cloned().unwrap_or_default();
        assert!(
            !required.iter().any(|value| value == "intent"),
            "{} must not require intent",
            def.name
        );
    }
}

fn delayed_mcp_server_config(delay_seconds: f32) -> (tempfile::TempDir, McpConfig) {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let server_script = temp_dir.path().join("delayed-mcp.sh");
    std::fs::write(
        &server_script,
        format!(
            r#"#!/usr/bin/env bash
sleep {delay_seconds}
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *initialize*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":"2024-11-05","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"delayed","version":"1.0.0"}}}}}}\n' "$id"
      ;;
    *tools/list*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"tools":[{{"name":"hello","description":"Delayed hello","inputSchema":{{"type":"object","properties":{{}}}}}}]}}}}\n' "$id"
      ;;
    *tools/call*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"content":[{{"type":"text","text":"hello from delayed"}}],"isError":false}}}}\n' "$id"
      ;;
  esac
done
"#
        ),
    )
    .expect("write delayed MCP script");

    let mut servers = HashMap::new();
    servers.insert(
        "delayed".to_string(),
        McpServerConfig {
            command: "bash".to_string(),
            args: vec![server_script.to_string_lossy().to_string()],
            env: HashMap::new(),
            transport: None,
            url: None,
            headers: HashMap::new(),
            auth: None,
            shared: false,
        },
    );
    (temp_dir, McpConfig { servers })
}

fn flaky_once_mcp_server_config() -> (tempfile::TempDir, McpConfig) {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let marker = temp_dir.path().join("failed-once");
    let server_script = temp_dir.path().join("flaky-once-mcp.sh");
    std::fs::write(
        &server_script,
        r#"#!/usr/bin/env bash
marker="$MARKER"
if [ ! -f "$marker" ]; then
  touch "$marker"
  exit 42
fi
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *initialize*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"flaky","version":"1.0.0"}}}\n' "$id"
      ;;
    *tools/list*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"hello","description":"Flaky hello","inputSchema":{"type":"object","properties":{}}}]}}\n' "$id"
      ;;
  esac
done
"#,
    )
    .expect("write flaky MCP script");

    let mut servers = HashMap::new();
    let mut env = HashMap::new();
    env.insert("MARKER".to_string(), marker.to_string_lossy().to_string());
    servers.insert(
        "flaky".to_string(),
        McpServerConfig {
            command: "bash".to_string(),
            args: vec![server_script.to_string_lossy().to_string()],
            env,
            transport: None,
            url: None,
            headers: HashMap::new(),
            auth: None,
            shared: false,
        },
    );
    (temp_dir, McpConfig { servers })
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(key);
        crate::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => crate::env::set_var(self.key, value),
            None => crate::env::remove_var(self.key),
        }
    }
}

#[tokio::test]
async fn mcp_registry_diagnostics_tracks_management_and_server_tools() {
    let registry = Registry::empty();
    let diagnostics = registry.mcp_registry_diagnostics().await;
    assert!(!diagnostics.mcp_management_registered);
    assert_eq!(diagnostics.mcp_server_tool_count, 0);

    let (_temp_dir, config) = delayed_mcp_server_config(0.0);
    let manager = Arc::new(RwLock::new(McpManager::with_config(config)));
    registry
        .register_mcp_tools_from_manager(None, manager)
        .await;

    for _ in 0..20 {
        let diagnostics = registry.mcp_registry_diagnostics().await;
        if diagnostics.mcp_server_tool_count == 1 {
            assert!(diagnostics.mcp_management_registered);
            assert_eq!(
                diagnostics.mcp_server_tool_names,
                vec!["mcp__delayed__hello"]
            );
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    panic!(
        "delayed MCP fixture did not register server tool; diagnostics={:?}",
        registry.mcp_registry_diagnostics().await
    );
}

#[tokio::test]
async fn register_mcp_tools_waits_for_delayed_server_tools_within_barrier() {
    let registry = Registry::empty();
    let (_temp_dir, config) = delayed_mcp_server_config(0.4);
    let manager = Arc::new(RwLock::new(McpManager::with_config(config)));

    let started = std::time::Instant::now();
    registry
        .register_mcp_tools_from_manager(None, manager)
        .await;

    assert!(
        started.elapsed() >= std::time::Duration::from_millis(300),
        "readiness barrier should wait for delayed MCP registration instead of returning immediately"
    );
    let diagnostics = registry.mcp_registry_diagnostics().await;
    assert!(diagnostics.mcp_management_registered);
    assert_eq!(diagnostics.mcp_server_tool_count, 1);
    assert_eq!(
        diagnostics.mcp_server_tool_names,
        vec!["mcp__delayed__hello"]
    );
}

#[tokio::test]
async fn register_mcp_tools_times_out_but_continues_background_registration() {
    let registry = Registry::empty();
    let (_temp_dir, config) = delayed_mcp_server_config(0.4);
    let manager = Arc::new(RwLock::new(McpManager::with_config(config)));

    let started = std::time::Instant::now();
    registry
        .register_mcp_tools_from_manager_with_timeout(
            None,
            manager,
            std::time::Duration::from_millis(50),
        )
        .await;

    assert!(
        started.elapsed() < std::time::Duration::from_millis(300),
        "short timeout should keep reconnect readiness bounded"
    );
    let after_timeout = registry.mcp_registry_diagnostics().await;
    assert!(after_timeout.mcp_management_registered);
    assert_eq!(after_timeout.mcp_server_tool_count, 0);

    for _ in 0..30 {
        let diagnostics = registry.mcp_registry_diagnostics().await;
        if diagnostics.mcp_server_tool_count == 1 {
            assert_eq!(
                diagnostics.mcp_server_tool_names,
                vec!["mcp__delayed__hello"]
            );
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    panic!(
        "background MCP registration did not complete after timeout; diagnostics={:?}",
        registry.mcp_registry_diagnostics().await
    );
}

#[tokio::test]
async fn reconcile_mcp_tools_restores_missing_registry_entries() {
    let registry = Registry::empty();
    let (_temp_dir, config) = delayed_mcp_server_config(0.0);
    let manager = Arc::new(RwLock::new(McpManager::with_config(config)));

    registry
        .register_mcp_tools_from_manager(None, Arc::clone(&manager))
        .await;
    assert_eq!(
        registry
            .mcp_registry_diagnostics()
            .await
            .mcp_server_tool_count,
        1
    );

    let removed = registry.unregister_prefix("mcp__delayed__hello").await;
    assert_eq!(removed, vec!["mcp__delayed__hello"]);
    assert_eq!(
        registry
            .mcp_registry_diagnostics()
            .await
            .mcp_server_tool_count,
        0
    );

    let report = registry.reconcile_mcp_tools_from_manager(manager).await;
    assert_eq!(report.expected_mcp_server_tool_count, 1);
    assert_eq!(report.already_registered_count, 0);
    assert_eq!(report.repaired_tool_names, vec!["mcp__delayed__hello"]);
    assert_eq!(
        registry
            .mcp_registry_diagnostics()
            .await
            .mcp_server_tool_names,
        vec!["mcp__delayed__hello"]
    );
}

#[tokio::test]
async fn execute_repairs_missing_mcp_registry_entry_after_reload() {
    let registry = Registry::empty();
    let (_temp_dir, config) = delayed_mcp_server_config(0.0);
    let manager = Arc::new(RwLock::new(McpManager::with_config(config)));

    registry
        .register_mcp_tools_from_manager(None, Arc::clone(&manager))
        .await;
    assert_eq!(
        registry
            .mcp_registry_diagnostics()
            .await
            .mcp_server_tool_names,
        vec!["mcp__delayed__hello"]
    );

    let removed = registry.unregister_prefix("mcp__delayed__hello").await;
    assert_eq!(removed, vec!["mcp__delayed__hello"]);

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let ctx = ToolContext {
        session_id: "test".to_string(),
        message_id: "test".to_string(),
        tool_call_id: "test".to_string(),
        working_dir: Some(temp_dir.path().to_path_buf()),
        stdin_request_tx: None,
        graceful_shutdown_signal: None,
        turn_cancel_signal: None,
        execution_mode: ToolExecutionMode::Direct,
    };

    let output = registry
        .execute("mcp__delayed__hello", serde_json::json!({}), ctx)
        .await
        .expect("missing MCP registry entry should be repaired before execute");
    assert!(output.output.contains("hello from delayed"));
    assert_eq!(
        registry
            .mcp_registry_diagnostics()
            .await
            .mcp_server_tool_names,
        vec!["mcp__delayed__hello"]
    );
}

#[tokio::test]
async fn register_mcp_tools_retries_transient_startup_failure() {
    let _attempts = EnvVarGuard::set("JCODE_MCP_CONNECT_ATTEMPTS", "3");
    let _backoff = EnvVarGuard::set("JCODE_MCP_RETRY_BACKOFF_MS", "10");
    let _readiness = EnvVarGuard::set("JCODE_MCP_READINESS_TIMEOUT_MS", "30000");
    let registry = Registry::empty();
    let (_temp_dir, config) = flaky_once_mcp_server_config();
    let manager = Arc::new(RwLock::new(McpManager::with_config(config)));

    registry
        .register_mcp_tools_from_manager(None, manager)
        .await;

    let diagnostics = registry.mcp_registry_diagnostics().await;
    assert!(diagnostics.mcp_management_registered);
    assert_eq!(diagnostics.mcp_server_tool_names, vec!["mcp__flaky__hello"]);
}

#[test]
fn test_resolve_tool_name_oauth_aliases() {
    assert_eq!(Registry::resolve_tool_name("file_grep"), "grep");
    assert_eq!(Registry::resolve_tool_name("file_read"), "read");
    assert_eq!(Registry::resolve_tool_name("file_write"), "write");
    assert_eq!(Registry::resolve_tool_name("file_edit"), "edit");
    assert_eq!(Registry::resolve_tool_name("file_glob"), "glob");
    assert_eq!(Registry::resolve_tool_name("shell_exec"), "bash");
    assert_eq!(Registry::resolve_tool_name("task_runner"), "subagent");
    assert_eq!(Registry::resolve_tool_name("task"), "subagent");
    assert_eq!(Registry::resolve_tool_name("pwd"), "cwd");
    assert_eq!(Registry::resolve_tool_name("cd"), "cwd");
    assert_eq!(Registry::resolve_tool_name("launch"), "open");
    assert_eq!(Registry::resolve_tool_name("todo_read"), "todo");
    assert_eq!(Registry::resolve_tool_name("todo_write"), "todo");
    assert_eq!(Registry::resolve_tool_name("todoread"), "todo");
    assert_eq!(Registry::resolve_tool_name("todowrite"), "todo");
    assert_eq!(Registry::resolve_tool_name("bash"), "bash");
    assert_eq!(Registry::resolve_tool_name("grep"), "grep");
    assert_eq!(Registry::resolve_tool_name("batch"), "batch");
    assert_eq!(Registry::resolve_tool_name("memory"), "memory");
}

#[tokio::test]
async fn test_batch_resolves_oauth_names() {
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let registry = Registry::new(provider).await;
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let temp_dir_str = temp_dir.path().to_string_lossy().to_string();

    let ctx = ToolContext {
        session_id: "test".to_string(),
        message_id: "test".to_string(),
        tool_call_id: "test".to_string(),
        working_dir: Some(temp_dir.path().to_path_buf()),
        stdin_request_tx: None,
        graceful_shutdown_signal: None,
        turn_cancel_signal: None,
        execution_mode: ToolExecutionMode::Direct,
    };

    let result = registry
        .execute(
            "file_grep",
            serde_json::json!({"pattern": "nonexistent_xyz", "path": temp_dir_str}),
            ctx,
        )
        .await;
    assert!(result.is_ok(), "file_grep should resolve to grep tool");
}

#[tokio::test]
async fn test_definitions_keep_batch_schema_generic() {
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let registry = Registry::new(provider).await;

    let defs = registry.definitions(None).await;
    let batch_def = defs
        .iter()
        .find(|def| def.name == "batch")
        .expect("batch definition should exist");

    assert!(batch_def.input_schema["properties"]["tool_calls"]["items"]["oneOf"].is_null());
    assert!(
        batch_def.input_schema["properties"]["tool_calls"]["items"]["required"]
            .as_array()
            .map(|required| required.iter().any(|value| value == "tool"))
            .unwrap_or(false)
    );
    assert!(
        batch_def.input_schema["properties"]["tool_calls"]["items"]["properties"]["parameters"]
            .is_null()
    );
}

#[test]
fn resolve_tool_name_maps_communicate_to_swarm() {
    assert_eq!(Registry::resolve_tool_name("communicate"), "swarm");
    assert_eq!(Registry::resolve_tool_name("swarm_now"), "swarm");
}

#[tokio::test]
#[ignore]
async fn print_tool_definition_token_report() {
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let registry = Registry::new(provider).await;
    let mut defs = registry.definitions(None).await;
    defs.sort_by_key(|def| std::cmp::Reverse(def.prompt_token_estimate()));

    println!("name,total_tokens,description_tokens");
    for def in defs {
        println!(
            "{},{},{}",
            def.name,
            def.prompt_token_estimate(),
            def.description_token_estimate()
        );
    }
}

fn schema_type_includes(schema: &Value, expected: &str) -> bool {
    match schema.get("type") {
        Some(Value::String(value)) => value == expected,
        Some(Value::Array(values)) => values
            .iter()
            .any(|value| value.as_str().is_some_and(|value| value == expected)),
        _ => false,
    }
}

fn collect_schema_errors(schema: &Value, path: &str, errors: &mut Vec<String>) {
    match schema {
        Value::Object(map) => {
            if schema_type_includes(schema, "array") && !map.contains_key("items") {
                errors.push(format!("{path}: array schema missing items"));
            }

            for keyword in ["anyOf", "oneOf", "allOf"] {
                let Some(branches) = map.get(keyword) else {
                    continue;
                };
                let Some(branches) = branches.as_array() else {
                    errors.push(format!("{path}.{keyword}: must be an array"));
                    continue;
                };
                for (idx, branch) in branches.iter().enumerate() {
                    let branch_path = format!("{path}.{keyword}[{idx}]");
                    match branch {
                        Value::Object(branch_map) => {
                            if !branch_map.contains_key("type") {
                                errors.push(format!("{branch_path}: schema missing type"));
                            }
                        }
                        _ => errors.push(format!("{branch_path}: schema branch must be an object")),
                    }
                }
            }

            for (key, value) in map {
                collect_schema_errors(value, &format!("{path}.{key}"), errors);
            }
        }
        Value::Array(values) => {
            for (idx, value) in values.iter().enumerate() {
                collect_schema_errors(value, &format!("{path}[{idx}]"), errors);
            }
        }
        _ => {}
    }
}

#[tokio::test]
async fn test_tool_definitions_do_not_expose_invalid_array_schemas() {
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let registry = Registry::new(provider).await;

    let defs = registry.definitions(None).await;
    let mut errors = Vec::new();
    for def in &defs {
        collect_schema_errors(
            &def.input_schema,
            &format!("tool `{}`", def.name),
            &mut errors,
        );
    }

    assert!(
        errors.is_empty(),
        "tool definitions must not expose invalid schemas:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_schema_validator_rejects_any_of_branches_without_type() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "status_filter": {
                "anyOf": [
                    { "enum": ["running", "completed"] },
                    { "type": "array", "items": { "type": "string" } }
                ]
            }
        }
    });

    let mut errors = Vec::new();
    collect_schema_errors(&schema, "tool `test`", &mut errors);

    assert!(
        errors
            .iter()
            .any(|error| error.contains("status_filter.anyOf[0]: schema missing type")),
        "expected missing type error, got: {errors:?}"
    );
}

#[tokio::test]
async fn test_context_guard_small_output_passes_through() {
    let compaction = Arc::new(RwLock::new(CompactionManager::new().with_budget(200_000)));
    let registry = Registry {
        tools: Arc::new(RwLock::new(HashMap::new())),
        skills: Arc::new(RwLock::new(crate::skill::SkillRegistry::default())),
        compaction,
        mcp_manager: Arc::new(RwLock::new(None)),
    };

    let output = ToolOutput::new("small output");
    let result = registry.guard_context_overflow("test", output).await;
    assert_eq!(result.output, "small output");
}

#[tokio::test]
async fn test_context_guard_truncates_huge_single_output() {
    let compaction = Arc::new(RwLock::new(CompactionManager::new().with_budget(1000)));
    let registry = Registry {
        tools: Arc::new(RwLock::new(HashMap::new())),
        skills: Arc::new(RwLock::new(crate::skill::SkillRegistry::default())),
        compaction,
        mcp_manager: Arc::new(RwLock::new(None)),
    };

    // 30% of 1000 = 300 tokens = 1200 chars max for a single output
    // Create output that's way larger
    let big_output = "x".repeat(8000); // 2000 tokens, well over 30% of 1000
    let output = ToolOutput::new(big_output.clone());
    let result = registry.guard_context_overflow("test", output).await;
    assert!(
        result.output.len() < big_output.len(),
        "Output should be truncated"
    );
    assert!(
        result.output.contains("TRUNCATED"),
        "Should contain truncation warning"
    );
}

#[tokio::test]
async fn test_context_guard_truncates_when_context_nearly_full() {
    let compaction = Arc::new(RwLock::new(CompactionManager::new().with_budget(10_000)));
    {
        let mut mgr = compaction.write().await;
        mgr.update_observed_input_tokens(9500); // 95% full
    }
    let registry = Registry {
        tools: Arc::new(RwLock::new(HashMap::new())),
        skills: Arc::new(RwLock::new(crate::skill::SkillRegistry::default())),
        compaction,
        mcp_manager: Arc::new(RwLock::new(None)),
    };

    // Even a modest output should get truncated when context is 95% full
    let output = ToolOutput::new("x".repeat(4000)); // 1000 tokens
    let result = registry.guard_context_overflow("test", output).await;
    assert!(
        result.output.contains("TRUNCATED") || result.output.contains("CONTEXT LIMIT"),
        "Should warn about context limits when nearly full"
    );
}

#[tokio::test]
async fn test_context_guard_zero_budget_passes_through() {
    let compaction = Arc::new(RwLock::new(CompactionManager::new().with_budget(0)));
    let registry = Registry {
        tools: Arc::new(RwLock::new(HashMap::new())),
        skills: Arc::new(RwLock::new(crate::skill::SkillRegistry::default())),
        compaction,
        mcp_manager: Arc::new(RwLock::new(None)),
    };

    let output = ToolOutput::new("x".repeat(100_000));
    let result = registry.guard_context_overflow("test", output).await;
    assert_eq!(
        result.output.len(),
        100_000,
        "Zero budget should pass through"
    );
}

#[tokio::test]
async fn test_request_permission_is_ambient_only() {
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let registry = Registry::new(provider).await;

    let defs = registry.definitions(None).await;
    assert!(
        !defs.iter().any(|d| d.name == "request_permission"),
        "request_permission should not be available in normal sessions"
    );

    registry.register_ambient_tools().await;
    let defs_after = registry.definitions(None).await;
    assert!(
        defs_after.iter().any(|d| d.name == "request_permission"),
        "request_permission should be available after ambient tool registration"
    );
}
