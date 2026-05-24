#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CARGO=("$ROOT/scripts/dev_cargo.sh" test -q -p jcode)

run_jcode_test() {
  echo "+ ${CARGO[*]} $*"
  "${CARGO[@]}" "$@"
}

echo "== LazyDino full custom regression audit =="

echo "-- prompt/private harness/skills/project commands"
run_jcode_test prompt::prompt_tests
run_jcode_test agent_profiles_md::tests
run_jcode_test project_commands::tests
run_jcode_test project_init::tests::init_project_writes_private_harness_and_git_exclude
run_jcode_test tui::app::tests::test_tui_system_prompt_uses_session_working_dir_for_agents_md
run_jcode_test tui::app::tests::test_context_summary_uses_session_working_dir_for_private_harness_and_skills

echo "-- MCP registry/reload/protocol/client"
run_jcode_test mcp::client::tests
run_jcode_test mcp::protocol::protocol_tests
run_jcode_test tool::mcp::tests

echo "-- auth/provider custom routes and env isolation"
run_jcode_test auth::tests
run_jcode_test auth::antigravity::tests
run_jcode_test auth::external::external_tests
run_jcode_test provider::tests
run_jcode_test provider::antigravity::antigravity_tests
run_jcode_test provider::gemini::tests
run_jcode_test provider::openrouter::tests
run_jcode_test provider_catalog_tests

echo "-- reload/session/swarm/selfdev"
run_jcode_test server::reload::reload_tests
run_jcode_test server::client_session::tests::reload_tests
run_jcode_test server::client_lifecycle::tests
run_jcode_test server::comm_session::comm_session_tests
run_jcode_test tool::selfdev::tests

echo "-- TUI input/remote/session picker/custom commands"
run_jcode_test tui::app::tests::test_paste_expansion_on_submit
run_jcode_test tui::app::tests::test_handle_paste_large
run_jcode_test tui::app::tests::session_picker_resume_action_keeps_overlay_open
run_jcode_test tui::app::tests::test_remote_command_suggestions_include_mcp_reload
run_jcode_test tui::app::remote::tests
run_jcode_test tui::session_picker::tests

echo "-- compaction/native/session persistence"
run_jcode_test compaction::tests
run_jcode_test agent::tests::messages_for_provider_replays_persisted_native_compaction_in_auto_mode
run_jcode_test agent::tests::messages_for_provider_applies_manual_compaction_in_native_auto_mode
run_jcode_test agent::tests::oversized_openai_native_compaction_is_persisted_as_text_fallback
run_jcode_test agent::tests::restore_session_rehydrates_injected_memory_ids

echo "-- ambient/schedule/tools/hooks/cwd"
run_jcode_test ambient::ambient_tests
run_jcode_test ambient::runner::runner_tests
run_jcode_test tool::ambient::tests
run_jcode_test hooks::tests -- --test-threads=1
run_jcode_test cwd::tests

echo "== LazyDino full custom regression audit passed =="
