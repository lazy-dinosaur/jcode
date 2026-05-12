# Lazydino Jcode — Outstanding Milestones

마지막 업데이트: 2026-05-11 (round 15 — M21 ver.2 deployed, M28~M32 신규 등록)
관련 문서:
- `LAZYDINO_MAINTENANCE.md` (커밋된 패치 이력)
- `LAZYDINO_STATUS_2026-05-11.md` (현재 배포/OPEN 상태 스냅샷)

이 문서는 **아직 미해결이거나 계획된 큰 작업** 들을 추적합니다. 완료되면 `LAZYDINO_MAINTENANCE.md` 로 이동시키고 여기에서 제거 또는 "DONE" 표시합니다.

추가/수정 규칙:
- 새로 발견한 증상은 빠짐없이 본 문서에 기록한다.
- 진단이 진행되면 해당 마일스톤 본문에 누적 업데이트.
- 마일스톤 완료 시 `LAZYDINO_MAINTENANCE.md` 로 이동, 본 문서에서 제거.

---

## 📋 마일스톤 진행 현황 (한눈에)

| ID  | 제목                                               | 상태   | 우선순위    |
|-----|----------------------------------------------------|--------|-------------|
| M1  | Background task delivery 가 parent/report-back chain 을 안 따라감 | ✅ **DONE** (commit `1387e77e` + `b9085898`, binary `b9085898`) | — |
| M2  | Swarm 버그 (phase C diagnostics + upstream #76)    | ✅ **DONE** — Stages 1-4 complete (Stage 4: worker heartbeat, reversible running_stale, opt-in per-task timeout). Stage 3 의 spawn hard cap default 는 후속 결정으로 `0` (unlimited, upstream 동작 동일) 으로 변경 — opt-in safety harness 로 유지 (commit `4f90de73`). #76 의 진짜 fix 는 Stage 4 의 heartbeat + cwd validation (Stage 3) + force-headless (Stage 2). | — |
| M3  | Hook 시스템 확장 (`session.stop`, `response.completed`) | ✅ **DONE** (commit `003fcf65` + `1c97ef70`, binary `1c97ef70`) | — |
| M4  | TUI interleave 가 tool 완료 후에야 흡수됨 (jcode 원설계) | 🟡 BY-DESIGN with caveats — UX 개선 가치 | Medium |
| M5  | Alt+B early race — `background_tool_signal.reset()` 이 너무 늦음 | ✅ **DONE** (commit `52375aac` + `f76dfdda`, binary `f76dfdda`) | — |
| M6  | Alt+B 후 부모 turn 이 idle 빠져도 background task 완료 시 자동 wake | ✅ **DONE** (commit `f2c8430f` + `074dcbef`, binary `074dcbef`) — 라이브 30s idle wake 검증 완료 (T-PLAN 2026-05-10 12:36 UTC) | — |
| M7  | 비정상 종료 시 메시지 유실 — `/reload`, server SIGKILL/crash 시 `/save` 안 부른 채로 종료하면 모든 메시지 날아감 | ✅ **DONE** (commit `5f597b98`, binary `5f597b98`) — `load_startup_stub` fallback 이 journal replay 하도록 fix + 회귀 테스트 추가 | **Critical (데이터 유실)** |
| M8  | Alt+B detach 후 turn 이 안 끝나서 TUI `is_processing=true` 로 묶임 (queued 메시지 dispatch 안 됨) | ✅ **DONE** (commit `4e3d8189`, binary `4e3d8189`) — detach branch 가 skipped ToolResult fill + `return Ok(())` 로 turn 즉시 종료. 회귀 테스트 `turn_streaming_mpsc_altb_ends_turn_immediately_after_detach` 추가 (provider_calls == 1 assert) | High |
| M9  | Hook 이중 발동 — `~/.jcode/config.toml` 이 home 하위 working_dir 에서 글로벌+project local 양쪽으로 read (자기참조 skip 없음, 코드 read 로 확정) | ✅ **DONE** (commit `82b7c81f` on `deploy/m9-m10`, source `9774d9a6` on `patch/hook-config-dedupe`, binary `82b7c81f`) — `Config::hooks_for_working_dir` 가 `paths_resolve_to_same_file` (canonicalize+lexical fallback) 로 global path == project-discovered path 인 경우 merge skip. 회귀 테스트 2건: dedupe 동작 + distinct path merge 보존 | — |
| M10 | Non-blocking lifecycle hook 이 단발성 CLI (`jcode run`) 에서 race 로 누락 — `tokio::spawn` fire-and-forget + 즉시 process 종료 | ✅ **DONE** (commit `13dd3132` on `deploy/m9-m10`, source `e74791df` on `patch/lifecycle-hook-cli-flush`, binary `82b7c81f`) — `pending_nonblocking_hooks()` (`OnceLock<Mutex<Vec<JoinHandle>>>`) + `spawn_tracked_nonblocking_hook` + `flush_nonblocking_hooks(timeout)` 추가. 두 spawn site (tool nonblocking, lifecycle nonblocking) 가 tracked spawn 사용. `cli/startup.rs::run` 가 dispatch 종료 후 `flush_nonblocking_hooks(5s)` 호출. 회귀 테스트 3건 (serial mutex 로 process-global state 보호): tracked handle 대기, 빈 슬롯 short-circuit, timeout 경계 | — |
| M11 | Lifecycle hook (`response.completed`, `session.stop`) 의 `decision.action` JSON parsing + medivance/claude-code style self-correcting stop semantics | ✅ **DONE** — stages 1+2+3+4+5+6 complete. Latest Stage 6 deploy commits `f74bffac` + `9c8ff1ea` (source `8aa6f8c4` + `21883999`) makes `response.completed` deny trigger an immediate continuation turn instead of waiting for the next user prompt. Added wire-compatible `stop_hook_active: bool`, configurable `max_lifecycle_deny_streak` (env `JCODE_MAX_LIFECYCLE_DENY_STREAK` > project > global > default 3, `0` unlimited), user-turn streak reset, and streaming/non-streaming continuation handling. Tests: hooks payload golden + stop_hook_active, lifecycle deny cap/default/env/reset, config project override. | — |
| M12 | Anthropic OAuth provider 가 `ToolSearch` 를 AI 에게 광고하는데 dispatch 핸들러 미구현 — AI 가 호출하면 `Unknown tool: ToolSearch` 에러 | ✅ **DONE** (commit `c14ffdf8`, branch `patch/anthropic-oauth-tool-schema-align`) — `crates/jcode-provider-core/src/anthropic.rs` 에 `ToolSearch <-> codesearch` 양방향 매핑 추가 + `src/provider/anthropic.rs` 의 광고 schema 를 `CodeSearchTool` dispatch 와 align (required `query`, dead `max_results` 제거). 회귀 테스트 `test_oauth_tool_search_advertised_schema_matches_codesearch_dispatch` 추가 | — |
| M13 | `schedule` (ScheduleWakeup) 도구 schema 가 AI 호출 형식과 불일치 — AI 가 호출하면 `missing field 'task'` 에러 | ✅ **DONE** (commit `c14ffdf8`, branch `patch/anthropic-oauth-tool-schema-align`) — `src/provider/anthropic.rs` 의 `ScheduleWakeup` 광고 schema 를 `ScheduleTool` dispatch 와 align (required `task`, dead `delaySeconds`/`reason`/`prompt` 제거). 회귀 테스트 `test_oauth_schedule_tool_advertised_schema_matches_dispatch` 추가 | — |
| M14 | `/compact` (대화 컨텍스트 축약 명령) 동작 안 함 — 사용자 보고 (2026-05-10 13:28 UTC). 진단 미착수 | ✅ **DONE** (commit `e71713ba`, branch `patch/compaction-failure-cooldown`) — 진단 결과 "동작 안 함" 의 실체는 **요약기가 한 번 실패한 뒤 proactive/semantic auto-trigger 가 매 turn 마다 재발화** 였음. 실패 path 가 `turns_since_last_compact` 를 reset 안 해서 cooldown 무력화됨. 새 streak counter (`MAX_CONSECUTIVE_COMPACTION_FAILURES=3`) + `note_compaction_success/failure` helper + `should_compact_with` short-circuit 추가. 회귀 테스트 4건 (`test_note_compaction_*`, `test_should_compact_with_short_circuits_after_failure_streak`) | — |
| M14a | **Emergency compaction 무한 루프** — 라이브 22회 연속 emergency compaction (사용자 13:42 UTC 보고). 504k→20k→500k 패턴 반복. retry counter `MAX_CONTEXT_LIMIT_RETRIES=5` 는 이미 존재하지만 무력화됨 — 카운터가 reset 되는 경로가 있다는 뜻 | ✅ **DONE** (commit `e71713ba`, branch `patch/compaction-failure-cooldown`) — M14 와 동일한 streak counter 를 `Agent::try_auto_compact_after_context_limit` 와 `ensure_context_fits` 의 critical-threshold hard-compact 에 공유. per-turn `MAX_CONTEXT_LIMIT_RETRIES` 가 못 보는 session-wide 반복을 streak 가 차단함. 22회 같은 폭주 패턴은 이제 3회 후 emergency 진입 자체가 거부되며 그대로 turn-level retry budget 이 reject → AI 에게 context-limit error propagation. 라이브 재현은 다음 jcode session 에서 검증 필요 | — |
| M15 | Jcode TUI 에 첨부된 이미지가 외부 jcode session (지금 우리 대화하는 session) 에는 alt-text 만 들어오고 실제 image data 가 안 옴 — 디버그 사이클 효율 영향 | ✅ **DONE — candidate C** (commits `09f19df9` protocol test backfill + `f3b7d72d` M15 feature on `patch/m15-sibling-user-message-fanout`) — `ServerEvent::UserMessage { id, session_id, content, images: Vec<RenderedImage> }` 새 wire variant 추가; `start_processing_message` 가 turn 시작 직후 `fanout_session_event_except(.., Some(origin_conn_id), ..)` 으로 sibling 전체에게 echo, origin 은 exclusion 으로 double-render 방지. TUI 측 `tui::app::remote::server_events` 가 `active_client_session_id` 매칭 시 `DisplayMessage::user` push + `remote_side_pane_images.extend(images)`. round-trip 2건 (`test_user_message_event_roundtrip{,_no_images}`) protocol crate test 통과. 2개의 pre-existing 실패 (`handle_resume_session_allows_live_attach_when_existing_agent_is_busy`, `handle_get_history_falls_back_to_persisted_snapshot_when_agent_is_busy`) 는 baseline `0c27110d` 에서도 동일 — M15 와 무관 (별도 backlog M27 로 등록 권고). 후속: 라이브 2-client manual 검증 + release 빌드 후 sha256 4-path | Low-Medium (debug UX) |
| M16 | **Anthropic OAuth provider 가 hand-rolled JSON 으로 도구 광고** — 다른 provider (OpenAI/Gemini/Copilot/Cursor/Bedrock/Antigravity) 는 모두 `ToolDefinition.input_schema` (ToolRegistry single source of truth) 를 직접 직렬화해서 광고함. Anthropic OAuth 만 hard-coded JSON 11개 도구 화이트리스트 → stale schema 위험 영구 존재 (M12/M13 발생지). 근본 해결: Anthropic OAuth 도 ToolDefinition 기반 광고로 통일 | 🔴 OPEN — 코드 위치: `src/provider/anthropic.rs::format_tools` (is_oauth=true branch). 작업 범위: (a) ToolRegistry 에서 광고할 OAuth 화이트리스트 정의, (b) 도구 이름 매핑 (`bash`→`Bash` 등) 을 광고 단계에서 자동 적용, (c) cache_control breakpoint 로직 보존, (d) 회귀 테스트 — 광고 schema 가 모든 ToolDefinition.input_schema 와 항상 일치 | Medium-High (구조 개선, 미래 회귀 방지) |
| M17 | **Live turn handoff to swarm member (claude-code parity)** — main session 이 긴 turn 도는 동안 다음 요청을 보내면 queue 에 들어감. 이 queue 로 가는 흐름을 swarm 으로 바로 보내고 싶음. fork (turn snapshot 떠서 subagent 화) 는 race + conversation 분기로 복잡 → 대신 **queue 를 swarm 으로 라우팅** 하는 접근 선택. 결과 회수는 swarm completion + M6 idle wake path 가 자동 처리 | 🔴 OPEN — 미설계. **두 가지 옵션 후보**: **(A) queue cancel + swarm 재발행** — 사용자가 평소처럼 다음 메시지 enqueue, 그 후  로 queue 의 마지막 항목을 dequeue + swarm spawn. **(B) queue skip + direct swarm** —  슬래시명령 한 번에 queue 안 거치고 바로 swarm spawn (B1=one-shot, B2=sticky toggle 모드). **B1 이 가장 단순**: 기존 swarm spawn tool 이 이미 있으니 TUI 슬래시명령 한 줄 추가 ~50줄. 결과 회수: swarm completion notification → M6 wake path → 사용자가 보고 싶을 때 확인. **결정 필요한 디테일**: (a) A vs B 중 선택 (또는 둘 다), (b) sticky 모드 필요 여부, (c) main conversation 에 swarm 위임 흔적 표시 방식 (시스템 메시지 한 줄 vs 보이지 않게) | High (사용자 워크플로우) |
| M18 | **SDK `Client::get_history()` 가 image bytes 명시적으로 drop** (M15 의 가장 좁은 fix candidate B) — `src/server/client_api.rs:154-159` 가 `ServerEvent::History { messages, images, .. }` 에서 `messages` 만 반환하고 `images` 는 `..` 로 버림. SDK consumer 가 image 데이터 못 받는 직접 원인. M15 의 sub-set 으로 분리 — fix 가 한 함수 추가 (~10줄) 로 해결 가능 | ✅ **DONE** (commit `4531dc5d` fix + `fe7eb87e` test on `deploy/m9-m10`, source `baad5364`/`9051050b` on `patch/sdk-history-images`) — `Client::get_history_with_images() -> Result<(Vec<HistoryMessage>, Vec<RenderedImage>)>` 추가, `split_history_event(event) -> Option<(messages, images)>` helper 분리. 기존 `get_history()` 는 backward compat 유지. test 2건 (`server::client_api::tests::m18_*`) nightly 로 통과. **빌드/배포 주의**: stable rustc 1.90.0 은 upstream AWS crates MSRV 1.91.1 막혀서 `cargo +nightly` 필요 — release binary 도 nightly 로 빌드 | Low (작은 SDK 추가) |
| M19 | **Config hot-reload** — `~/.jcode/config.toml` 편집 시 `OnceLock<Config>` 가 옛 값을 캐시한 채로 process 종료까지 반영 안 됨. 장시간 떠 있는 TUI server 의 root cause 로, M9 hook fix 가 binary 는 새 것이지만 config 는 옛 것이라 효과를 못 보는 사례 발생 | ✅ **DONE** (commit `96445a80` fix + `da4fe48f` test on `deploy/m9-m10`, source `93282b44`/`bdf63f5c` on `patch/config-hot-reload`, binary `lazydino-da4fe48f`) — `OnceLock<Mutex<ConfigCache>>` + 매 호출 `stat()` + 500ms debounce + `Box::leak` (기존 `&'static Config` API 보존). last-good fallback 으로 toml 파싱 깨짐에 대해 default 로 안 떨어짐. test 5건 (`test_m19_config_*`) 모두 통과 (`config::tests::test_m19 → 5 passed`) | — |
| M20 | **jcode bash tool 의 2분 hard timeout** — `src/tool/bash.rs:23` 의 `DEFAULT_TIMEOUT_MS: u64 = 120000` 하드코딩 때문에 cargo test/build, 긴 sleep, stress 검증이 모두 2분에 강제 background 이동. nohup+disown 으로 process 자체는 살릴 수 있지만 호출 메시지가 끊겨 결과 폴링이 강제됨 | ✅ **DONE** (commit `28b524ba` fix + `e6f98968` test on `deploy/m9-m10`, source `11edbb91`/`10db6a96` on `patch/bash-tool-timeout`, binary `lazydino-da4fe48f`) — 새 `[tool.bash]` config 섹션 (`ToolConfig`/`BashToolConfig`) 도입. default 5min, max 20min (`HARD_CAP_MS`). `resolve_timeout_ms(requested)` helper 가 세 사용처 (foreground/agent-turn/background) 통합. schema description 에 5min default / 20min cap / `[tool.bash]` config knob 명시. test 5건 (`tool::bash::tests::test_m20_*`) 모두 통과 | — |
| M21 | **upstream rebase + dedupe** — `fork/master` 가 `origin/master (1jehuang/jcode)` 보다 226 commit 뒤짐. 그 226 안에 우리 fix 와 같은 영역의 commit 다수: `Move compaction emergency/estimation/contracts into core` (M14/M14a 영역), `Keep restored remote queues pending across reload` (M7 영역), 그리고 가장 위험한 대규모 "Extract/Move into core crates" refactor 30+건 — 우리 patch 들이 변경한 `src/*` 위치가 새 `crates/*-core/` 로 이동. fork 에 우리 작업 push 시도하면 `workflow` scope 부족 + (그 후) reapply 시 path mismatch 충돌 폭탄 가능 | ✅ **DONE** (2026-05-11) — PAT (repo+workflow scope, lazy-dinosaur) 갱신 완료. `fork/master` 를 `origin/master` 로 fast-forward (226 commits). deploy/m9-m10 + 48 patch branch 모두 fork 에 push (`fork`/`origin` 동기화 0 commits behind). dedupe rebase 는 불필요했음: 우리 코드 patch 들이 이미 origin/master 기준이고 deploy/m9-m10 도 그 위에 깔끔히 얹혀있어서 `git rebase --onto` 없이 그대로 push 가능. M14/M14a/M7 영역도 upstream commit 과 충돌 없이 push. 헬퍼 스크립트 `scripts/fork-push.sh` 추가 (`/.env` 의 GH_TOKEN 사용) | — |
| M22 | **Subagent same-round spawn defer** — main session 한 turn 안에서 subagent (`task`/`subagent` tool) 를 두 번 spawn 하면 두 번째가 즉시 실행 안 되고 background 로 deferred 됨. workaround: round 당 한 subagent 만 spawn. 이게 jcode 의 swarm/subagent dispatch 코드 버그인지, 의도된 직렬화인지 모름. 진단부터 필요 | ✅ **DONE — BY-DESIGN** (2026-05-11) — 라이브 재현 (`jcode run` 단발) + upstream/타도구 비교 (searcher 분석) 결과: **jcode 의 subagent (`SubagentTool`) tool 은 의도된 직렬 실행** 임. `src/agent/turn_streaming_mpsc.rs:752` `for tool_index in 0..tool_count` 로 한 번에 하나씩 await — upstream `1jehuang/jcode@50d2c68b` 도 동일 구조. 사용자가 보았던 "deferred" 메시지는 `[Skipped: '<tool>' was moved to background; remaining tools in this round are deferred]` (line 1081) 이고, 이건 **Alt+B detach 분기에서만** 발생. 동시 실행 (병렬 tool_use) 이 필요한 경우 **swarm 사용**: `try_join_all(task_futures)` (`src/server/swarm.rs:1029`) 가 진짜 병렬. opencode (`Effect.forEach concurrency=10`) / Claude Code (자동 parallel) / Codex CLI (per-tool capability lock) 는 일반 tool 병렬 지원하지만 jcode 는 swarm 으로 명시적 fan-out 만 제공. 일반 tool 병렬화는 **M24** 로 분리하여 추후 진행. | — |
| M23 | **Build artifact cleanup / retention policy** — release 빌드마다 `~/.jcode/builds/versions/lazydino-<hash>/jcode` 가 약 94 MiB 씩 누적. retention 없음. 매 milestone 마다 새 binary 만들어서 디스크 압박. 본 세션 종료 시점에 versions/ 안에 N 개 누적 중 | 🔴 OPEN — 미설계. 구현 후보: (a) `jcode build-cleanup --keep N` 보조 cli, (b) atomic deploy 스크립트 안에서 자동으로 N 개 초과분 삭제 (예: stable/current 가 가리키는 것 + 최근 3개 외 삭제), (c) systemd timer 로 주기적 cleanup. 위험: 옛 binary 로 rollback 하고 싶은 경우 cleanup 이 너무 공격적이면 곤란. 현 시점 우선순위 낮음 (디스크 충분), 디스크 80% 넘으면 즉시 등록 | Low (디스크 충분 동안) |
| M24 | **일반 tool 의 round-내 병렬 실행** — M22 의 결론에서 분리. 현재 `src/agent/turn_streaming_mpsc.rs:752` 의 `for tool_index in 0..tool_count` 가 모든 tool (subagent 포함, 일반 tool 도) 을 직렬로 await. opencode 는 `Effect.forEach concurrency=10`, Claude Code 는 자동 parallel, Codex CLI 는 per-tool capability lock 으로 병렬 가능. jcode 는 **swarm `try_join_all`** 만 진짜 병렬 (`src/server/swarm.rs:1029`) — 일반 LLM tool_use 묶음에서는 fan-out 안 됨. 같은 round 안에 `Bash + Read + Grep` 동시 실행 가능하면 디버그 사이클이 크게 줄어듦 | 🔴 OPEN — 미설계. 작업 범위: (a) **dependency 분석** — 같은 round 안 tool 들이 서로 의존성 있는지 (예: Write→Read 같은 file path 충돌) 정적으로 못 봄, 대부분 LLM 이 의도해서 한 묶음으로 호출하니 병렬 안전 가정 OK. (b) `tokio::join_all` 로 fan-out — subagent/swarm tool 은 제외 (이미 별도 처리). (c) **streaming UI 충돌** — 현재 single-tool 가정으로 tool_call 진행도 표시. 병렬이면 동시 진행 N 개 UI 필요. (d) **per-tool capability lock** (Codex 패턴) — `bash`+`bash` 동시는 위험 (mutex). 다른 tool 묶음은 OK. (e) 회귀 테스트 — 직렬 path 보존 (config knob `[tool] parallel_in_round=true`). 우선순위 Medium-High (디버그 사이클 효율) | Medium-High (디버그 효율) |
| M25 | **Swarm worker auto-cleanup / GC** — `jcode-server` 띄워놓고 swarm 으로 작업하다 보면 `swarm_members` 안에 status=`ready`/`completed` 인 worker 가 무한 누적됨 (현재 PID 1770980 server: `ready` 22개가 1.77h 누적 confirmed 2026-05-11). cleanup 명시적으로 호출 안 하면 안 빠짐 | 🟡 **PRE-CLOSED — BY-DESIGN** (2026-05-11 확정) — upstream `docs/SWARM_ARCHITECTURE.md` 의 두 정책에 의해 의도된 동작: (1) **"Completion Report Policy"** (line 67): coordinator 가 명시적으로 cleanup 호출할 ���까지 worker 유지 (재할당 가능 상태). (2) **"Communication" §** (line 171): *"Completed or idle agents do not resume automatically when notifications arrive. They only resume when the coordinator assigns new work, explicitly starts or wakes an assigned task, or respawns them."* → 즉 fire-and-forget 이지만 **회수는 coordinator 의 의도** 에 따름. **사용자 워크플로우**: 진짜 무한 누적이 문제라면 (a) `communicate cleanup` 명시 호출, (b) server `/restart` 로 in-memory state 비우기. 만약 fork 에서 자동 GC 가 정말 필요하면 `[swarm] auto_cleanup_after_secs = 3600` 같은 opt-in config 추가 가능 (현 시점 부재) — 새 M28 로 분리 가능. 현재는 by-design 으로 닫음 | — (BY-DESIGN) |
| M26 | **Swarm `await_completion` 동기 모드** — `swarm action="spawn"` 의 tool_result 가 `"spawned session_id, active=N/cap=M"` 메타데이터만 반환하고 worker 의 completion_report 를 직접 회수하지 않음. parent 가 보고 받으려면 별도 `Notification` (TUI 표시용) 으로만 옴. async/sync 옵션이 없어서 "여러 worker 작업 다 끝나면 결과 모아서 다음 turn 진행" 워크플로우 어려움 | 🟡 **PRE-CLOSED — BY-DESIGN** (2026-05-11 확정) — upstream `docs/SWARM_ARCHITECTURE.md` 동일 sections (Completion Report Policy + Communication line 171) 가 명확하게 fire-and-forget 으로 설계. **대안**: (a) main session 한 turn 안에서 결과까지 받고 싶으면 **`batch` 또는 `subagent` (Task tool)** 사용 — 둘 다 동기 await. (b) 진짜 병렬 + 비동기 회수 필요하면 swarm + M6 idle wake path (이미 구현) 가 worker 결과 도착 시 main session 깨움. 즉 jcode 에는 두 모드가 이미 명확히 분리: **swarm = fire-and-forget 병렬**, **subagent = sync 직렬**. 이 분리가 직관에 안 맞다면 fork 에서 새 `swarm action="await"` 또는 `await_completion=true` 옵션 추가 가능 — 새 M29 로 분리 가능. 현재는 by-design 으로 닫음 | — (BY-DESIGN) |
| M27 | **Pre-existing test fail — busy-agent history snapshot fallback 두 건** — 둘 다 `messages.len() == 0` (right=1 기대) 로 panic. 라이브 영향 가능성: agent 가 long turn 도는 동안 다른 jcode TUI/SDK 가 같은 session 으로 resume/attach 하거나 `get_history` 호출하면 **persisted history 가 비어 보일 수 있음**. baseline `0c27110d` (M15 작업 직전) 부터 이미 깨져 있어서 M15 와 무관 — git bisect 로 어느 commit 부터 깨졌는지 (M11 stage 6 의심 / M21 upstream merge 226 commits 의심) 확인 필요. 라이브 재현 안 함 → 실사용 영향 미확정 | 🔴 OPEN — 코드 위치: (1) `src/server/client_session_tests/resume/busy_existing_attach.rs:177` (resume + busy + live attach 경로), (2) `src/server/client_state_tests.rs:198` (`handle_get_history` 가 agent lock 못 잡았을 때 persisted snapshot 으로 fallback 경로). 두 테스트 모두 setup 에서 `session.append_stored_message("persisted ...")` 후 `session.save()` → 새 path 가 그 message 를 못 읽거나 빈 history 로 응답하는 회귀로 추정. 작업 순서 권고: (a) 직접 코드 read 로 `handle_get_history` persisted fallback path 가 messages 어디로 사라지는지 확인 (~10분), (b) 라이브 재현으로 실사용 영향 단정 (~5분), (c) 필요 시 git bisect (~20분) | Medium (실사용 영향 미확정 — 라이브 재현이 정확도 결정) |
| M28 | **Mermaid 렌더링 전체 미작동 — flowchart 가 텍스트 box 로만 표시됨 (이미지 변환 path 자체 disabled)** — 사용자 라이브 보고 (Round 16 Test 3, 2026-05-12). 초기 증상 진단은 "우하단 클리핑" 이었으나 실제 라이브 재현 결과 **mermaid 다이어그램이 아예 이미지로 렌더되지 않고** `┌─ mermaid` 박스 안에 raw `flowchart LR` 텍스트 그대로 표시. **근본 원인**: `crates/jcode-tui-mermaid/Cargo.toml` 의 `renderer` feature 와 `crates/jcode-tui-markdown/Cargo.toml` 의 `mermaid-renderer` feature 가 **둘 다 top-level `Cargo.toml` 에서 default 로 enable 되어 있지 않음** (`default = ["pdf"]` 만). upstream master 도 동일하게 opt-in 상태. install 스크립트 `scripts/lazydino/install-custom-jcode.sh:51` 은 `cargo build --release` 만 호출 → feature flag 없이 빌드 → 모든 `#[cfg(feature = "renderer")]` 코드 (`crates/jcode-tui-mermaid/src/lib.rs:39,84,233,259,409,618,768`, `mermaid_cache_render.rs:131,247,304,313,598` 등) 가 통째로 빠짐. **사전 결함** (M21 ver.2 회귀 아님 — backup branch `deploy-m9-m10-pre-catchup-20260511-225725Z` 도 동일 상태). 환경: Kitty terminal (`TERM=xterm-kitty`, image protocol 지원 OK), mmdc v11.12.0 + puppeteer cache 정상, 의존성 측면 문제 없음 | 🟡 **DEPLOYED, awaiting live verification** (commit `4a686023`, binary `ff5bd5cfa55b11c9...`, Round 16, 2026-05-12) — fix path (a) 채택: `Cargo.toml` 의 `default = ["pdf", "mermaid-renderer"]` + workspace-level `mermaid-renderer = ["jcode-tui-markdown/mermaid-renderer"]` 정의 추가, `crates/jcode-tui-markdown/Cargo.toml` 의 `mermaid-renderer` 가 `dep:jcode-tui-mermaid` 와 `jcode-tui-mermaid/renderer` 두 feature 를 propagate 하도록 정정. `cargo build --release --bin jcode` 통과, mermaid-rs-renderer v0.2.1 git dep 정상 fetch. 5-path sha256 동기 (`ff5bd5cfa55b11c9...`). 다음 검증: 사용자 라이브 mermaid 프롬프트 재현 → 이미지로 렌더되는지 + 우하단 클리핑 (원래 M28 의도, 별개 이슈일 수 있음) 도 여전히 발생하는지 확인 | High (feature 전체 사용 불가) |
| M29 | **Test 격리/순서 의존성 오염 — 39 → 23 fail (16건 fix됨, 부분 진행)** — `cargo test --lib` parallel 시 다수 fail. 단독 실행 시 PASS (대부분). 즉 **테스트 순서/parallel 시 글로벌 state 오염** (env vars + 글로벌 cache + Bus broadcast subscriber). **사용자 영향: 0** — release binary 동작 영향 없음, CI 만 noisy | 🟡 **PARTIAL** — Round 24: provider:: 영역 2건 fix 적용 → 광역 lib parallel fail 39 → 23 (16건 해소). **Commits**: (1) `753fc750 fix(m29): drop file-local ENV_LOCK; align anthropic disk-cache test` — `src/provider/openrouter_tests.rs` 의 file-local `static ENV_LOCK: Mutex<()>` 제거하고 process-wide `crate::storage::lock_test_env()` 로 일괄 교체 (PoisonError 폭포 cascade 의 root cause), + `src/provider/tests/catalog_subscription.rs` 의 `test_anthropic_model_catalog_hydrates_from_disk_cache` 가 `claude-opus-4-7` hard-coded 200_000 branch 에 short-circuit 되던 문제를 fake model id (`claude-disk-only-model`) 로 우회. 검증: `provider::` 전체 parallel 307/307 PASS (이전 17 fail). **잔존 23건** 은 별도 M37 로 분리 (cli/session/side_panel/server/bus 등 다양한 모듈, 단독 PASS 대부분 + cli::commands 1건은 단독에서도 라이브 환경 의존). M21 ver.2 합성의 `notify_config_reloaded()` Bus broadcast 가설은 미확인 (M37 진단 항목). 우선순위 Medium (deploy blocker 아님, CI hygiene 만) | Medium (사용자 영향 0, CI 만) |
| M30 | **Background task 완료 알림 누락** — `nohup ... &` 로 띄운 bg task 가 `notify=true` 로 등록되어 `status=completed, exit_code=0` 가 23:39:02 에 file 에 기록됐는데, jcode session 으로의 wake/notify 가 안 와서 사용자가 23:39:39 (37초 후) 직접 물어보고 나서야 발견. 사용자 보고 (2026-05-11 round 14). `event_history` 에는 `kind: completed, status: completed` 1개만 — kind=`wake` 또는 `notify` 이벤트 없음 → harness 의 wake 전달 path 가 트리거 안 됨. **사용자 영향: 워크플로우 disruption** (사용자가 매번 손으로 진행 상황 물어봐야 함, autonomy 저하) | 🔴 OPEN — 코드 위치: 백그라운드 task spawn → wake 전달 path (Jcode harness 내부). 진단: (1) `notify=true` 가 실제로 wake event 를 enqueue 하는지 (2) bg ProcessFinished → session wake 매핑 (3) session 이 quiescent (LLM waiting for user) 상태일 때 wake 가 dropped 되는지. 우선순위 Medium-High (autonomy 영향 직접) | Medium-High (autonomy + UX) |
| M31 | **Background tool 결과의 LLM 자동 주입 (auto-enqueue)** — bg task 완료 시 stdout/exit_code 를 LLM 컨텍스트에 inject 하고 다음 inference turn 을 자동 trigger. 사용자 round 14 명시: "릴리즈하고 bg 에서 돌리는 툴 결과를 너가 바로 받아서 결과로 사용해야 하는데 그것도 안 됐고" | 🟢 **DONE Round 23** (commits `a459b0a1` wake channel + `b085caef` auto-inject 본구현, M21 이후 rebase 포함). 구조: `src/turn/bg_completion.rs` (BackgroundCompletion mpsc channel + `enqueue_bg_completion_injection`), `src/background.rs:206,477` (bg 완료 시 `send_bg_completion` 발사), `src/server/client_lifecycle.rs:1063,1222` (TUI client connection 마다 `register_bg_completion_receiver` → `recv` → `enqueue_bg_completion_injection` → `start_processing_message` 로 LLM turn auto-trigger). `bg`/`bash` tool 의 `auto_inject` (default true), `auto_inject_format` (default `system_reminder`), `auto_inject_max_bytes` field 노출. Unit 9/9 PASS. **라이브 검증 PASS Round 23** (사용자 confirm "잘된다"): tmux + 새 TUI 세션에서 `bash run_in_background=true notify=true wake=true` 로 sleep+echo 마커 실행 → `[bg/inject] enqueued completion task_id=997266nodw session_id=session_snake_...` 로그 + 자동 turn 확인. **한계 (의도된 동작, M31 scope 밖)**: receiver 는 `client_lifecycle.rs::handle_client` 의 TUI socket connection loop 안에서만 등록 → `jcode run` single-shot 이나 `debug create_session` headless 모드에서는 client 연결이 짧아 `[bg-completion] no receiver` 로그 후 drop. TUI client 가 활성 상태일 때만 inject 발생. | High (autonomy 직접) |
| M32 | **Assistant streaming events (`TextDelta`/`ToolStart`/`ToolDone`/`MessageEnd`/`Done`) 의 sibling client fanout 누락 — bg wake/sibling attach 환경에서 응답이 보이지 않는 증상** — 사용자 라이브 보고 (2026-05-11 round 14): Alt+B 로 bg 전환된 task 가 끝나고 system-reminder 가 들어와 assistant continuation turn 이 정상 trigger 되었으나 **UI 에 응답이 렌더링되지 않음**. 백엔드 흐름은 정상 (`bg complete` → `system-reminder` insert → assistant turn → text generated) 인데 frontend 가 못 받음. 근본 원인: `src/agent/turn_streaming_mpsc.rs:260` (그리고 `turn_streaming_broadcast.rs:261,270,288`) 에서 LLM streaming 이 단일 `event_tx: mpsc::UnboundedSender<ServerEvent>` 로만 보냄 — 즉 **agent turn 의 모든 ServerEvent (TextDelta/ToolStart/ToolExec/ToolDone/GeneratedImage/TokenUsage/MessageEnd/Done 등) 가 "이 turn 을 시작한 single connection" 한 곳에만 가고, 같은 session 에 attach 한 sibling client 의 `event_txs` map 에는 broadcast 안 됨**. M11 (multi-attach) / M15 (user-message fanout) 가 자가-attach + user message 까지만 해결, **assistant turn streaming** 은 안 했음. 사용자 영향 직접: ambient/swarm/comm_task 등 bg-trigger turn 의 응답을 다른 attached client 가 못 봄 | 🟢 **DONE Round 23 (fanout 코드는 deploy 됨)** + 🟡 **PARTIAL — TUI mirror 는 BY-DESIGN 으로 미지원** (Round 25, 2026-05-12 재확인). Fix Round 23: `src/server/client_lifecycle.rs::start_processing_message` 에 fanout wrapper task 추가 (commit `97d58e67`). agent turn 의 모든 `ServerEvent` 를 origin client + `fanout_session_event_except` 로 sibling 에 broadcast. 회귀 21/21 PASS. **그러나 라이브 두 TUI mirror 검증 결과 (Round 25)**: 사용자 환경에서 `jcode --resume <id>` 로 두번째 TUI 띄우면 첫번째 TUI 가 **takeover 로 disconnect** 됨 → sibling 동시 존재 자체가 안 됨. upstream `docs/MULTI_SESSION_CLIENT_ARCHITECTURE.md` 명시: *"v1 should prefer a single active controller per session"* + Non-Goal: *"Supporting fully concurrent editing from multiple interactive attachments to the same session in the first version"*. `src/server/client_session.rs::handle_resume_session:798-846` 가 conflict 발견 시 `disconnect_tx.send(())` 로 기존 client 쫓아냄 — 의도된 takeover semantics. **현재 fanout 코드의 실제 효과**: SDK client / swarm worker / debug attach 등 takeover 분기를 안 타는 path 의 sibling 회수 (예: M38 의 swarm worker report → parent UI 가 이 fanout path 로 도착) 에 한정. 두 TUI 동시 mirror 는 upstream v1 design 으로 **BY-DESIGN 미지원**. 사용자 결정 (Round 25): **(A) upstream 의도대로 닫음** — 두 TUI 동시 attach 는 takeover, mirror 는 v1 non-goal. M32 fanout 코드는 SDK/swarm/debug fanout 용으로 유지. 진짜 mirror 가 필요해지면 새 milestone (`--follow <session>` read-only stream 등) 으로 분리 등록. | — (BY-DESIGN, fanout 코드는 SDK/swarm fanout 용으로 유지) |
| M33 | **이미지 클립보드 paste UX — "Reading clipboard..." 만 뜨고 사라짐, 실패/이유 표시 없음** — 사용자 라이브 보고 (Round 16 Test 2, 2026-05-11). `src/tui/app/input.rs:124-155` 의 `paste_image_from_clipboard` 가 `clipboard_image()` 결과 None 또는 클립보드에 image MIME 없으면 status notice 만 잠깐 띄우고 조용히 abort. 사용자는 "이미지가 첨부될 것" 으로 기대했는데 텍스트만 있어서 silent fail. **재현 환경**: Wayland (`WAYLAND_DISPLAY=wayland-1`), `wl-paste --list-types` 결과 `text/plain` 만 있을 때 paste image 시도. 사용자 영향: 디버그 사이클 — 이미지 첨부 실패 원인 (실제로는 클립보드에 image 없음) 을 사용자가 추측해야 함 | 🔴 OPEN — fix 후보: (a) `clipboard_image()` 가 None 반환 시 status notice 를 `"클립보드에 이미지가 없습니다 (text only)"` 로 명시 + 2-3초 유지, (b) 실패 원인 enum (`NoClipboard | NoImageMime | DecodeFailed | UrlDownloadFailed`) 추가하여 each case 마다 다른 메시지, (c) 디버그 모드에서 `wl-paste --list-types` 출력 status notice 에 포함. 우선순위 Low (기능은 정상, UX 만) | Low (UX) |
| M34 | **`schedule` (ScheduleTool) 의 광고 schema field 와 deserialize struct field 이름 mismatch** — LLM 라이브 보고 (Round 16 Test 6, 2026-05-12 ambient wake 테스트 중). 첫 호출 `error: [schedule] Error: invalid type: null, expect...`, 두 번째 호출 우연히 성공. 진단 (코드 read 로 확정): `src/tool/ambient.rs::ScheduleTool::parameters_schema` 는 `"properties": { "task": {...} }` 와 `"required": ["task"]` 로 광고하는데, `src/tool/ambient.rs::struct ScheduleInput` 의 실제 필드는 `context: String` (required). serde_json::from_value 시 `task` 가 들어오면 `context` 필드 누락으로 `invalid type: null, expected string` 에러 발생. **두 번째 호출이 성공한 이유**: LLM 이 retry 하면서 schema 와 무관하게 `context` 키를 명시적으로 보냈을 가능성 (또는 다른 ScheduleAmbientTool dispatch 로 라우팅됨). 추가로 `deserialize_string_or_option_u32` 함수가 같은 파일에 **두 번 정의됨** (line 53 외 다른 곳, build warning 후보). M13 (anthropic OAuth `ScheduleWakeup` 광고 schema align) 이 이 generic path 는 못 막음 — M16 (Anthropic OAuth 광고를 ToolDefinition 으로 통일) 의 근본 해결 필요성 또 한 번 확인됨 | 🟡 **DEPLOYED, awaiting live verification** (commit `4a686023`, binary `ff5bd5cfa55b11c9...`, Round 16, 2026-05-12) — 양방향 alias 확장: `ScheduleInput.context` 에 `#[serde(alias = "task")]`, `ScheduleToolInput.task` 에 `#[serde(alias = "context")]` 추가 → LLM 이 두 ambient/schedule 도구를 헷갈려도 transparent 하게 deserialize. unit test 2개 (`m34_schedule_tool_input_accepts_context_alias`, `m34_schedule_ambient_input_accepts_task_alias`) 추가 + PASS. Round 16 의 deserialize_string_or_option_u32 중복 정의 보고는 grep 재확인 시 단일 정의 (line 53) 만 존재 — false positive, 정정. 추가로 진단 결과: 사용자가 본 `invalid type: null, expected string` 의 정확한 trigger 는 LLM 이 `schedule_ambient` 를 의도하면서 `task` 키만 보냈을 때 또는 그 역케이스로 추정 — alias 가 양쪽 모두 흡수. (b) (M16 ToolDefinition 단일 출처) 는 별도 milestone 으로 유지  | Medium-High (LLM 자주 사용) |
| M35 | **Lifecycle hook 결과 (stdout/decision) 의 LLM 다음 turn 자동 주입 — feedback loop 미완성** — 사용자 라이브 보고 (Round 16 Test 4 후속, 2026-05-12): "lifecycle 훅은 잘 되는것 같은데 이거 문제가 훅을 다시 결과로 받아서 진행하는게 중요한거지??". 현 상태 분석 (코드 read): (1) `tool.execute.before` 는 `HookDecision { action: deny, reason }` → tool 차단 + reason 이 tool_result 로 LLM 에 ✅ 반환 OK. (2) `response.completed` 는 deny → `inject_lifecycle_reminder_for_continuation` → 다음 user message 로 reminder ✅ M11 stage 6 OK. (3) **그러나 `tool.execute.after` stdout 은 LLM 에 절대 안 들어감** (`run_tool_hooks` 가 result 만 hook 에 넘기고 stdout 회수 안 함) — hook 으로 tool output 검증/추가 정보 부착 불가. (4) **`session.stop` / `client.disconnect` stdout 도 무시됨** — 종료 직전이라 어차피 의미 없음. (5) `pre_tool_use` 의 decision 이 `allow` / `deny` 만 — `modify` (tool input 수정), `inject` (LLM 에 추가 컨텍스트 주입) 같은 풍부한 action 없음. (6) `blocking=false` hook 은 결과를 기다리지 않음 — async 회수 path 부재 | 🔴 OPEN — 설계 옵션: (a) `tool.execute.after` 의 hook stdout 을 옵셔널 system-reminder 로 다음 turn 에 inject (`HookDecision` 에 `inject: Option<String>` 필드 추가), (b) `pre_tool_use` decision 에 `modify` action 추가 — 새 `modified_input: Value` 로 tool input 교체, (c) `response.completed` 에도 `inject` action 추가 (deny 가 아닌 단순 컨텍스트 추가), (d) async hook 결과 회수: nonblocking spawn 의 completion 을 다음 turn 시작 전 collect 하여 컨텍스트 첨부. 우선순위 High (M31 의 bg tool 자동 주입과 같은 결의 autonomy 핵심). **Round 16 라이브 후속 검증 (2026-05-12)**: 사용자가 "subagent 결과가 바로 리턴 안 되던 문제도 있고" 언급 → 코드 read 로 확인 결과 **현재 subagent 결과는 정상 sync return** (`src/tool/task.rs:475` `agent.run_once_capture(&prompt).await` → `final_text` → `ToolOutput::new(output)` 로 LLM 에 즉시 반환). 사용자가 기억하는 "안 리턴" 케이스는 (가) M1/M5 시절 옛 race fix 됨, (나) Alt+B detach 시 background 로 옮겨지면 turn 종료 = M8 의도된 동작, (다) `output_mode="answer"` 때 thinking 안 보이는 UX. **즉 subagent 결과 path 는 OK, 진짜 missing 은 `tool.execute.after` (subagent 포함 모든 tool) hook stdout 의 LLM 주입**. 또한 사용자가 "subagent 여러 개 동시" 언급 → M22 BY-DESIGN (직렬) 으로 이미 정리, 일반 tool 병렬화는 M24 분리 등록 완료. **Round 22 (2026-05-12) DEPLOYED + 라이브 검증 PASS + cap 동작 정확성 확인**: `4a87de32` binary (`fix(m35): wire hook inject into ContinueImmediate path like M11 deny`) 에서 hook stdout `HOOK_ACK_<ts>` directive 가 system-reminder 로 inject + 사용자 입력 없이 LLM 자동 다음 turn 동작 3 회 연속 PASS (스크린샷 confirm). `/tmp/jcode-response-completed.log` 의 rooster session 발화 패턴 분석으로 cap=3 정확 검증: user message 1 회당 fire 4 회 (1 initial `stop_hook_active=false` + 3 continuation `stop_hook_active=true`) 후 cap exceeded → 자동 turn stop 정상. **claude-code 표준 비교**: Anthropic 공식 Stop hook spec (https://docs.anthropic.com/en/docs/claude-code/hooks) 은 cap 없음 — `stop_hook_active` 만 제공하고 script 가 직접 체크 self-throttle 책임. jcode 는 `max_lifecycle_deny_streak=3` default 로 더 안전, `max_lifecycle_deny_streak=0` 시 claude-code trust mode 호환. 사용자 의문 "예전엔 없던 문제 — 한 user turn 에 hook 4-5 회 fire" 는 **M35 fix 가 도입한 의도된 동작**: 이전엔 hook stdout 이 LLM 에 안 들어가서 자동 turn 0 → fire 1 회; M35 후엔 cap 안에서 자동 turn N 회 → fire N+1 회. 버그 아님 ✅. **Round 22 후속: default cap 3 → 0 (claude-code 호환 trust mode) 변경 + self-throttle demo hook 라이브 PASS** (commit `65c52089`, binary `d7a5094dc45f59aa`): (a) `src/agent/turn_loops.rs` 의 `DEFAULT_MAX_LIFECYCLE_DENY_STREAK: u8 = 3 → 0`, (b) `src/config/default_file.rs` 안내문 갱신 + `jq -r '.stop_hook_active' <&0` self-throttle 예시 추가, (c) `~/.jcode/hooks/m35-self-throttle-demo.sh` 신규 demo: `stop_hook_active=true` 면 stdout 없이 종료, `false` 면 `{"action":"allow","inject":{"body":"HOOK_ACK_<ts> ...","format":"system_reminder"}}` JSON wire format 으로 stdout 출력. **사용자 라이브 검증 PASS** (elephant session): user "안녕" → turn 1 → fire #1 (active=false) inject → 자동 turn 2 (HOOK_ACK 포함) → fire #2 (active=true) skip → 자동 turn 정지. cap 없이도 hook script self-throttle 만으로 정확히 1회 continuation 시연 완료. (참고: 초기 plain-text stdout 시도는 inject path 안 탐 → `decision.inject.as_ref()` JSON wire format 필수임을 확인하고 fix). 옛 `tee /tmp/jcode-response-completed.log` hook + 새 self-throttle demo hook 둘 다 활성 상태로 유지 | High (DONE — M35 lifecycle hook inject + wake + claude-code 호환 default cap=0 + self-throttle demo 모두 라이브 PASS) |
| M36 | **Hot-reload tests fail in isolation — debounce 호환성 버그** — `cargo test --lib` 환경/격리 노이즈를 분류하다 발견. upstream `e8f17de6 Reload config cache on file changes` (M21 ver.2 squash 로 들어옴) 가 추가한 두 테스트 `global_config_cache_reloads_after_manual_file_edit` + `cached_external_auth_trust_observes_manual_revocation` 가 **단독 실행에서도 deterministic FAIL** — `config()` 의 500ms stat debounce 와 manual `fs::write` 후 즉시 read 패턴이 호환되지 않음. M29 의 광역 환경 의존 FAIL 과는 별개. 사용자 영향: 0 (release binary 동작 영향 없음, CI hygiene 만 noisy) | ✅ **DONE** (commit `72df53fc`, branch `deploy/m9-m27-catchup`) — test 가 manual fs::write 직후 `crate::config::force_reload_config()` 호출해서 debounce bypass. 두 테스트 + 전체 `config::tests` 53/53 PASS 확인 | — |
| M37 | **잔존 23건 광역 cargo test --lib parallel fail — 다양한 모듈의 env/glob state 의존** — M29 partial fix 후 잔존. 모듈 분포 (라이브 사용자 환경: openrouter/copilot creds 없음, 즉 환경 의존 test 가 fail): `provider::openrouter::*` 4건 (env race 잔존 source), `session::tests::cases::*` 5건 (JCODE_HOME 의존), `side_panel::*` 2건, `bus::tests`, `cli::commands::*` (단독 FAIL 1건 포함), `server::provider_control::*`, `provider::bedrock`, `tool::ambient`, `sidecar`, `soft_interrupt_store` 등 다양. 단독 PASS: server::provider_control, session::tests, tool::ambient, bus::tests, side_panel, provider::bedrock, soft_interrupt_store (M29 패턴 그대로). **단독 FAIL**: `cli::commands::tests::configured_auth_test_targets_only_include_configured_supported_providers` — production `state_for_provider` (`src/auth/mod.rs:273`) 가 OpenRouter/OpenAiApiKey/Azure/Bedrock/OpenAiCompatible 분기에서 self storage 가 아닌 라이브 환경 (`api_key_available("OPENROUTER_API_KEY", "openrouter.env")` 등) 을 다시 probe → 사용자가 안 쓰는 provider creds 가 없으면 `is_configured()=false` → test 의 mock `AuthStatus` 무시되고 라이브 환경에 dependent. 사용자 핵심 지적 (Round 24): **"openrouter/copilot 안 쓰는데 왜 자꾸 잡히는지 이해가 안 가" — 명백한 unit test 격리 위반**. **사용자 영향: 0** (release binary 동작 무관, CI 만 noisy) | 🔴 OPEN — 코드 분석으로 fix path 두 개 식별. **(A) Production fix (권장, 작은 변경)**: `state_for_provider` 의 OpenRouter/OpenAiApiKey/Azure/Bedrock/OpenAiCompatible 특수 분기를 제거하고 일반 `_ => self.state_for_key(provider.auth_state_key)` fallback path 로 떨어뜨림. 그러면 self.openrouter/openai/azure/bedrock 등 **이미 probe 함수가 채운 storage field** 를 신뢰 → mock test 와 production 둘 다 일관. 호출자 분석 (Round 24): 모든 호출자가 `AuthStatus::check()`/`check_fast()` 후 사용 → probe �� 먼저 실행 → storage 가 라이브 환경 정확히 반영 → 안전. 위험: probe 안 한 default `AuthStatus` 가 사용되는 경로가 있다면 ⇒ 라이브 환경 변경 못 감지. (단 grep 결과 그런 경로 없음 확인). **(B) Test-only fix (안전하지만 광범위)**: 모든 API_KEY env vars + config file paths tempdir 격리, openrouter/anthropic/gemini/copilot 등 모든 assertion 의 mock 일관성 재작성. **권장 순서**: 다음 세션에서 (A) 를 별 patch branch 에 적용 → 단독+parallel 검증 → reviewer 확인 → 사용자 commit 승인. 그 후 나머지 22건 (`session::tests` JCODE_HOME isolation, `provider::openrouter::*` 남은 env race source, `bus::tests` per-test reset 등) 을 모듈별로 sweep. 우선순위 Medium (deploy blocker 아님) | Medium (사용자 영향 0, CI hygiene) |
| M38 | **Swarm worker → parent completion report body 전달 누락 — status-unchanged 두 번째 report 가 fanout 게이트에 막혀 사라짐** — 사용자 라이브 보고 (Round 24, 2026-05-12 19:48 UTC): "swarm 의 작업이 끝나거나 부모에게 전송이 되어야할 정보들이 전달이 안 되고있는거잔아". swarm DM (`action=dm`) path 는 정상, swarm worker `action=report` 의 큰 audit body 만 parent UI 로 안 옴. 근본 원인 (코드 read + unit reproduction 확정): `src/server/swarm.rs::update_member_status_with_report` 의 `should_notify_coordinator` 가 **`status_changed` 만 게이트** 로 사용. worker 가 첫 짧은 report 로 `running→ready` 전이한 후, 같은 ready 상태에서 full body 를 다시 보내면 `status_changed=false` → fanout 안 됨. body 는 `member.latest_completion_report` 에 덮어쓰기만 되고 거기 갇힘. `CommReport` dispatch (`client_lifecycle.rs:479-512`) 가 이 한 경로에만 의존하므로 프로토콜 수준 빈틈으로 노출 | ✅ **DONE** (commits fix `3671fae7` + test `a562067e`, branch `deploy/m9-m27-catchup`) — `should_notify_coordinator` 에 `report_changed` 분기 추가: `(report_changed && report_back_to_session_id.is_some() && matches!(status, "ready"\|"failed"\|"stopped"\|"completed"))`. `report_changed = completion_report.is_some() && member.latest_completion_report != completion_report` 이므로 **같은 body 재발사 자동 dedup** — spam 위험 없음. 회귀 테스트 2건: `update_member_status_with_report_notifies_when_only_body_changes` (라이브 시나리오 reproduction, fix 전 FAIL `got events: []`, fix 후 PASS), `update_member_status_with_report_dedups_identical_body` (재발사 시 추가 fanout 0 보장). 회귀 합계 73건 (server::swarm 17 + client_lifecycle 6 + comm_control 19 + comm_session 31) 0 fail. 세션 문서: `docs/lazydino/sessions/2026-05-12-m38-swarm-report-delivery.md` | High (라이브 차단) |

⚠️ **운영 노트** (2026-05-11 갱신): 모든 fix 가 binary 까지 깔려도 **이미 띄워진 jcode-server process 는 옛 binary 를 메모리에 들고 있음**. 새 patch 효과를 보려면 사용자가 외부 터미널에서 `pkill -9 -f "jcode .* server"` (또는 `/restart`) 후 새 binary 로 재기동 필요. 5-path sha256 검증 protocol: `target/release` + `~/.jcode/builds/versions/<name>` + `stable` + `current` + `~/.local/bin/jcode` (모두 `readlink -f` 사용, `-L` 아님).

핵심 인과 관계 (Round 16 끝, 2026-05-12):
- ✅ M1, M3, M5, M6, M7, M8 — 기존 fix 모두 새 binary 에 통합 유지.
- ✅ M11 (multi-attach), M15 (user-message fanout) — sibling attach + user message 까지 해결. 단 **assistant streaming 은 M32 로 새로 발견** (M11/M15 가 못 봤던 영역).
- ✅ M21 ver.2 (round 15) — 130 lazydino + 178 새 upstream commit squash-rebase, 174 file / +20581/-723 lines.
- 🟡 **Round 16 DEPLOYED, awaiting live verification** (binary `ff5bd5cfa55b11c9...`, 5-path 동기):
  - **M28** mermaid renderer feature default 활성화 + propagate fix.
  - **M34** schedule/schedule_ambient 양방향 `#[serde(alias)]` + unit tests.
- 🔴 **OPEN 우선순위 정렬** (Round 16 끝 시점, M28/M34 deploy 대기 제외):
  1. **M32** (High, 사용자 라이브 회귀) — assistant streaming 이 sibling client 에 broadcast 안 됨. bg wake/ambient wake 모두 영향.
  2. **M31** (High, autonomy 핵심) — bg tool 결과의 LLM 자동 주입 없음.
  3. ~~**M35**~~ ✅ **DONE (Round 22, 2026-05-12)** — lifecycle hook 결과의 LLM 다음 turn 자동 주입 (feedback loop 완성). `65c52089` binary 라이브 PASS + Round 22 에서 default cap 3 → 0 (claude-code 호환 trust mode) 변경 + self-throttle demo hook 라이브 PASS. **문서**: 설계/history `docs/M35_LIFECYCLE_HOOK_INJECT.md`, 사용 가이드 `docs/HOOKS_USER_GUIDE.md`. M31 과 공통 mechanism 재사용 가능 (`ContinueImmediateWithInject` 패턴).
  4. **M30** (Medium-High) — bg `notify=true` 가 실제 wake 전달 안 함. M32 fix 일부와 겹칠 가능성.
  5. **M17** (High, 사용자 워크플로우) — main↔swarm queue 라우팅.
  6. **M24** (Medium-High) — 일반 tool round-내 병렬 실행 (M22 후속).
  7. **M16** (Medium-High) — Anthropic OAuth 광고 schema 의 ToolDefinition 통일 (M13/M34 와 같은 결).
  8. **M27** (Medium) — busy-agent history fallback 두 건 (pre-existing).
  9. **M29** (Medium, 사용자 영향 0) — test 격리/순서 의존성 (CI hygiene).
  10. **M33** (Low) — 이미지 클립보드 paste UX (silent fail 시 명시).
  11. **M23** (Low) — build artifact retention.

---

## 📍 Milestone M1 — Background task delivery 가 parent/report-back chain 을 안 따라감

상태: **🔴 OPEN — 이중 검증 완료 (2026-05-10), 수정 미착수**
우선순위: **High**

### 증상

A. **"No output captured"**: bg 로 옮긴 long task (subagent 포함) 가 정상 종료(`exit 0`)했는데 부모 TUI 카드가 출력을 잡지 못하고 부모 turn loop 가 깨어나지 않음.

B. **"Moving tool to background..." 가 풀리지 않음**: Alt+B 직후 status notice 가 clear 되지 않은 채로 멈춰 보임. (단, 이 증상의 일부는 M5 의 race 때문에 Alt+B 자체가 처리되지 못한 케이스도 섞여 있음.)

### Root cause (oracle 이중 검증, 2026-05-10)

이전 가설인 "내장 subagent Alt+B 시 `adopt(&self.session.id, ...)` 의 `self.session.id` 가 자식 세션 id" 는 **반박됨**. 내장 subagent tool 의 child agent 는 `run_once_capture` 로 돌고 `run_turn_streaming_mpsc` 를 쓰지 않으며, Alt+B 는 parent turn 의 session id 로 adopt 한다.

**정정된 root cause**: background task delivery 가 "작업 실행 session" 과 "알림/회수 대상 session" 을 분리하지 않고 단일 `task.session_id` 만 사용한다. 다음 두 가지가 합쳐서 깨짐:

1. **`fanout_session_event` 가 parent chain 을 안 따라감** — `src/server/state.rs:321~351`. `Session.parent_id` 도 `SwarmMember.report_back_to_session_id` 도 무시하고 단일 id 만 lookup.
2. **`run_background_task_message_in_live_session_if_idle` 의 live 판정 오류** — `src/server/background_tasks.rs:14~36` 의 `!member.event_tx.is_closed()` 체크가 headless drain task 때문에 항상 true. → headless/child session 이 background completion message 를 자기가 흡수해서 parent wake 가 막힘.
3. **Alt+B adopt 의 `wake=false` 기본값** — `src/background.rs:433~449`. 설사 알림이 도달해도 부모 turn loop 자동 재개 안 함. 모델이 명시적으로 `bg wait` 호출해야 하지만 그때는 이미 부모 turn 이 1 회 응답 후 idle.

이 셋이 합쳐져서 swarm worker / headless 세션에서 발생한 background task 의 completion 이 parent TUI 로 도달하지 않음.

### 회귀 여부
- **upstream 부터 존재한 설계 미흡** (특히 #2, #3). 우리 patch 가 만든 회귀 아님.
- bash 같은 self-session bg task (parent==self) 는 같은 fanout 이 자연 도달하므로 잘 보이지 않았음.

### 결정적 코드 근거 (oracle 검증)
- `src/server/state.rs:321~351` `fanout_session_event` — parent/report-back resolution 부재.
- `src/server/background_tasks.rs:14~36` `run_background_task_message_in_live_session_if_idle` — headless drain 을 live 로 오판.
- `src/server/background_tasks.rs:85~151` dispatch 들이 `task.session_id` 단일 lookup.
- `src/server/headless.rs:128~157` headless `SwarmMember { event_txs: empty, event_tx: drain }` — drain channel 이 항상 open.
- `src/background.rs:433~449` `adopt` 기본 `wake=false`.
- `src/tool/task.rs:374~475` 내장 subagent 가 `run_once_capture` 로 실행 — parent turn 의 mpsc 와 분리됨.
- `src/tui/app/local.rs:323~330` TUI 가 `task.session_id != app.session.id` 면 카드 표시 자체 거부.

### upstream PR/issue 조사 (2026-05-10)
- **동일 fix PR/issue 없음.** closed issue **#12 "Background task completion notifications"** 는 일반 bash bg 알림 요구로 다른 문제.

### 해결 방향 (oracle 권고)

`adopt` 한 줄 수정이 아니라 **delivery target 개념 도입** 이 정답:

1. `BackgroundTaskStatusFile` / `BackgroundTaskCompleted` / `BackgroundTaskProgressEvent` 에 선택적 `delivery_session_id` (또는 `owner_session_id`) 추가.
2. `BackgroundTaskManager::adopt` 호출 시 delivery target resolution:
   - 현재 session 이 headless 이고 `SwarmMember.report_back_to_session_id` 가 있으면 그 id.
   - 아니면 `Session.parent_id` chain 에서 live attached session 탐색.
   - 최종 fallback 은 현재 session_id.
3. `dispatch_background_task_completion` / `dispatch_background_task_progress` 가 `task.session_id` 대신 resolved delivery target 으로 fanout/wake.
4. `run_background_task_message_in_live_session_if_idle` 의 live check 를 `event_txs` 중심으로 변경. headless `event_tx` drain 은 live client 로 보지 않음.

선택적: Alt+B adopt 시 `wake=true` 정책 (별도 옵션 또는 task type 별).

### 회귀 위험 체크리스트
- [ ] self-session bash bg task 의 기존 notify 동작 보존.
- [ ] headless worker 에게 일부러 broadcast 하던 내부 흐름 보존 (예: ambient runner 의 background notification).
- [ ] `wake=true` 적용 범위가 너무 넓어지지 않게 — 예상치 못한 모델 turn 재개 폭주 방지.
- [ ] parent 가 detach 된 상태일 때 soft interrupt queue fallback 확인.

### 검증 시나리오
1. 내장 `subagent` tool Alt+B → `BackgroundTaskCompleted.session_id` (또는 `delivery_session_id`) 가 parent 인지 확인.
2. headless child `{ event_txs: empty, report_back_to_session_id: Some(parent) }` 에서 background 발생 → parent attached `event_txs` 로 notification 도달.
3. `event_txs: empty`, `event_tx: open(drain)` 인 headless member 에 대해 `run_background_task_message_in_live_session_if_idle(child)` 가 `false` 반환.
4. self-session bash bg task: 기존 `background_task_notify_without_wake_does_not_queue_soft_interrupt`, `background_task_progress_notifies_attached_clients` 회귀 없음.

### 관련 파일
- `src/server/state.rs` `fanout_session_event`, `session_event_fanout_sender`, `SwarmMember`
- `src/server/background_tasks.rs` 전체
- `src/server/headless.rs:128~157`
- `src/background.rs` `adopt`, `BackgroundTaskStatusFile`
- `src/bus.rs` `BackgroundTaskCompleted`, `BackgroundTaskProgressEvent`
- `src/agent/turn_streaming_mpsc.rs:980~1018` (Alt+B adopt 호출 지점)
- `src/agent/turn_streaming_broadcast.rs` 동일 구조
- `src/tool/task.rs:374~475` (내장 subagent fork)
- `src/tui/app/local.rs:323~330` (`handle_background_task_completed`)

---

## 📍 Milestone M2 — Swarm 버그 수정

상태: **✅ DONE — Stages 1-4 complete (worker heartbeat + opt-in timeout까지 적용)**
우선순위: **Medium-High**

### 알려진 단서
- `LAZYDINO_MAINTENANCE.md` 에 이미 적용된 swarm 패치들:
  - `swarm-stability-core` (run id 도입, retry 처리)
  - `swarm-run-id-scope` (cleanup, list, await 의 scope 를 run id 로)
- stash 에 미완성: `stash@{0}: On patch/swarm-diagnostics: wip: phase C swarm-diagnostics partial` (`git stash show -p` 로 phase C 진행 상태 확인 필요).
- 사용자 보고: 추가 swarm 버그 존재. 구체 증상은 캡처 필요.

### upstream PR/issue 조사 (2026-05-10)

#### upstream issue **#76 "[BUG] Autonomous swarm bug"** (OPEN, 작성자 xueyouchao, 2026-04-29)
URL: https://github.com/1jehuang/jcode/issues/76

핵심 요약:
- 사용자가 coordinator 에 여러 model 의 agent 를 spawn 하여 repo review.
- 시작은 정상, 그러나 **subagent 1 개만 완료, 나머지 3 개 무응답**.
- coordinator 가 추가 agent 를 계속 만들면서 로컬에 **터미널 10 개 이상 열림**.
- 일부 agent 는 **원래 지정한 path 가 아닌 다른 project/path 에서 review 수행**.
- maintainer 댓글: "swarm agents not always sending information back to the coordinator is a known bug", spawn directory 문제도 인정.

우리 마일스톤 매핑: **직접 관련**.
- 이미 우리 patch (run id scope, owned_only await, stale cleanup, coordinator self-promotion retry) 와 부분 겹침.
- 추가 확인 필요 항목:
  - cwd propagation: subagent 가 잘못된 작업 디렉토리에서 시작되는지.
  - unbounded spawning: coordinator 가 무한 worker 만들지 않게 cap 추가.
  - aggregation: worker 결과가 coordinator 로 안 돌아오는 root cause.

cherry-pick 가치: 중간 (코드는 없음, 재현 시나리오로 매우 유용).

#### upstream PR **#78 "Add tmux pane spawning support"** (OPEN, watzon, 2026-04-29)
URL: https://github.com/1jehuang/jcode/pull/78
- swarm spawn 시 새 터미널 대신 tmux split pane 사용.
- 변경: `src/cli/tui_launch.rs`, `src/tui/app/helpers.rs` 등.
- M2 표면 증상 (10 개 터미널 폭발) 완화에는 도움이 되지만 worker 무응답 자체를 고치지는 않음.
- cherry-pick 가치: 중간-낮음. 별도 UX patch 로 검토 가능.

#### upstream issue **#135 "Full Audit - Bugs"** (OPEN, 1jehuang)
URL: https://github.com/1jehuang/jcode/issues/135
- swarm 관련 audit: `src/server/client_state.rs`, `debug_command_exec.rs`, `debug_jobs.rs`, `swarm.rs` 의 `Mutex<Agent>` 가 `.await` 지점에 걸쳐 유지되어 deadlock 위험.
- `src/server/runtime.rs` 의 `event_history: VecDeque<SwarmEvent>` 가 unbounded growth.
- M2 와 간접 관련. phase C diagnostics 에 lock hold time / event_history size / stalled worker state dump 포함하면 좋음.
- cherry-pick 가치: 낮음 (issue only). diagnostics checklist 로 활용.

#### upstream PR **#151 "Proposal: jcode-harness embedded skills + LLM wiki memory loop"** (OPEN, chapzin, 2026-05-06)
URL: https://github.com/1jehuang/jcode/pull/151
- 대형 proposal. swarm-analysis artifacts / docs 중심, runtime fix 는 아님.
- 우리 phase C swarm-diagnostics 와 이름만 유사, 내용은 다름.
- cherry-pick 가치: 낮음. swarm-analysis report 구조 아이디어만 참고.

### 다음 단계
1. `git stash show -p stash@{0}` 로 phase C 진행도 확인.
2. 사용자 측 추가 증상 캡처 (코디네이터 promotion 실패? worker spawn race? cleanup leak? coord 가 multiple worker 결과 못 모음?).
3. 위 #76 핵심 시나리오를 그대로 우리 환경에서 재현 시도 → 로그 분석.
4. 마일스톤 본문에 root cause 채우고 패치 분리.

### 관련 파일 후보
- `src/server/swarm.rs`
- `src/server/comm_session.rs`
- `src/server/runtime.rs` (`event_history` audit)
- `src/server/client_state.rs`, `debug_command_exec.rs`, `debug_jobs.rs` (lock audit)
- `src/agent/swarm/*` 또는 `src/tool/swarm*`
- 진단 도구: `jcode swarm list/await/cleanup` (이미 run id scoping 들어감)

---

## 📍 Milestone M3 — Hook 시스템 확장: `session.stop` 과 `response.completed`

상태: **🟡 PARTIAL — 명세 확정 필요, 구현 안 됨**
우선순위: **Medium**

### 현재 구현된 hook (확정)
`src/hooks.rs` 11~12 줄:
```rust
pub const TOOL_EXECUTE_BEFORE: &str = "tool.execute.before";
pub const TOOL_EXECUTE_AFTER:  &str = "tool.execute.after";
```
- ✅ `tool.execute.before` — `pre_tool_use` 에서 발행, `run_tool_hooks(...)` 통해 외부 명령 실행.
- ✅ `tool.execute.after`  — `post_tool_use` 에서 발행, 결과 페이로드 포함.
- payload 구조: `ToolHookPayload { event, session_id, message_id, tool_call_id, cwd, tool: { name, args, result? } }`.
- config: `[hooks]` + `[[hooks.commands]] event = "..." command = "..." tool = "*" or "<tool name>"`.
- 현재 lazydino 사용 사례: `check-bash.sh`, `log-tool.sh`.

### 추가하려는 event (현재 source 에 없음)

| event              | 시점            | 페이로드 (제안)                                   | 용도                                |
|--------------------|-----------------|---------------------------------------------------|-------------------------------------|
| `session.stop`     | 세션 종료 직전  | `{ session_id, working_dir, reason, message_count }` | 세션 종료 시 백업/정리/외부 알림    |
| `response.completed` | 한 턴의 응답 완료 후 | `{ session_id, message_id, stop_reason, tool_calls_count, output_len }` | 자동 review, 메모리 추출, 통계 수집 |

### upstream PR/issue 조사 (2026-05-10)
- **동일 hook event 추가 PR/issue 없음.**
- 간접 관련:
  - **#144 "programmatic orchestration API for external harnesses"** (OPEN, ao92265, 2026-05-06): jcode 를 외부 multi-agent orchestrator backend 로 쓰기 위한 stable non-interactive API 요청. session lifecycle / structured event 노출 요구. → naming/payload 설계 시 참고.
  - **#54 "Decompose TUI app state and turn orchestration"** (OPEN, 1jehuang, 2026-04-13): turn engine boundary 정리 제안. → hook 발행 지점 정할 때 구조적 위치 검토.
- cherry-pick 가치: 모두 issue only, 낮음. 설계 참고 자료.

### 발행 지점 (구현 위치 후보)
- `session.stop`:
  - `src/server/client_disconnect_cleanup.rs` 또는 `src/server/comm_session.rs` 의 세션 종료 경로.
  - reload 로 인한 세션 일시 detach 와 진짜 종료를 구분해야 함 (reason 필드).
- `response.completed`:
  - `src/agent/turn_loops.rs` 또는 turn streaming 의 `Turn complete` 로그 직전.
  - 빈 응답 retry, incomplete continuation 후의 최종 한 번만 발행 (재시도 중간엔 발행 안 함).

### 작업 항목
- [ ] event 상수 두 개 추가, `pub const`.
- [ ] payload 타입 정의 (`SessionStopHookPayload`, `ResponseCompletedHookPayload`).
- [ ] 발행 함수 추가 (`run_session_hooks(SESSION_STOP, ...)`, `run_response_hooks(RESPONSE_COMPLETED, ...)`).
- [ ] 발행 지점 wire-up.
- [ ] 단위 테스트 (config 로딩, 발행 시점, payload 직렬화 골든).
- [ ] `default_file.rs` 의 주석 예시 갱신.
- [ ] 문서: `LAZYDINO_MAINTENANCE.md` 에 패치 등록 + skill `/jcode-init` 의 hooks 섹션 갱신.

### 회귀 위험 체크리스트
- [ ] hook 명령 실행 실패 시 세션/턴 진행은 막히지 않아야 함 (현재 `post_tool_use` 처럼 `warn` 로깅만).
- [ ] `session.stop` 은 reload 시 발행되지 않아야 함 (오발행 시 중복 알림).
- [ ] `response.completed` 가 retry/continuation 중간에 발행되지 않아야 함 (마지막 한 번).
- [ ] 헤드리스 / TUI / remote 모두에서 동일 시점 보장.

### 관련 파일
- `src/hooks.rs`
- `src/config.rs` (`HooksConfig`, `HookCommandConfig`)
- `src/config/default_file.rs:274` (주석 예시)
- `src/agent/turn_loops.rs`
- `src/agent/turn_streaming_mpsc.rs`, `src/agent/turn_streaming_broadcast.rs`
- `src/server/comm_session.rs`, `src/server/client_disconnect_cleanup.rs`

---

## 📍 Milestone M4 — TUI interleave 가 tool 완료 후에야 흡수됨

상태: **🟡 BY-DESIGN with caveats — UX 개선 가치 있음**
우선순위: **Medium**

### 결론 (oracle 이중 검증, 2026-05-10)
**현재 동작은 jcode upstream 의 의도된 설계.** 우리 patch 의 회귀 아님.

- 평문 Enter 의 interleave 는 항상 `urgent=false` 로 보내짐 (`src/tui/app/remote/key_handling.rs:21` `remote.soft_interrupt(content, false)`).
- non-urgent soft interrupt 는 tool 사이엔 inject 안 되고 **Point D (모든 tool 완료 후)** 에서만 inject 됨. 주석에 명시:
  > `turn_streaming_mpsc.rs:1020` "We do NOT inject between tools (non-urgent) because that would place user text between tool_results, which may violate API constraints."
- API constraint 의 의미: assistant message 의 tool_use 는 user tool_result 와 즉시 페어링되어야 한다 (Anthropic / OpenAI 모두). 사이에 일반 user text 가 끼면 구조가 깨짐.

### 정정 사항 (이전 가설 반박)
- 이전 가설: "urgent=true 면 현재 tool 즉시 abort 후 inject."
- **반박**: urgent 도 현재 실행 중인 tool 을 abort 하지 않음. urgent 는 `tool_index > 0` 일 때 **다음 tool 시작 전에 남은 tool 들을 skip** 하는 Point C 동작 (`turn_streaming_mpsc.rs:715~752`). 단일 tool / 첫 tool / subagent tool 에는 urgent 도 즉시 효과 없음.
- 즉 long-running tool 또는 subagent 가 도는 동안 사용자 메시지를 즉시 모델에 전달하는 경로는 **현재 코드에 존재하지 않음**.

### 사용자 입장에서의 부작용
- Subagent 가 길게 도는 동안 평문 Enter 로 입력 → `⏭ Interleave sent` 표시는 뜨지만 모델 응답까지 시간이 매우 길어 보임.
- Subagent 가 끝나야 비로소 메시지가 흡수됨. UX 적으로 "묶여 있는 듯한" 인상.
- 이게 사용자 입장에선 "버그처럼" 느껴짐. **M5 가 깨져 있어서 Alt+B 로 빠져나갈 수도 없으니 더 답답해짐.**

### 개선 옵션 (안전한 순서)
1. **상태 메시지 명확화** (가장 안전, 가장 빠름)
   - `"⏭ Interleave sent"` → `"⏭ Will be processed after current tool"` 같이 정확히 표현.
   - `src/tui/app/remote/key_handling.rs:29`.
2. **urgent 단축키 노출** (중간)
   - 별도 단축키 (예: Ctrl+Shift+Enter) 로 `remote.soft_interrupt(content, true)` 호출.
   - 단, current tool abort 가 아니라 "다음 tool 시작 전 남은 tool skip" 임을 UI 에 명확히.
3. **현재 tool 에 soft interrupt select 추가** (큰 변경)
   - `turn_streaming_mpsc.rs` 의 tool `tokio::select!` 에 urgent soft interrupt notification 추가.
   - current tool abort / graceful tool_result 처리 / subagent child cleanup 모두 별도 설계 필요.
4. **subagent forwarding channel** (구조적, 가장 큼)
   - parent interleave 를 child subagent queue 로 forwarding.
   - child session 의 안전한 inject point 에서 처리.
   - parent/child transcript 중복 기록 방지 설계 필요.

### 회귀 위험
- tool_use ↔ tool_result adjacency 깨면 Anthropic/OpenAI API 오류.
- urgent 로 current tool abort 시 file write/edit/batch 의 partial side effect.
- subagent forwarding 시 parent/child 양쪽에 동일 user input 중복.

### 추가 단위/통합 테스트 제안
1. non-urgent interleave during single long tool → tool 완료 전 SoftInterruptInjected 발생 안 함, 완료 후 Point D 발생.
2. urgent interleave before second tool → 첫 tool 완료 후 second tool 실행 전 Point C 로 remaining tools skipped.
3. urgent interleave during first/single tool → 현재 코드 기준 즉시 inject 안 됨을 명시적으로 고정.
4. subagent tool during parent interleave → parent queue 에 쌓이고 subagent completion 후 Point D 에서 inject.

### 관련 파일
- `src/tui/app/remote/key_handling.rs:11~31` `send_interleave_now`
- `src/tui/backend.rs:689~701` `RemoteConnection::soft_interrupt`
- `crates/jcode-protocol/src/lib.rs:90~98` `Request::SoftInterrupt`
- `src/server/client_lifecycle.rs:1311~1323, 2761~2771` server-side soft interrupt queue
- `src/agent/turn_streaming_mpsc.rs:649~653, 715~752, 820~870, 1020~1045` inject points B/C/D
- `src/agent/interrupts.rs` queue/inject helpers

---

## 📍 Milestone M5 — Alt+B early race: `background_tool_signal.reset()` 이 너무 늦음 ✅ DONE

상태: **✅ DONE (2026-05-10)** — commit `52375aac` + `f76dfdda`, binary `v0.12.97-dev (f76dfdda)` 설치 완료
원본 우선순위: Highest

### 결과 요약
- **Fix commit**: `52375aac` — `fix: preserve early Alt+B fire by moving background signal reset before ToolStart`
- **Docs commit**: `f76dfdda` — `docs: record alt+b early race patch`
- **변경 파일**:
  - `src/agent/turn_streaming_mpsc.rs` — `background_tool_signal.reset()` 호출을 turn loop iteration 시작점 (ToolStart emit 보다 앞) 으로 이동, 기존 line 831 (tool spawn 직후) reset 제거.
  - `src/server/client_lifecycle.rs::move_tool_to_background` — `request_background_current_tool()` 반환값이 `false` 면 `ServerEvent::Error` 로 응답 (사용자가 race 실패를 인지할 수 있게).
  - `crates/jcode-agent-runtime/src/lib.rs` — InterruptSignal 의 fire-before-notified 보존성 단위 테스트 1 개 추가.
  - `src/agent_tests.rs` — 통합 테스트 7 개 추가.
  - `LAZYDINO_MAINTENANCE.md`, `scripts/lazydino/reapply-custom-stack.sh` — 패치 등록.
- **새 단위 테스트 (8 개, 모두 PASS)**:
  - `interrupt_signal_is_set_false_initially`
  - `interrupt_signal_is_set_true_after_fire`
  - `interrupt_signal_reset_clears_flag`
  - `interrupt_signal_fire_before_notified_does_not_hang`
  - `interrupt_signal_fire_concurrent_with_notified`
  - `interrupt_signal_notified_completes_after_fire`
  - `interrupt_signal_altb_early_race_fire_survives_until_reset` ← 핵심 race 검증
  - `turn_streaming_mpsc_altb_early_race_preserves_fire_after_tool_start` ← turn-loop 시나리오 검증
  - `turn_streaming_mpsc_clears_stale_background_signal_before_next_tool_start` ← stale signal 회귀 방지
- **회귀 테스트**: `cargo test --lib agent::tests` → **24 passed, 1 failed** (1 failed 는 `env_snapshot_detail_is_minimal_for_empty_sessions_and_full_after_history` — `LAZYDINO_MAINTENANCE.md` 의 알려진 upstream known-failure list 에 이미 등록된 항목, 새 회귀 아님).

### Root cause (확정)
`BackgroundToolSignal` (= `InterruptSignal` = AtomicBool + tokio::Notify latch) 의 reset 타이밍이 `ToolStart` 이벤트 노출보다 너무 늦었음:
1. agent 가 tool spawn → ToolStart emit → TUI 가 RunningTool 표시.
2. 사용자가 즉시 Alt+B → server 가 `signal.fire()` 로 SET.
3. 그 직후 agent 코드가 `self.background_tool_signal.reset()` → AtomicBool=false 로 wipe.
4. 이후 `bg_signal.notified().await` 는 새 fire 가 와야 깨어남 → tool 끝까지 그대로 진행.

### Fix
`src/agent/turn_streaming_mpsc.rs:12~17`:
```rust
loop {
    // Clear any stale background-tool request before the provider can emit
    // ToolStart for this turn. Once ToolStart is visible to the UI, an
    // Alt+B fire must be preserved until the tool execution select observes it.
    self.background_tool_signal.reset();

    let repaired = self.repair_missing_tool_outputs();
    ...
}
```
ToolStart 노출 이전에 reset 을 옮기고, 기존 spawn 직후의 reset 은 제거. select 까지의 race window 가 사라짐.

추가로 `move_tool_to_background` 가 무조건 `Ack` 만 보내던 것을 `request_background_current_tool()` 반환값으로 분기하여 실패 시 `ServerEvent::Error` 발행.

### 검증 시나리오 (수동, 사용자가 직접 확인 가능)
1. `~/.local/bin/jcode --version` → `v0.12.97-dev (f76dfdda)` 가 보여야 함. ✅
2. 새 jcode 세션에서 subagent 또는 긴 bash tool 호출.
3. ToolStart 카드가 나타나는 즉시 Alt+B → "Moving tool to background..." status notice 가 뜨고 곧이어 background task 카드로 전환되어야 함.
4. 두 번째 / 세 번째 연속 Alt+B 도 동일하게 동작.

### 후속 추적 (별도 마일스톤)
- M5 가 닫혔지만, bg 로 옮긴 후 부모 turn loop 가 알아서 재개되지 않는 것은 **M1** 영역 (delivery target 라우팅).
- M4 의 "interleave 가 tool 완료 후에야 흡수" 도 별개. M5 만으론 안 풀림.

### 관련 파일 (참고용)
- `src/agent/turn_streaming_mpsc.rs:11~17, ~830~870` — reset 위치, select 진입.
- `crates/jcode-agent-runtime/src/lib.rs:32~67, 78~95` — InterruptSignal 정의 + 신규 테스트.
- `src/server/client_lifecycle.rs:2789~2806` — `move_tool_to_background`.
- `src/agent_tests.rs:178~340` — `GatedToolProvider`, `SingleToolProvider`, `DelayTestTool`, 신규 테스트들.

---

## 📍 Milestone M6 — Alt+B 후 부모 turn 즉시 다음 응답 → "잠잠 (idle)" 으로 변경

상태: **🔵 DEFERRED** — upstream 의도된 설계를 변경하는 작업이므로 기본 버그/문제 (M1, M2, M3) 정리 후 진행
우선순위: Low (deferred)
사용자 결정 (2026-05-10): "이건 우선 이대로 놔봐. 기본적인 에러들이랑 문제들 먼저 잡고 가자."

### 증상 (사용자 보고, 2026-05-10)
- Alt+B 로 tool 을 background 로 옮기면 **부모 turn loop 가 즉시 다음 모델 호출로 진행** 해서 모델이 ack/요약을 응답함.
- 사용자 기대: "background 로 치워뒀으니 결과 나올 때까지 잠잠해야 한다." 새 prompt 가 들어오기 전에는 모델이 떠들면 안 됨.
- 결과: 사용자가 입력 시도해도 (M4 영향) "Interleave sent" 만 뜨고 묶임 → 답답한 UX.

### 원본 jcode upstream 동작 확인 (2026-05-10)
`origin/master:src/agent/turn_streaming_mpsc.rs:971~1007` — 우리 fork 와 동일.

```rust
// User pressed Alt+B — move tool to background
let bg_info = crate::background::global()
    .adopt(&tc.name, &self.session.id, tool_handle).await;

let bg_msg = format!(
    "Tool '{}' was moved to background by the user (task_id: {}). \
     Use the `bg` tool with action 'wait' to wait for completion/checkpoints, \
     or action 'status'/'output' to inspect it.",
    tc.name, bg_info.task_id
);

let _ = event_tx.send(ServerEvent::ToolDone { ... output: bg_msg.clone(), ... });
self.add_message_with_duration(Role::User, vec![ContentBlock::ToolResult { ... }], ...);
self.session.save()?;
self.background_tool_signal.reset();
// ← return 안 함, fall through 해서 다음 turn iteration 으로 진입.
// 다음 iteration 에서 모델 API 호출이 다시 발생.
```

즉 **upstream 의 의도된 설계**: ToolResult 합성 → conversation 구조 보존 → 다음 모델 호출 → 모델이 알아서 `bg wait` 로 폴링하길 기대.

문제: 현실에서 모델이 `bg wait` 를 항상 호출하지는 않음. 짧은 ack 로 끝나거나 아무 동작 안 하기도 함. 그래서 사용자 입장에선 "옮겼는데 왜 모델이 떠드나" 가 됨.

### Claude Code 와의 비교 (참고)
- Claude Code 의 일반적 background 동작: `run_in_background: true` 로 bash 실행 시 즉시 task_id 만 반환하고 turn 종료. 사용자 prompt / 명시적 회수 시점까지 silent.
- 즉 Claude Code 는 "치워두면 잠잠" 을 선택. 사용자 멘탈 모델에 부합.

### 결정: 기본을 idle 로 변경 (옵션 1) + config 토글 (안전 장치)

**기본 동작 (`altb_yields_turn = true`)**:
1. Alt+B 분기에서 ToolResult 합성 + history 추가까지는 그대로.
2. 그 다음 **`return Ok(())` 로 turn 즉시 종료**.
3. 사용자 새 prompt 또는 background completion wake 시 새 turn 시작.

**Config 토글 (`altb_yields_turn = false`)**:
- upstream 호환 동작 (turn 계속 진행, 모델이 ack 응답).
- backward-compatibility 안전 장치.

### 구현 계획

#### 변경 1 — `src/config/default_file.rs` 에 옵션 추가
`[display]` 섹션에 (또는 더 적합한 곳에):
```toml
# When true, pressing Alt+B yields the current turn immediately after detaching
# the tool to background. The model stays silent until the user sends a new
# prompt or the background task completes. When false, the legacy upstream
# behavior continues the turn so the model can acknowledge the detach.
altb_yields_turn = true
```

config struct 에 필드 추가, default = true.

#### 변경 2 — `src/agent/turn_streaming_mpsc.rs` Alt+B 분기 수정
현재의 line 1007 `self.background_tool_signal.reset()` 다음에:
```rust
if config().display.altb_yields_turn {
    return Ok(());
}
// else: fall through (upstream 호환)
```

#### 변경 3 — `src/agent/turn_streaming_broadcast.rs` 에도 같은 분기 적용

#### 변경 4 — TUI 측 status notice
- Alt+B 직후 `"Moving tool to background..."` 가 `"Detached. Idle."` 또는 비슷한 명시적 표현으로 전환.
- 이미 M5 에서 ack 분리 했으니 Ack 받으면 적절한 상태로.

#### 변경 5 — Wake 정책 (M1 종속)
- M1 에서 background completion 이 부모 turn 으로 자동 wake 되게 만들 때, **idle 상태에서만 wake** 하면 자연스러움.
- 사용자가 새 prompt 보내서 turn 도는 중이면 wake 가 큐에 쌓이고 (현재 `queue_soft_interrupt_for_session` 경로) 다음 inject point 에서 흡수.

### 검증 시나리오
1. Alt+B 후 모델이 추가 응답 안 함 (idle 진입). status notice 가 idle 임을 알림.
2. background task 완료 시 (M1 wake 정책) 모델이 자동으로 결과 회수 turn 시작.
3. 사용자가 idle 중 새 prompt 보내면 정상 turn 시작 + ToolResult 가 history 에 있어서 모델이 컨텍스트 인지.
4. `altb_yields_turn = false` 로 토글 시 upstream 동작 회복.
5. 다단계 subagent 에서 Alt+B 도 동일하게 idle.

### 회귀 위험
- ToolResult 가 채워졌는데 turn 이 종료되면, 다음 user prompt 시점에 conversation 구조가 valid 해야 함. 일반적으로 user_message 가 합쳐지면 OK.
- background completion wake 가 사용자 입력과 race 시 inject point 처리 (M1 영역).
- `bg wait` 를 명시적으로 prompt 한 흐름이 있다면 (예: subagent 내부) 그건 별개로 동작 — turn 종료가 그 동작에 영향 안 줘야 함.

### 관련 파일
- `src/agent/turn_streaming_mpsc.rs:980~1010` Alt+B 분기 (return 추가)
- `src/agent/turn_streaming_broadcast.rs` 동일 패턴
- `src/config.rs`, `src/config/default_file.rs` (`DisplayConfig`)
- `src/tui/app/remote/key_handling.rs:333~339` (status notice 텍스트)

---

## 우선순위와 작업 순서 (2026-05-10 갱신)

oracle 이중 검증 결과를 반영한 우선순위:

1. ✅ **M5 완료** — Alt+B race fix. commit `52375aac`/`f76dfdda`, binary `f76dfdda` 설치 완료.
2. ✅ **M1 완료** — Background task delivery target routing. commit `1387e77e`/`b9085898`, binary `b9085898` 설치 완료. delivery_session_id 필드 + parent chain resolver + headless drain false-live check 도입.
3. **M3 다음** — hook event 두 개 추가 (`session.stop`, `response.completed`). 작은 변경, 자동 review/메모리 추출 가치 큼.
4. **M2** — phase C swarm-diagnostics stash 회수 → upstream #76 시나리오 재현 → 추가 패치.
5. **M4 옵션 1 만 빠르게** — 상태 메시지 문구 개선 (`"⏭ Will be processed after current tool"`). 한 줄짜리 patch. 후순위.
6. **M6 deferred** — upstream 의도 변경이라 기본 버그 다 끝낸 뒤.

### 마일스톤 완료 시 표준 절차
- 패치 브랜치 생성 (`patch/<short-name>`).
- `LAZYDINO_MAINTENANCE.md` 에 등록.
- `scripts/lazydino/reapply-custom-stack.sh` 의 PATCH_STACK 에 추가.
- 빌드 → `~/.jcode/builds/versions/...` 에 새 버전 → `current` symlink 갱신.
- 본 문서에서 항목 제거 또는 `✅ DONE (commit ...)` 로 표시.

### 진단 출처 (이중 검증)
- 직접 코드 읽기: 본 문서 작성자.
- oracle subagent 검증 (GPT-5.5 high effort): `session_crow_1778405765497_8ccc0d933f4ac974` (M1/M4/M5), `session_pig_1778405043496_327e97d8d0a0a8ae` (upstream PR/issue 조사).
- 두 출처가 일치하는 결론만 마일스톤 본문에 정상 기재. 가설/반박은 명시.

---

## 부록 A — upstream PR/issue 조사 미션 결과 요약 (2026-05-10)

대상: `1jehuang/jcode` (upstream), `lazy-dinosaur/jcode` (fork).

총 PR 55, issues 128 수집 후 `tool.execute.before/after`, `session.stop`, `response.completed`, `BackgroundTaskCompleted/Progress`, `No output captured`, `Alt+B`, `Moving tool to background`, `swarm diagnostics`, `owned_only`, `stale worker` 등 키워드 검색.

| 마일스톤 | upstream artifact | 매핑     | 비고                                              |
|----------|-------------------|----------|---------------------------------------------------|
| M1       | issue #12         | 간접     | 일반 bg task 완료 알림 요구. 닫힘. 직접 fix 아님. |
| M2       | issue #76         | 직접     | known bug. cwd / unbounded spawn / aggregation.   |
| M2       | PR #78            | 표면     | tmux pane spawn. UX 완화에만 도움.                |
| M2       | issue #135        | 간접     | swarm lock audit / event_history audit.           |
| M2       | PR #151           | 매우간접 | harness proposal. swarm-analysis docs 참고.       |
| M3       | issue #144        | 간접     | external orchestration API. naming 참고.          |
| M3       | issue #54         | 간접     | turn orchestration boundary. 구조 참고.           |
| M4       | (없음)            | -        | 직접 hit 없음.                                    |
| M5       | (없음)            | -        | 직접 hit 없음.                                    |

fork (`lazy-dinosaur/jcode`): PR 0 개, issues disabled, hit 없음.

상세 내용은 위 각 마일스톤 본문의 "upstream PR/issue 조사" 섹션 참고.

---

## 📍 Milestone M7 — 비정상 종료 시 메시지 유실 (reload + crash) 🔴 OPEN

상태: **🔴 OPEN — oracle 진단 진행중 (task `167472npu9` 대기)**
우선순위: **Critical (데이터 유실)**

### 증상

1. `~/.jcode/sessions/<id>.json` (snapshot) 이 메시지 1 개 (세션 시작 시점) 로 굳어 있음.
2. `~/.jcode/sessions/<id>.journal.jsonl` 만 활발히 채워짐 (수십~수백 줄).
3. `/reload` 또는 server SIGKILL/crash 후 `journal` 을 무시하는 경로로 reload 되면 메시지 다 사라짐.

### 라이브 증거 (2026-05-10 12:43 UTC)

```
session_mouse_1778416384546_b20646bdb7e456c9
  snap_msgs=1 snap_mtime=12:33:04 UTC (세션 시작)
  jrn_lines=88 jrn_mtime=12:43:34 UTC (활발히 기록)
  diff=630s (10.5 분간 snapshot 미갱신)
```

10 개 이상의 다른 세션에서도 동일 패턴 확인.

### Root cause (oracle v1, task `096192ai9s`, 2026-05-10 12:48 UTC)

**가장 유력 원인**: `src/session/persistence.rs:159-163` `load_startup_stub()` 가 snapshot JSON 만 읽고 journal 을 무시한다. 그리고 `src/session.rs:268-298` 의 `session_from_startup_stub()` 는 명시적으로 `session.messages.clear()` 등으로 transcript 를 다 비운다.

`src/server/client_state.rs:162-163, 288-289, 340-341` 에서 reload/history 경로가 `Session::load_for_remote_startup` 실패 시 `Session::load_startup_stub` 으로 fallback 하는데, **이 fallback 이 발동하면 디스크의 journal 데이터를 봤어도 화면엔 0 메시지로 뜸**.

세부 트리거:
- 첫 save 전에 server crash → snapshot 파일이 0 메시지인 채로 굳음
- `append_journal_entry_for_new_message` 의 early-return `!snapshot_path.exists()` (line 51) 로 새 세션 첫 메시지가 journal 에도 안 기록될 가능성
- journal append 시 fsync 부재 (의심) — SIGKILL 시 OS buffer 의 데이터 유실

(상세 root cause 매트릭스는 oracle v2 결과 도착 후 확정 — task `167472npu9`.)

### 회귀 위험

- `load_startup_stub` 에 journal 적용 추가 시 startup latency 영향 (현재는 paint 우선 빠른 stub 용도)
- fsync 추가 시 hot path 성능 영향
- fallback 경로 자체를 제거하면 normal-case 에서 stub 의 lightweight 의도 깨짐

### 수정안 (oracle v2 결과 후 확정)

후보 (잠정):
- A. `load_startup_stub` fallback 시에도 journal 을 적용하도록 변경 (1 줄 가까운 fix)
- B. journal append 시 fsync 옵션 추가
- C. 메시지 push 즉시 durable persist 보장 (큰 변경)

### 검증 계획

- 단위 테스트: `load_startup_stub_preserves_journal_messages` 추가
- 라이브 검증: 현재 `session_mouse_*` 세션을 사본 떠놓고 `/reload` 후 메시지 카운트 측정 (88 lines journal 이 화면 메시지에 반영되는지)
- crash injection: kill -9 직후 reload 시 메시지 보존 측정

---

## 📍 Milestone M8 — Subagent Alt+B 가 가끔 background detach 안 됨 🟠 OPEN

상태: **🟠 OPEN — reproduction 정보 수집중**
우선순위: **High**

### 증상 (사용자 보고, 2026-05-10)

- subagent 도구 (가끔 edit/bash 도) 에 Alt+B 누르면 **"Moving tool to background..." status notice 만 뜨고 풀리지 않음**.
- detach 안 되어 도구가 foreground 에서 계속 도는 채 turn 이 잠김.

### 차별 (M5 와 다름)

M5 는 `background_tool_signal.reset()` 이 fire 를 wipe 해서 신호 자체가 안 잡힌 케이스 (이미 수정됨, commit `52375aac`). M8 은 **status notice 까지 떴는데 풀리지 않는** 다른 패턴.

### 의심 root cause (잠정)

코드 흐름:
1. TUI: `key_handling.rs:336-340` 에서 `ProcessingStatus::RunningTool` 일 때만 `remote.background_tool()` 호출.
2. server: `client_lifecycle.rs:1330` `Request::BackgroundTool` → `move_tool_to_background` (line 2789) → `session_control.request_background_current_tool()` 호출.
3. `state.rs:473-480`: `background_tool_signal` 이 등록된 경우만 fire, 아니면 false.

**가설 A (가장 의심)**: subagent 호출 직후 client 측 status 가 잠시 `Streaming`/`Reasoning` 상태인 동안 Alt+B 무시됨 → 사용자가 다시 누르면 그땐 RunningTool 인데 server signal listener 가 이미 dropped 상태 (race).

**가설 B**: subagent 의 nested tool 실행 중에 inner agent 가 자체 background_tool_signal 을 들고 있어서 outer (parent) 의 신호를 못 받음. M1 에서 부분적으로 다뤘지만 detach 자체에는 적용되지 않은 가능성.

**가설 C**: `RunningTool` 상태 진입과 server 측 signal 등록 사이의 race. TUI 가 `RunningTool` 로 전환된 시점 ≠ server 가 signal 등록한 시점.

### 라이브 데이터

오늘 jcode 로그 (`~/.jcode/logs/jcode-2026-05-10.log`) 에서 12:00~12:48 UTC 사이 모든 Alt+B 시도가 정상 detach 됨 ("Tool 'subagent' moved to background after ..." 로그). 사용자 보고 케이스는 이 시점 이전 또는 다른 세션일 수 있음. 정확한 reproduction 정보 + 그때의 server 로그 필요.

### 진단 다음 단계

1. 사용자한테 reproduction 시 어떤 세션이었고, 시각, 로그 캡처 부탁
2. 현재 세션에서 의도적으로 race 조건 만들어 재현 시도 (subagent 호출 직후 ms 단위로 Alt+B)
3. server 측에 "background_tool requested but no active session control signal is registered" debug 로그 (line 2797) 가 떴는지 확인
4. M8 은 M7 다음 진행 (M7 critical, M8 high)

---

## 📍 Milestone M9 — Hook 이중 발동 (project-local config 가 글로벌 config 를 자기 자신으로 인식) 🔴 OPEN

상태: **🟡 INVESTIGATING — 1회 확정 재현, 후속 미재현은 server caching 인공물 (2026-05-10 13:11 UTC 코드 read 완료)**
우선순위: **Medium-High (코드 가설은 코드 read 로 확정)**

### 증상 (1회 확정 재현, T-M3 첫 검증, 2026-05-10 12:55 UTC)

- `[hooks]` 섹션을 `~/.jcode/config.toml` 한 곳에만 정의했는데, **`response.completed` hook 의 동일 payload 가 한 turn 당 두 행씩 로그 파일에 추가됨** (session_id, message_id 동일).
- 환경: `jcode run "OK"` 단발 CLI (= 새 process, 새 `Config::load()`), working_dir=`/home/lazydino`, blocking=true.

### 후속 재현 시도 (2026-05-10 13:06~13:08 UTC) — 모두 1회씩 fire

| 시도 | 환경 | 결과 |
|------|------|------|
| rooster session | TUI, working_dir=`/home/lazydino`, blocking=true | response.completed × 1, session.stop × 1 |
| sheep session   | TUI, working_dir=`/home/lazydino/dev/jcode`, blocking=true | response.completed × 1, session.stop × 1 |
| sloth session   | TUI, working_dir=`/home/lazydino`, blocking=true | response.completed × 1, session.stop × 1 |

→ TUI 컨텍스트에서는 1회만. 처음엔 모순으로 보였으나 코드 read 로 원인 확정 (아래).

### 결정적 코드 근거 (코드 read 완료, 가설 확정)

#### Step 1 — `find_project_config_dir` (src/config/config_file.rs:170-185)

```rust
fn find_project_config_dir(working_dir: &Path) -> Option<PathBuf> {
    let start = if working_dir.is_file() { working_dir.parent()? } else { working_dir };
    for ancestor in start.ancestors() {
        let project_config = ancestor.join(".jcode").join("config.toml");
        let local_config = ancestor.join(".jcode").join("config.local.toml");
        if project_config.exists() || local_config.exists() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}
```

→ **자기참조 skip 로직 없음.** working_dir=`/home/lazydino` 또는 그 하위면 ancestor 순회 중 `/home/lazydino` 에 도달 → `~/.jcode/config.toml` 발견 → `/home/lazydino` 가 project root 로 return.

#### Step 2 — `hooks_for_working_dir` (src/config/config_file.rs:104-119)

```rust
let mut hooks = self.hooks.clone();   // 글로벌 hooks (1)
if let Some(project_dir) = working_dir.and_then(Self::find_project_config_dir) {
    for config_path in [...] {
        if let Some(project_hooks) = Self::load_hooks_from_file(&config_path) {
            hooks.commands.extend(project_hooks.commands);  // project hooks (2)
        }
    }
}
```

`load_hooks_from_file` 는 toml 다시 parse 해서 `config.hooks` 그대로 반환 (캐시 없음). 자기참조 skip 없음. **global + project_local = 같은 hook 두 번 register 확정.**

#### Step 3 — `run_lifecycle_hook_commands` (src/hooks.rs:208~243)

```rust
for hook in matching {
    if hook.blocking { run_blocking_lifecycle_hook(...).await }
    else { tokio::spawn(...) }
}
```

dedupe 없이 그냥 모두 fire. → 두 번 등록되면 두 번 fire.

#### 핵심: 왜 후속 검증에서 1회만 fire 됐는가

`config()` = `OnceLock<Config>`. **server process 가 처음 spawn 될 때만** `Config::load()` 가 호출되어 글로벌 hooks 가 메모리에 박힘. 그 이후 `~/.jcode/config.toml` 을 수정해도 글로벌 hooks 는 변하지 않음.

- T-M3 첫 검증 (12:55 UTC, elephant): `jcode run` = 새 process spawn → **`Config::load()` 가 새 hooks (`response.completed` × 1) 를 글로벌로 read** → `hooks_for_working_dir` 에서 또 한 번 project-local 로 read → total 2 register → **2회 fire ✅ 가설대로**.
- T-M3 후속 검증 (13:06~13:08 UTC): TUI 가 attach 된 server process 는 12:00 UTC 무렵 시작된 것. 그 시점 `~/.jcode/config.toml` 의 `[hooks] enabled=false, commands=[]` 로 글로벌 init. 이후 우리가 config 수정해서 hooks 추가했지만 **글로벌은 비어있는 채로 stale**. project-local read 만 1 hit → total 1 register → **1회 fire**.

**결론**: M9 가설은 **확정적으로 옳음**. 후속 미재현은 server caching 인공물이고, 새 server 띄우면 항상 재현됨.

### 회귀 여부

- **upstream 문제** 거의 확실 (글로벌 config 가 home 디렉토리 아래 `.jcode/` 인 케이스에서 항상 발생). prompt_for_working_dir, agents_for_working_dir 도 같은 패턴이라 같은 버그 가능성 높음.
- 우리 M3 patch 가 새로 만든 `session.stop`/`response.completed` 도 그대로 영향. `tool.execute.before/after` 도 마찬가지 (기존 upstream 동작이 이미 이중 발동).

### 라이브 데이터

```
$ cat /tmp/jcode-response-completed.log   # 12:55 UTC, jcode run "OK", 새 process
{"event":"response.completed","session_id":"session_elephant_...",..."output_chars":2}{"event":"response.completed","session_id":"session_elephant_...",..."output_chars":2}
```

후속 (13:08 UTC, stale server):
```
sheep    cwd=/home/lazydino/dev/jcode   → 1 회 (글로벌 hooks 비어있음)
sloth    cwd=/home/lazydino             → 1 회
horse    cwd=/home/lazydino             → 1 회
```

### 해결 방향 (옵션)

1. **자기 자신 제외 (권장)**: `hooks_for_working_dir` 에서 project_dir 의 `.jcode` canonical path 가 글로벌 jcode_dir 와 같으면 skip. 같은 함수 호출자 (prompt_for_working_dir, agents_for_working_dir) 에도 동일 fix 적용.
2. **명시적 marker**: `.jcode/.project-marker` 같은 파일이 있을 때만 project root 로 인식.
3. **파일 path identity dedupe**: 글로벌 config_path 와 project_config_path 의 canonical 이 같으면 두 번째 read 를 skip.

옵션 1 이 가장 작고 안전. 옵션 3 은 더 일반적 (다른 의도치 않은 자기참조도 막음).

### 회귀 위험

- `/home/lazydino/dev/jcode/.jcode/config.toml` 같은 진짜 project-local 파일은 그대로 동작해야 함. 옵션 1 은 글로벌 jcode_dir 와의 path 비교만 하므로 안전.
- `prompt_for_working_dir`, `agents_for_working_dir` 도 동일 fix 필요 (현재 같은 자기참조 발생 중).

### 검증 시나리오

1. **재현 확정**: 새 server 시작 (`pkill jcode || true; jcode serve &`) → `~/.jcode/config.toml` 에 hook 1 개 → `jcode run "ok"` → log 정확히 2 행.
2. **fix 적용 후**: 동일 시나리오에서 1 행만 기록.
3. **prompt_for_working_dir 회귀**: 글로벌 prompt 가 두 번 적용되지 않는지 확인.
4. **진짜 project-local 정상 merge**: `/home/lazydino/dev/jcode/.jcode/config.toml` 만들어 별도 hook 추가 → 글로벌 1 + project 1 = 2 행 (정상).

### 관련 파일

- `src/config/config_file.rs:104-119` `hooks_for_working_dir` (직접 수정 위치)
- `src/config/config_file.rs:124-141` `prompt_for_working_dir` (같은 패턴, 동일 fix)
- `src/config/config_file.rs:148-178` `agents_for_working_dir` (같은 패턴, 동일 fix)
- `src/config/config_file.rs:170-185` `find_project_config_dir` (자기참조 skip 로직 없음 — 이 함수 자체에 추가도 가능)
- `src/hooks.rs:182-194` `run_lifecycle_hooks` (caller)
- `src/hooks.rs:187-220` `load_hooks_from_file` (toml 재파싱, dedupe 없음)

---

## 📍 Milestone M10 — Non-blocking lifecycle hook 이 단발성 CLI process 에서 race 로 누락 🔴 OPEN

상태: **🔴 OPEN — 라이브 재현 완료 (T-M3 검증 도중 발견, 2026-05-10 12:54 UTC)**
우선순위: **Medium**

### 증상

- `[[hooks.commands]]` 에서 `blocking = false` 인 hook 은 `jcode run "..."` 같은 단발성 CLI 호출에서 **fire 안 됨** (로그 파일 0 byte).
- `blocking = true` 로 바꾸면 즉시 정상 fire.
- TUI 모드 / `jcode debug message --wait` 같은 server-attached path 에선 non-blocking 도 정상 작동 (server 가 살아있으니 spawn task 가 끝까지 살아남음).

### 결정적 코드 근거

`src/hooks.rs` `run_lifecycle_hook_commands`:

```rust
} else {
    // non-blocking: tokio::spawn 으로 fire-and-forget
    let command = hook.command.clone();
    ...
    tokio::spawn(async move {
        if let Err(err) =
            run_nonblocking_hook(&command, timeout_ms, cwd.as_deref(), &payload_json).await
        {
            crate::logging::warn(...);
        }
    });
}
```

`tokio::spawn` 은 task 를 runtime 에 넘기고 **JoinHandle 을 버림**. caller 함수는 즉시 return. 그러면:

1. `jcode run` 의 turn loop 가 `fire_response_completed_hook().await` 호출.
2. 그 안에서 `tokio::spawn(...)` 으로 non-blocking hook task 등록 + 즉시 return.
3. turn 끝남 → process 종료 절차 시작 → tokio runtime drop.
4. 등록된 non-blocking spawn task 는 **start 도 못 한 채 cancel** 되거나, hook 명령의 child process 만 시작됐다 strand.
5. 결과: `/tmp/jcode-response-completed.log` 에 아무것도 안 쓰여짐.

### 회귀 여부

- **upstream 동작 그대로** 가능성 높음. tool.execute.before/after 의 non-blocking 형태도 같은 path 라 단발성 CLI 에서는 같은 race 가 있음 (기존 ���용자가 단발 CLI 에서 hook 검증 안 했을 가능성).
- 우리 M3 patch 가 새로 만든 `session.stop`/`response.completed` 도 그대로 영향.

### 라이브 검증 데이터

T-M3 시나리오 1 라이브:
- `blocking = false` × 2 회 `jcode run "OK"`: log 0 byte ❌
- `blocking = true` 로 변경 후 1 회: log 에 payload 2 줄 (M9 이중발동 영향) ✅

### 해결 방향 (안)

1. **process 종료 전에 spawn task 들 join 까지 wait**:
   - hooks 모듈에 weak set / counter 로 in-flight non-blocking task 추적.
   - main 의 process exit 직전 (CLI dispatch 마지막) `wait_for_pending_lifecycle_hooks(timeout)` 호출.
2. **단발 CLI 모드에선 강제 blocking**:
   - `run_lifecycle_hook_commands` 가 "single-shot CLI 컨텍스트" 인지 알 수 있다면 blocking 으로 fallback.
   - 단점: caller 가 명시 컨텍스트 hint 를 넘겨야 해서 boilerplate 늘어남.
3. **tokio runtime shutdown_timeout 늘리기**:
   - process 종료 시 runtime drop 하는 시점에 `shutdown_timeout(Duration::from_secs(...))` 사용해 task 가 끝까지 돌게.
   - 단점: hook 이 hung 되면 process 가 그만큼 늦게 종료.

옵션 1 이 정확한 해결. 옵션 3 은 단순하지만 race timeout 정확도가 낮음.

### 회귀 위험

- TUI/server-long-running 컨텍스트에선 영향 없음 (이미 잘 작동).
- non-blocking 의 의도는 turn 응답 시간을 막지 않으려는 것 — 옵션 1 도 결국 process 종료 직전엔 wait 하므로 단발 CLI 한정으로는 trade-off OK.
- timeout 너무 길면 사용자가 jcode run 결과 받고 추가 응답 대기 시간 길어짐.

### 검증 시나리오

1. `blocking = false` hook + `jcode run "ok"` → fix 적용 후 log 한 줄 기록.
2. blocking 과 non-blocking 섞은 hook 여러 개 → 모두 fire 후 process 종료.
3. hook 명령이 5 초 sleep 같이 느린 경우 → wait timeout 안에 끝나면 jcode run 도 그만큼 기다려야 함.
4. 회귀: TUI 에서 non-blocking 이 turn 응답을 막지 않는지 (이건 옵션 1 도 turn 종료 시점이 아니라 process 종료 시점에만 wait 하므로 OK).

### 관련 파일

- `src/hooks.rs` `run_lifecycle_hook_commands` (직접 수정 위치)
- `src/hooks.rs` `run_tool_hook_commands` (tool.execute.* 도 같은 패턴, 확인 필요)
- `src/cli/dispatch.rs` (CLI process 종료 직전 wait 호출 추가 위치)

---

