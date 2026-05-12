use super::{Registry, Tool, ToolContext, ToolOutput};
use crate::agent::Agent;
use crate::bus::{Bus, BusEvent, ToolSummary, ToolSummaryState};
use crate::logging;
use crate::protocol::HistoryMessage;
use crate::provider::Provider;
use crate::session::Session;
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

pub struct SubagentTool {
    provider: Arc<dyn Provider>,
    registry: Registry,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ResolvedSubagentRoute {
    model: Option<String>,
    effort: Option<String>,
    description: Option<String>,
    when: Vec<String>,
    prompt: Option<String>,
}

impl SubagentTool {
    pub fn new(provider: Arc<dyn Provider>, registry: Registry) -> Self {
        Self { provider, registry }
    }

    fn preferred_parent_subagent_model(parent_session_id: &str) -> Option<String> {
        Session::load(parent_session_id)
            .ok()
            .and_then(|session| session.subagent_model)
    }

    fn resolve_model(
        requested_model: Option<&str>,
        existing_session_model: Option<&str>,
        routed_model: Option<&str>,
        parent_subagent_model: Option<&str>,
        configured_swarm_model: Option<&str>,
        provider_model: &str,
    ) -> String {
        requested_model
            .or(existing_session_model)
            .or(routed_model)
            .or(parent_subagent_model)
            .or(configured_swarm_model)
            .unwrap_or(provider_model)
            .to_string()
    }

    fn route_for_subagent_type(
        subagent_type: &str,
        working_dir: Option<&Path>,
    ) -> ResolvedSubagentRoute {
        let agents = crate::config::config().agents_for_working_dir(working_dir);
        let direct = subagent_type.trim();
        if direct.is_empty() {
            return ResolvedSubagentRoute::default();
        }

        let rich_route = Self::profile_for_subagent_type(&agents, direct);
        if let Some(route) = rich_route {
            let raw_model = route
                .model
                .as_deref()
                .map(str::trim)
                .filter(|model| !model.is_empty());
            let raw_variant = route.variant.as_deref().map(str::trim);
            let model =
                raw_model.map(|model| Self::apply_route_variant_to_model(model, raw_variant));
            return ResolvedSubagentRoute {
                model,
                effort: route
                    .effort
                    .as_deref()
                    .or(route.variant.as_deref())
                    .and_then(Self::normalize_route_effort),
                description: route.description.clone(),
                when: route.when.clone(),
                prompt: route.prompt.clone(),
            };
        }

        ResolvedSubagentRoute {
            model: agents
                .routing
                .get(direct)
                .or_else(|| agents.routing.get(&direct.to_ascii_lowercase()))
                .cloned()
                .filter(|model| !model.trim().is_empty()),
            effort: None,
            description: None,
            when: Vec::new(),
            prompt: None,
        }
    }

    fn profile_for_subagent_type<'a>(
        agents: &'a crate::config::AgentsConfig,
        subagent_type: &str,
    ) -> Option<&'a crate::config::AgentRouteConfig> {
        let direct = subagent_type.trim();
        let lower = direct.to_ascii_lowercase();
        agents
            .profiles
            .get(direct)
            .or_else(|| agents.profiles.get(&lower))
            .or_else(|| agents.routes.get(direct))
            .or_else(|| agents.routes.get(&lower))
    }

    fn apply_route_variant_to_model(model: &str, variant: Option<&str>) -> String {
        let variant = variant.unwrap_or_default().trim().to_ascii_lowercase();
        if variant == "max" && Self::supports_claude_max_variant(model) && !model.ends_with("[1m]")
        {
            format!("{model}[1m]")
        } else {
            model.to_string()
        }
    }

    fn supports_claude_max_variant(model: &str) -> bool {
        model.starts_with("claude-opus-4-7")
            || model.starts_with("claude-opus-4-6")
            || model.starts_with("claude-sonnet-4-6")
    }

    fn normalize_route_effort(effort: &str) -> Option<String> {
        match effort.trim().to_ascii_lowercase().as_str() {
            "none" | "low" | "medium" | "high" | "xhigh" => {
                Some(effort.trim().to_ascii_lowercase())
            }
            // oh-my-opencode's `max` variant maps to jcode/OpenAI's highest effort level.
            "max" => Some("xhigh".to_string()),
            "" => None,
            _ => None,
        }
    }

    fn should_apply_route_effort(model: &str) -> bool {
        model.starts_with("gpt-") || model.starts_with("openai/")
    }

    fn configured_subagent_types(working_dir: Option<&Path>) -> Vec<String> {
        let agents = crate::config::config().agents_for_working_dir(working_dir);
        Self::configured_subagent_types_for_agents(&agents)
    }

    fn configured_subagent_types_for_agents(agents: &crate::config::AgentsConfig) -> Vec<String> {
        let mut types = BTreeSet::new();
        types.insert("general".to_string());
        types.extend(agents.profiles.keys().cloned());
        types.extend(agents.routes.keys().cloned());
        // Deprecated/simple routing remains a compatibility fallback, so expose those keys too
        // when a user still has them configured.
        types.extend(agents.routing.keys().cloned());
        types.into_iter().collect()
    }

    fn configured_subagent_type_docs(working_dir: Option<&Path>) -> Vec<String> {
        let agents = crate::config::config().agents_for_working_dir(working_dir);
        Self::configured_subagent_type_docs_for_agents(&agents)
    }

    fn configured_subagent_type_docs_for_agents(
        agents: &crate::config::AgentsConfig,
    ) -> Vec<String> {
        let mut docs = Vec::new();
        let mut seen = BTreeSet::new();
        docs.push("general: default general-purpose subagent".to_string());
        seen.insert("general".to_string());
        for (name, route) in agents.profiles.iter().chain(agents.routes.iter()) {
            if !seen.insert(name.clone()) {
                continue;
            }
            let mut parts = Vec::new();
            if let Some(description) = route
                .description
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                parts.push(description.to_string());
            }
            if !route.when.is_empty() {
                parts.push(format!("use when: {}", route.when.join("; ")));
            }
            if let Some(model) = route
                .model
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                parts.push(format!("model: {model}"));
            }
            docs.push(if parts.is_empty() {
                name.clone()
            } else {
                format!("{}: {}", name, parts.join("; "))
            });
        }
        for (name, model) in &agents.routing {
            if seen.insert(name.clone()) {
                docs.push(format!("{}: legacy route model: {}", name, model));
            }
        }
        docs
    }

    fn prompt_with_profile(
        prompt: &str,
        subagent_type: &str,
        route: &ResolvedSubagentRoute,
    ) -> String {
        let has_description = route
            .description
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());
        let has_prompt = route
            .prompt
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());
        if !has_description && route.when.is_empty() && !has_prompt {
            return prompt.to_string();
        }

        let mut output = String::new();
        output.push_str("<agent_profile>\n");
        output.push_str(&format!("type: {}\n", subagent_type.trim()));
        if let Some(description) = route
            .description
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            output.push_str(&format!("description: {}\n", description));
        }
        if !route.when.is_empty() {
            output.push_str("when_to_use:\n");
            for item in &route.when {
                output.push_str(&format!("- {}\n", item));
            }
        }
        if let Some(profile_prompt) = route
            .prompt
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            output.push_str("instructions:\n");
            output.push_str(profile_prompt);
            output.push('\n');
        }
        output.push_str("</agent_profile>\n\n");
        output.push_str(prompt);
        output
    }
}

#[derive(Deserialize)]
struct SubagentInput {
    description: String,
    prompt: String,
    #[serde(default = "default_subagent_type")]
    subagent_type: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    output_mode: SubagentOutputMode,
    #[serde(rename = "command", default)]
    _command: Option<String>,
}

fn default_subagent_type() -> String {
    "general".to_string()
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SubagentOutputMode {
    /// Return only the subagent's final answer plus metadata. This preserves the
    /// historical low-token default for ordinary delegation.
    #[default]
    Answer,
    /// Return the final answer plus a human-readable transcript similar to what
    /// a user would inspect: roles, text, tool calls, and tool results.
    Compact,
    /// Return the final answer plus the persisted raw child session messages as
    /// pretty JSON for debugging/auditing.
    FullTranscript,
}

#[async_trait]
impl Tool for SubagentTool {
    fn name(&self) -> &str {
        "subagent"
    }

    fn description(&self) -> &str {
        "Run a subagent."
    }

    fn parameters_schema(&self) -> Value {
        let configured_types = Self::configured_subagent_types(None);
        let configured_docs = Self::configured_subagent_type_docs(None);
        let type_description = if configured_docs.is_empty() {
            "Subagent type.".to_string()
        } else {
            format!(
                "Subagent type. Configured agent profiles: {}.",
                configured_docs.join(" | ")
            )
        };
        let mut schema = json!({
            "type": "object",
            "required": ["description", "prompt", "subagent_type"],
            "properties": {
                "intent": super::intent_schema_property(),
                "description": {
                    "type": "string",
                    "description": "Task description."
                },
                "prompt": {
                    "type": "string",
                    "description": "Task prompt."
                },
                "subagent_type": {
                    "type": "string",
                    "description": type_description
                },
                "model": {
                    "type": "string",
                    "description": "Model override."
                },
                "session_id": {
                    "type": "string",
                    "description": "Existing session ID."
                },
                "output_mode": {
                    "type": "string",
                    "enum": ["answer", "compact", "full_transcript"],
                    "description": "Return mode. 'answer' returns the final answer only, 'compact' adds a user-visible transcript, and 'full_transcript' adds raw persisted messages. Defaults to 'answer'."
                },
                "command": {
                    "type": "string",
                    "description": "Source command."
                }
            }
        });
        if !configured_types.is_empty()
            && let Some(subagent_type_schema) = schema
                .get_mut("properties")
                .and_then(|properties| properties.get_mut("subagent_type"))
        {
            // Keep the schema flexible: models may invent useful ad-hoc types, while configured
            // profiles are still advertised as examples and explained in the description.
            subagent_type_schema["examples"] = json!(configured_types);
        }
        schema
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: SubagentInput = serde_json::from_value(input)?;

        let mut session = if let Some(session_id) = &params.session_id {
            Session::load(session_id).unwrap_or_else(|err| {
                logging::warn(&format!(
                    "[tool:subagent] failed to load existing session {}; creating a new subagent session instead: {}",
                    session_id, err
                ));
                Session::create(Some(ctx.session_id.clone()), Some(subagent_title(&params)))
            })
        } else {
            Session::create(Some(ctx.session_id.clone()), Some(subagent_title(&params)))
        };
        let parent_subagent_model = Self::preferred_parent_subagent_model(&ctx.session_id);
        let provider_model = self.provider.model();
        let agents = crate::config::config().agents_for_working_dir(ctx.working_dir.as_deref());
        let route =
            Self::route_for_subagent_type(&params.subagent_type, ctx.working_dir.as_deref());
        let resolved_model = Self::resolve_model(
            params.model.as_deref(),
            session.model.as_deref(),
            route.model.as_deref(),
            parent_subagent_model.as_deref(),
            agents.swarm_model.as_deref(),
            &provider_model,
        );
        session.model = Some(resolved_model.clone());
        if session.reasoning_effort.is_none()
            && let Some(route_effort) = route.effort.clone()
            && Self::should_apply_route_effort(&resolved_model)
        {
            session.reasoning_effort = Some(route_effort);
        }

        if let Some(ref working_dir) = ctx.working_dir {
            session.working_dir = Some(working_dir.display().to_string());
        }

        session.save()?;

        let mut allowed: HashSet<String> = self.registry.tool_names().await.into_iter().collect();
        // Lazydino: allow optional recursive subagent calls. By default (and matching
        // upstream behavior) child subagents have `subagent`/`task`/`todo*` removed
        // from their tool set to prevent unbounded recursion. Setting
        // `[agents] allow_subagent_recursion = true` in config.toml, or the env var
        // `JCODE_ALLOW_SUBAGENT_RECURSION=1`, lifts that restriction.
        let allow_recursion = {
            let env_override = std::env::var("JCODE_ALLOW_SUBAGENT_RECURSION")
                .ok()
                .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"));
            match env_override {
                Some(value) => value,
                None => agents.allow_subagent_recursion,
            }
        };
        if !allow_recursion {
            for blocked in ["subagent", "task", "todo", "todowrite", "todoread"] {
                allowed.remove(blocked);
            }
        }

        let summary_map: Arc<Mutex<HashMap<String, ToolSummary>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let summary_map_handle = summary_map.clone();
        let session_id = session.id.clone();

        let mut receiver = Bus::global().subscribe();
        let listener = tokio::spawn(async move {
            loop {
                match receiver.recv().await {
                    Ok(BusEvent::ToolUpdated(event)) => {
                        if event.session_id != session_id {
                            continue;
                        }
                        let mut summary = summary_map_handle
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        summary.insert(
                            event.tool_call_id.clone(),
                            ToolSummary {
                                id: event.tool_call_id.clone(),
                                tool: event.tool_name.clone(),
                                state: ToolSummaryState {
                                    status: event.status.as_str().to_string(),
                                    title: if event.status.as_str() == "completed" {
                                        event.title.clone()
                                    } else {
                                        None
                                    },
                                },
                            },
                        );
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
        });

        logging::info(&format!(
            "Subagent starting: {} (type: {})",
            params.description, params.subagent_type
        ));

        // Run subagent on an isolated provider fork so model/session changes do not
        // mutate the coordinator's provider instance.
        let mut agent = Agent::new_with_session(
            self.provider.fork(),
            self.registry.clone(),
            session,
            Some(allowed),
        );

        let start = std::time::Instant::now();
        let prompt = Self::prompt_with_profile(&params.prompt, &params.subagent_type, &route);
        let final_text = agent.run_once_capture(&prompt).await.map_err(|err| {
            logging::warn(&format!(
                "[tool:subagent] subagent failed description={} type={} session_id={} model={} error={}",
                params.description,
                params.subagent_type,
                agent.session_id(),
                resolved_model,
                err
            ));
            err
        })?;
        let sub_session_id = agent.session_id().to_string();
        let history = if params.output_mode == SubagentOutputMode::Compact {
            Some(agent.get_history())
        } else {
            None
        };
        let full_transcript = if params.output_mode == SubagentOutputMode::FullTranscript {
            let session = Session::load(&sub_session_id)?;
            Some(serde_json::to_string_pretty(&session.messages)?)
        } else {
            None
        };

        logging::info(&format!(
            "Subagent completed: {} in {:.1}s",
            params.description,
            start.elapsed().as_secs_f64()
        ));

        listener.abort();

        let mut summary: Vec<ToolSummary> = summary_map
            .lock()
            .map_err(|_| anyhow::anyhow!("tool summary lock poisoned"))?
            .values()
            .cloned()
            .collect();
        summary.sort_by(|a, b| a.id.cmp(&b.id));

        let output = format_subagent_output(
            &final_text,
            &sub_session_id,
            params.output_mode,
            history.as_deref(),
            full_transcript.as_deref(),
        );

        Ok(ToolOutput::new(output)
            .with_title(subagent_display_title(&params, &resolved_model))
            .with_metadata(json!({
                "summary": summary,
                "sessionId": sub_session_id,
                "model": resolved_model,
                "outputMode": params.output_mode.as_str(),
            })))
    }
}

fn subagent_title(params: &SubagentInput) -> String {
    format!(
        "{} (@{} subagent)",
        params.description, params.subagent_type
    )
}

fn subagent_display_title(params: &SubagentInput, model: &str) -> String {
    format!(
        "{} ({} · {})",
        params.description, params.subagent_type, model
    )
}

impl SubagentOutputMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Answer => "answer",
            Self::Compact => "compact",
            Self::FullTranscript => "full_transcript",
        }
    }
}

fn format_subagent_output(
    final_text: &str,
    sub_session_id: &str,
    output_mode: SubagentOutputMode,
    history: Option<&[HistoryMessage]>,
    full_transcript: Option<&str>,
) -> String {
    let mut output = final_text.to_string();
    if !output.ends_with('\n') {
        output.push('\n');
    }

    match output_mode {
        SubagentOutputMode::Answer => {}
        SubagentOutputMode::Compact => {
            output.push_str("\n## Subagent transcript (compact)\n\n");
            output.push_str(&format_compact_subagent_history(history.unwrap_or(&[])));
        }
        SubagentOutputMode::FullTranscript => {
            output.push_str("\n## Subagent transcript (full)\n\n```json\n");
            output.push_str(full_transcript.unwrap_or("[]"));
            output.push_str("\n```\n");
        }
    }

    output.push('\n');
    output.push_str("<subagent_metadata>\n");
    output.push_str(&format!("session_id: {}\n", sub_session_id));
    output.push_str(&format!("output_mode: {}\n", output_mode.as_str()));
    output.push_str("</subagent_metadata>");
    output
}

fn format_compact_subagent_history(messages: &[HistoryMessage]) -> String {
    if messages.is_empty() {
        return "(empty transcript)\n".to_string();
    }

    let mut output = String::new();
    for (index, message) in messages.iter().enumerate() {
        output.push_str(&format!("### {}. {}\n\n", index + 1, message.role));
        if !message.content.trim().is_empty() {
            output.push_str(message.content.trim());
            output.push_str("\n\n");
        }
        if let Some(tool_calls) = &message.tool_calls
            && !tool_calls.is_empty()
        {
            output.push_str("Tool calls:\n");
            for call in tool_calls {
                output.push_str(&format!("- `{}`\n", call));
            }
            output.push('\n');
        }
        if let Some(tool_data) = &message.tool_data {
            output.push_str("Tool result:\n");
            output.push_str("```json\n");
            match serde_json::to_string_pretty(tool_data) {
                Ok(json) => output.push_str(&json),
                Err(_) => output.push_str("<unserializable tool data>"),
            }
            output.push_str("\n```\n\n");
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{
        SubagentInput, SubagentOutputMode, format_compact_subagent_history, format_subagent_output,
        subagent_display_title,
    };
    use crate::config::{AgentRouteConfig, AgentsConfig};
    use crate::message::{Message, ToolDefinition};
    use crate::protocol::HistoryMessage;
    use crate::provider::{EventStream, Provider};
    use jcode_tool_core::Tool;
    use std::sync::Arc;

    struct SchemaOnlyProvider;

    #[async_trait::async_trait]
    impl Provider for SchemaOnlyProvider {
        async fn complete(
            &self,
            _messages: &[Message],
            _tools: &[ToolDefinition],
            _system: &str,
            _resume_session_id: Option<&str>,
        ) -> anyhow::Result<EventStream> {
            unreachable!("schema-only provider should not be called")
        }

        fn name(&self) -> &str {
            "schema-only"
        }

        fn fork(&self) -> Arc<dyn Provider> {
            Arc::new(Self)
        }
    }

    #[test]
    fn subagent_display_title_includes_type_and_model() {
        let params = SubagentInput {
            description: "Verify subagent model".to_string(),
            prompt: "prompt".to_string(),
            subagent_type: "general".to_string(),
            model: None,
            session_id: None,
            output_mode: SubagentOutputMode::Answer,
            _command: None,
        };

        assert_eq!(
            subagent_display_title(&params, "gpt-5.4"),
            "Verify subagent model (general · gpt-5.4)"
        );
    }

    #[test]
    fn resolve_model_prefers_explicit_then_existing_then_route_then_parent_then_configured_then_provider()
     {
        assert_eq!(
            super::SubagentTool::resolve_model(
                Some("explicit"),
                Some("existing"),
                Some("route"),
                Some("parent"),
                Some("configured"),
                "provider"
            ),
            "explicit"
        );
        assert_eq!(
            super::SubagentTool::resolve_model(
                None,
                Some("existing"),
                Some("route"),
                Some("parent"),
                Some("configured"),
                "provider"
            ),
            "existing"
        );
        assert_eq!(
            super::SubagentTool::resolve_model(
                None,
                None,
                Some("route"),
                Some("parent"),
                Some("configured"),
                "provider"
            ),
            "route"
        );
        assert_eq!(
            super::SubagentTool::resolve_model(
                None,
                None,
                None,
                Some("parent"),
                Some("configured"),
                "provider"
            ),
            "parent"
        );
        assert_eq!(
            super::SubagentTool::resolve_model(
                None,
                None,
                None,
                None,
                Some("configured"),
                "provider"
            ),
            "configured"
        );
        assert_eq!(
            super::SubagentTool::resolve_model(None, None, None, None, None, "provider"),
            "provider"
        );
    }

    #[test]
    fn route_effort_normalizes_opencode_variants() {
        assert_eq!(
            super::SubagentTool::normalize_route_effort("medium"),
            Some("medium".to_string())
        );
        assert_eq!(
            super::SubagentTool::normalize_route_effort("HIGH"),
            Some("high".to_string())
        );
        assert_eq!(
            super::SubagentTool::normalize_route_effort("max"),
            Some("xhigh".to_string())
        );
        assert_eq!(super::SubagentTool::normalize_route_effort("unknown"), None);
    }

    #[test]
    fn route_variant_max_maps_supported_claude_models_to_1m_suffix() {
        assert_eq!(
            super::SubagentTool::apply_route_variant_to_model("claude-opus-4-7", Some("max")),
            "claude-opus-4-7[1m]"
        );
        assert_eq!(
            super::SubagentTool::apply_route_variant_to_model("claude-opus-4-7[1m]", Some("max")),
            "claude-opus-4-7[1m]"
        );
        assert_eq!(
            super::SubagentTool::apply_route_variant_to_model("claude-haiku-4-5", Some("max")),
            "claude-haiku-4-5"
        );
        assert_eq!(
            super::SubagentTool::apply_route_variant_to_model("gpt-5.5", Some("max")),
            "gpt-5.5"
        );
    }

    #[test]
    fn route_effort_applies_only_to_openai_style_models() {
        assert!(super::SubagentTool::should_apply_route_effort("gpt-5.5"));
        assert!(super::SubagentTool::should_apply_route_effort(
            "openai/gpt-5.5"
        ));
        assert!(!super::SubagentTool::should_apply_route_effort(
            "claude-opus-4-7"
        ));
        assert!(!super::SubagentTool::should_apply_route_effort(
            "gemini-3.1-pro-preview"
        ));
    }

    #[test]
    fn configured_subagent_types_include_profiles_routes_and_legacy_routing() {
        let mut agents = AgentsConfig::default();
        agents.profiles.insert(
            "planner".to_string(),
            AgentRouteConfig {
                model: Some("claude-opus-4-7".to_string()),
                variant: Some("max".to_string()),
                description: Some("Plan ambiguous work".to_string()),
                when: vec!["the request needs decomposition".to_string()],
                ..Default::default()
            },
        );
        agents.routes.insert(
            "coder".to_string(),
            AgentRouteConfig {
                model: Some("gpt-5.5".to_string()),
                variant: Some("medium".to_string()),
                ..Default::default()
            },
        );
        agents.routing.insert(
            "legacy-reviewer".to_string(),
            "claude-sonnet-4-6".to_string(),
        );

        assert_eq!(
            super::SubagentTool::configured_subagent_types_for_agents(&agents),
            vec!["coder", "general", "legacy-reviewer", "planner"]
        );
        let docs = super::SubagentTool::configured_subagent_type_docs_for_agents(&agents);
        assert!(
            docs.iter()
                .any(|doc| doc.contains("planner: Plan ambiguous work"))
        );
        assert!(
            docs.iter()
                .any(|doc| doc.contains("use when: the request needs decomposition"))
        );
    }

    #[test]
    fn test_subagent_routes_use_project_agents_config() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join(".jcode")).expect("create .jcode");
        std::fs::write(
            project.join(".jcode").join("config.toml"),
            r#"
            [agents.profiles.localreviewer]
            model = "project-local-model"
            description = "Project-local reviewer"
            prompt = "Use the project-local review checklist."
            "#,
        )
        .expect("write project config");

        let global_route = super::SubagentTool::route_for_subagent_type("localreviewer", None);
        let project_route =
            super::SubagentTool::route_for_subagent_type("localreviewer", Some(&project));

        assert_ne!(global_route.model.as_deref(), Some("project-local-model"));
        assert_eq!(project_route.model.as_deref(), Some("project-local-model"));
        assert_eq!(
            project_route.description.as_deref(),
            Some("Project-local reviewer")
        );
        assert_eq!(
            project_route.prompt.as_deref(),
            Some("Use the project-local review checklist.")
        );
    }

    #[test]
    fn prompt_with_profile_prepends_agent_profile_guidance() {
        let route = super::ResolvedSubagentRoute {
            model: Some("claude-opus-4-7[1m]".to_string()),
            effort: None,
            description: Some("Plan ambiguous work".to_string()),
            when: vec!["the request needs decomposition".to_string()],
            prompt: Some("Return a concise execution plan.".to_string()),
        };

        let prompt = super::SubagentTool::prompt_with_profile("Original task", "planner", &route);

        assert!(prompt.starts_with("<agent_profile>\n"));
        assert!(prompt.contains("type: planner\n"));
        assert!(prompt.contains("description: Plan ambiguous work\n"));
        assert!(prompt.contains("- the request needs decomposition\n"));
        assert!(prompt.contains("Return a concise execution plan."));
        assert!(prompt.ends_with("Original task"));
    }

    #[test]
    fn subagent_type_defaults_to_general_when_omitted() {
        let input: SubagentInput = serde_json::from_value(serde_json::json!({
            "description": "Do work",
            "prompt": "Work carefully"
        }))
        .expect("input without subagent_type should deserialize");

        assert_eq!(input.subagent_type, "general");
    }

    #[tokio::test]
    async fn subagent_schema_advertises_examples_without_restrictive_enum() {
        let provider = std::sync::Arc::new(SchemaOnlyProvider);
        let schema = super::SubagentTool::new(
            provider.clone(),
            super::Registry::new(provider.clone()).await,
        )
        .parameters_schema();
        let subagent_type_schema = &schema["properties"]["subagent_type"];
        assert!(subagent_type_schema.get("enum").is_none());
        assert!(subagent_type_schema["examples"].is_array());
    }

    #[test]
    fn format_subagent_output_preserves_answer_without_generic_next_step_footer() {
        let output = format_subagent_output(
            "answer",
            "session_test",
            SubagentOutputMode::Answer,
            None,
            None,
        );

        assert!(output.starts_with("answer\n\n<subagent_metadata>\n"));
        assert!(output.contains("session_id: session_test\n"));
        assert!(output.contains("output_mode: answer\n"));
        assert!(!output.contains("Next step: integrate this result"));
    }

    #[test]
    fn compact_output_includes_human_readable_history() {
        let history = vec![HistoryMessage {
            role: "assistant".to_string(),
            content: "I will inspect it.".to_string(),
            tool_calls: Some(vec!["read".to_string()]),
            tool_data: None,
        }];
        let output = format_subagent_output(
            "final answer",
            "session_test",
            SubagentOutputMode::Compact,
            Some(&history),
            None,
        );

        assert!(output.contains("## Subagent transcript (compact)"));
        assert!(output.contains("### 1. assistant"));
        assert!(output.contains("I will inspect it."));
        assert!(output.contains("- `read`"));
        assert!(output.contains("output_mode: compact\n"));
    }

    #[test]
    fn full_transcript_output_includes_raw_json_section() {
        let output = format_subagent_output(
            "final answer",
            "session_test",
            SubagentOutputMode::FullTranscript,
            None,
            Some("[{\"role\":\"user\"}]"),
        );

        assert!(output.contains("## Subagent transcript (full)"));
        assert!(output.contains("```json\n[{\"role\":\"user\"}]\n```"));
        assert!(output.contains("output_mode: full_transcript\n"));
    }

    #[test]
    fn compact_history_formats_empty_transcript() {
        assert_eq!(format_compact_subagent_history(&[]), "(empty transcript)\n");
    }
}
