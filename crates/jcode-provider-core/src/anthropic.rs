/// Claude Code OAuth beta headers used by the Anthropic transport.
pub const ANTHROPIC_OAUTH_BETA_HEADERS: &str = "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14,context-management-2025-06-27,prompt-caching-scope-2026-01-05,advisor-tool-2026-03-01,advanced-tool-use-2025-11-20,effort-2025-11-24";

/// Claude Code OAuth beta headers with Anthropic's explicit 1M context beta.
pub const ANTHROPIC_OAUTH_BETA_HEADERS_1M: &str = "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14,context-management-2025-06-27,prompt-caching-scope-2026-01-05,advisor-tool-2026-03-01,advanced-tool-use-2025-11-20,effort-2025-11-24,context-1m-2025-08-07";

fn anthropic_base_model_has_default_1m_context(model: &str) -> bool {
    let model = anthropic_strip_1m_suffix(model).trim().to_ascii_lowercase();
    model.starts_with("claude-fable-5")
        || model.starts_with("claude-opus-4-8")
        || model.starts_with("claude-opus-4.8")
        || model.starts_with("claude-opus-4-7")
        || model.starts_with("claude-opus-4.7")
        || model.starts_with("claude-opus-4-6")
        || model.starts_with("claude-opus-4.6")
        || model.starts_with("claude-sonnet-4-6")
        || model.starts_with("claude-sonnet-4.6")
}

/// Check if a model name explicitly requests 1M context via suffix
/// (for example `claude-opus-4-6[1m]`).
pub fn anthropic_is_1m_model(model: &str) -> bool {
    model.ends_with("[1m]")
}

/// Check if a model should use 1M context. Newer Claude models such as Fable 5,
/// Opus 4.8, Opus 4.7, Opus 4.6, and Sonnet 4.6 have 1M context by default on
/// the Claude API/Claude Code surfaces; `[1m]` remains as an explicit legacy alias.
pub fn anthropic_effectively_1m(model: &str) -> bool {
    anthropic_is_1m_model(model) || anthropic_base_model_has_default_1m_context(model)
}

/// Strip the `[1m]` suffix to get the actual API model name.
pub fn anthropic_strip_1m_suffix(model: &str) -> &str {
    model.strip_suffix("[1m]").unwrap_or(model)
}

/// Get the OAuth beta header value appropriate for the model.
pub fn anthropic_oauth_beta_headers(model: &str) -> &'static str {
    if anthropic_effectively_1m(model) {
        ANTHROPIC_OAUTH_BETA_HEADERS_1M
    } else {
        ANTHROPIC_OAUTH_BETA_HEADERS
    }
}

pub fn anthropic_map_tool_name_for_oauth(name: &str) -> String {
    match name {
        "bash" => "Bash",
        "read" => "Read",
        "write" => "Write",
        "edit" => "Edit",
        "glob" => "Glob",
        "grep" => "Grep",
        "subagent" => "Agent",
        "schedule" => "ScheduleWakeup",
        "skill_manage" => "Skill",
        // M12: Anthropic OAuth advertises ToolSearch; route the local
        // `codesearch` dispatcher to that public name on the wire so the
        // model sees a tool whose schema we actually serve.
        "codesearch" => "ToolSearch",
        _ => name,
    }
    .to_string()
}

pub fn anthropic_map_tool_name_from_oauth(name: &str) -> String {
    match name {
        "Bash" => "bash",
        "Read" => "read",
        "Write" => "write",
        "Edit" => "edit",
        "Glob" => "glob",
        "Grep" => "grep",
        "Agent" => "subagent",
        "ScheduleWakeup" => "schedule",
        "Skill" => "skill_manage",
        // M12: When the model invokes the advertised `ToolSearch`, dispatch
        // it via our local `codesearch` handler. Mirrors the equivalent
        // mapping already in `src/provider/claude.rs` for the Claude
        // provider; previously the comment claimed "no direct local
        // analogue" but `codesearch` already serves the same intent.
        "ToolSearch" => "codesearch",
        _ => name,
    }
    .to_string()
}

pub fn anthropic_stainless_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    }
}

pub fn anthropic_stainless_os() -> &'static str {
    match std::env::consts::OS {
        "linux" => "Linux",
        "macos" => "MacOS",
        "windows" => "Windows",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_suffix_helpers_treat_current_long_context_models_as_1m() {
        assert!(anthropic_effectively_1m("claude-fable-5"));
        assert!(anthropic_effectively_1m("claude-opus-4-8"));
        assert!(anthropic_effectively_1m("claude-opus-4-7"));
        assert!(anthropic_effectively_1m("claude-opus-4-6"));
        assert!(anthropic_effectively_1m("claude-sonnet-4-6"));
        assert!(anthropic_effectively_1m("claude-opus-4-6[1m]"));
        assert!(!anthropic_effectively_1m("claude-opus-4-5"));
        assert_eq!(
            anthropic_strip_1m_suffix("claude-opus-4-6[1m]"),
            "claude-opus-4-6"
        );
    }

    #[test]
    fn oauth_beta_headers_follow_1m_suffix() {
        assert_eq!(
            anthropic_oauth_beta_headers("claude-fable-5"),
            ANTHROPIC_OAUTH_BETA_HEADERS_1M
        );
        assert_eq!(
            anthropic_oauth_beta_headers("claude-opus-4-6"),
            ANTHROPIC_OAUTH_BETA_HEADERS_1M
        );
        assert_eq!(
            anthropic_oauth_beta_headers("claude-opus-4-6[1m]"),
            ANTHROPIC_OAUTH_BETA_HEADERS_1M
        );
        assert_eq!(
            anthropic_oauth_beta_headers("claude-opus-4-5"),
            ANTHROPIC_OAUTH_BETA_HEADERS
        );
    }

    #[test]
    fn oauth_tool_name_mapping_is_reversible_for_known_tools() {
        for (local, oauth) in [
            ("bash", "Bash"),
            ("read", "Read"),
            ("subagent", "Agent"),
            ("schedule", "ScheduleWakeup"),
            ("skill_manage", "Skill"),
            // M12: codesearch <-> ToolSearch round-trip.
            ("codesearch", "ToolSearch"),
        ] {
            assert_eq!(anthropic_map_tool_name_for_oauth(local), oauth);
            assert_eq!(anthropic_map_tool_name_from_oauth(oauth), local);
        }
        assert_eq!(anthropic_map_tool_name_for_oauth("custom"), "custom");
    }

    #[test]
    fn stainless_labels_are_non_empty() {
        assert!(!anthropic_stainless_arch().is_empty());
        assert!(!anthropic_stainless_os().is_empty());
    }
}
