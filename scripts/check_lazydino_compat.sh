#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CARGO_BIN=(cargo test -q -p jcode)

echo "== LazyDino compatibility smoke checks =="

echo "-- MCP compatibility"
"${CARGO_BIN[@]}" mcp::client::tests::stdio_mcp_spawn_cwd_falls_back_when_process_cwd_was_removed
"${CARGO_BIN[@]}" mcp::client::tests::m44_http_mcp_connect_uses_bearer_env_and_lists_tools
"${CARGO_BIN[@]}" mcp::client::tests::m44_http_mcp_call_tool_round_trips_arguments
"${CARGO_BIN[@]}" mcp::protocol::protocol_tests::test_m44_remote_mcp_config_roundtrip
"${CARGO_BIN[@]}" mcp::protocol::protocol_tests::test_global_external_mcp_import_is_disabled
"${CARGO_BIN[@]}" tool::tests::execute_repairs_missing_mcp_registry_entry_after_reload
"${CARGO_BIN[@]}" tool::mcp::tests::test_reload_preserves_existing_registry_tools_when_candidate_fails

echo "-- config, hooks, project commands, agent markdown"
"${CARGO_BIN[@]}" config::tests::test_hooks_for_working_dir_dedupes_when_global_path_equals_project_path
"${CARGO_BIN[@]}" config::tests::test_m19_config_reloads_when_mtime_changes
"${CARGO_BIN[@]}" config::tests::test_m19_config_keeps_last_good_on_invalid_toml
"${CARGO_BIN[@]}" project_commands::tests::load_all_commands_only_jcode_global_not_other_globals
"${CARGO_BIN[@]}" project_commands::tests::load_project_local_commands_priority
"${CARGO_BIN[@]}" agent_profiles_md::tests::agents_for_working_dir_project_toml_deep_merges_into_global_md
"${CARGO_BIN[@]}" agent_profiles_md::tests::parse_agent_md_file_accepts_context_window_and_extended_thinking_aliases

echo "-- providers and auth"
"${CARGO_BIN[@]}" auth::tests::openrouter_like_status_is_provider_specific
"${CARGO_BIN[@]}" provider::tests::test_configured_direct_compatible_profiles_are_listed_without_openrouter_key
"${CARGO_BIN[@]}" provider::tests::test_profile_prefixed_model_switch_reinitializes_direct_compatible_runtime
"${CARGO_BIN[@]}" provider::tests::test_gemini_3_5_flash_has_separate_gemini_and_antigravity_routes
"${CARGO_BIN[@]}" provider::antigravity::antigravity_tests::complete_uses_native_https_transport_not_cli_subprocess
"${CARGO_BIN[@]}" provider::antigravity::antigravity_tests::static_catalog_includes_backend_discovered_antigravity_gemini_3_5_flash_ids
"${CARGO_BIN[@]}" provider::gemini::tests::normalize_gemini_function_name_strips_antigravity_default_api_namespace

echo "-- TUI input, session picker, reload, schedule, swarm"
"${CARGO_BIN[@]}" tui::app::tests::test_paste_expansion_on_submit
"${CARGO_BIN[@]}" tui::app::tests::test_tui_system_prompt_uses_session_working_dir_for_agents_md
"${CARGO_BIN[@]}" tui::app::tests::test_context_summary_uses_session_working_dir_for_private_harness_and_skills
"${CARGO_BIN[@]}" tui::app::tests::session_picker_resume_action_keeps_overlay_open
"${CARGO_BIN[@]}" tui::app::tests::test_remote_command_suggestions_include_mcp_reload
"${CARGO_BIN[@]}" tui::app::remote::tests::process_remote_followups_auto_reloads_server_by_default
"${CARGO_BIN[@]}" server::reload::reload_tests::graceful_shutdown_sessions_only_interrupts_triggering_session_when_present
"${CARGO_BIN[@]}" server::reload::reload_tests::graceful_shutdown_sessions_defers_when_peer_remains_running
"${CARGO_BIN[@]}" server::client_lifecycle::tests::reload_starting_rejects_new_turn_without_spawning_processing_task
"${CARGO_BIN[@]}" tool::ambient::tests::m34_schedule_tool_input_accepts_context_alias
"${CARGO_BIN[@]}" server::comm_session::comm_session_tests::resolve_spawn_working_dir_falls_back_to_member_dir
"${CARGO_BIN[@]}" server::comm_session::comm_session_tests::swarm_stop_allowed_by_owner

echo "-- compaction/reload persistence"
"${CARGO_BIN[@]}" compaction::tests::test_new_message_after_restore_reenables_compaction
"${CARGO_BIN[@]}" compaction::tests::test_persisted_state_round_trip_preserves_compacted_view
"${CARGO_BIN[@]}" agent::tests::messages_for_provider_replays_persisted_native_compaction_in_auto_mode
"${CARGO_BIN[@]}" tool::selfdev::tests::test_reload_context_save_and_load_for_session_uses_session_scoped_file

echo "== LazyDino compatibility smoke checks passed =="
