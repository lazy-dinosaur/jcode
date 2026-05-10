| M19 | **Config hot-reload 부재** — `Config::load()` (src/config.rs:23) 가 `OnceLock::get_or_init` 으로 한 번만 로드. 사용자가 `~/.jcode/config.toml` 변경해도 server process 살아있는 동안 절대 reflect 안 됨. 매번 `/restart` 또는 server 재기동 필요 → 검증 사이클 마찰 큼 (M9 라이브 검증 중 발견, 2026-05-10 15:05 UTC). MCP config 는 reload 가 있는데 (src/mcp/pool.rs:243 `*self.config.write().await = McpConfig::load()`) 일반 Config 는 없음 | 🔴 OPEN — 미설계. **구현 후보**: (a) MCP 처럼 `Config` 를 `Arc<RwLock<Config>>` 로 바꾸고 reload helper 추가, (b) `/reload-config` 슬래시 명령 또는 `Request::ReloadConfig` 신설, (c) file watcher (notify crate) 로 자동 reload. 위험: config 가 광범위하게 쓰여서 (`Config::load()` 호출 site 가 많음) `&'static Config` 라는 가정 깨면 도미노 발생. 점진적 접근 권장: 먼저 hooks 만 hot-reloadable, 다른 필드는 나중. **검증 사이클 영향**: M9/M10/M11 라이브 검증 시 config 수정마다 `/restart` 필요해서 기억에 남았음 | Medium (UX) |
# Lazydino Jcode — Outstanding Milestones

마지막 업데이트: 2026-05-10
관련 문서: `LAZYDINO_MAINTENANCE.md` (커밋된 패치 이력)

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
| M2  | Swarm 버그 (phase C diagnostics + upstream #76)    | 🔴 OPEN — 증상 미확정             | Medium-High |
| M3  | Hook 시스템 확장 (`session.stop`, `response.completed`) | ✅ **DONE** (commit `003fcf65` + `1c97ef70`, binary `1c97ef70`) | — |
| M4  | TUI interleave 가 tool 완료 후에야 흡수됨 (jcode 원설계) | 🟡 BY-DESIGN with caveats — UX 개선 가치 | Medium |
| M5  | Alt+B early race — `background_tool_signal.reset()` 이 너무 늦음 | ✅ **DONE** (commit `52375aac` + `f76dfdda`, binary `f76dfdda`) | — |
| M6  | Alt+B 후 부모 turn 이 idle 빠져도 background task 완료 시 자동 wake | ✅ **DONE** (commit `f2c8430f` + `074dcbef`, binary `074dcbef`) — 라이브 30s idle wake 검증 완료 (T-PLAN 2026-05-10 12:36 UTC) | — |
| M7  | 비정상 종료 시 메시지 유실 — `/reload`, server SIGKILL/crash 시 `/save` 안 부른 채로 종료하면 모든 메시지 날아감 | ✅ **DONE** (commit `5f597b98`, binary `5f597b98`) — `load_startup_stub` fallback 이 journal replay 하도록 fix + 회귀 테스트 추가 | **Critical (데이터 유실)** |
| M8  | Alt+B detach 후 turn 이 안 끝나서 TUI `is_processing=true` 로 묶임 (queued 메시지 dispatch 안 됨) | ✅ **DONE** (commit `4e3d8189`, binary `4e3d8189`) — detach branch 가 skipped ToolResult fill + `return Ok(())` 로 turn 즉시 종료. 회귀 테스트 `turn_streaming_mpsc_altb_ends_turn_immediately_after_detach` 추가 (provider_calls == 1 assert) | High |
| M9  | Hook 이중 발동 — `~/.jcode/config.toml` 이 home 하위 working_dir 에서 글로벌+project local 양쪽으로 read (자기참조 skip 없음, 코드 read 로 확정) | ✅ **DONE** (commit `82b7c81f` on `deploy/m9-m10`, source `9774d9a6` on `patch/hook-config-dedupe`, binary `82b7c81f`) — `Config::hooks_for_working_dir` 가 `paths_resolve_to_same_file` (canonicalize+lexical fallback) 로 global path == project-discovered path 인 경우 merge skip. 회귀 테스트 2건: dedupe 동작 + distinct path merge 보존 | — |
| M10 | Non-blocking lifecycle hook 이 단발성 CLI (`jcode run`) 에서 race 로 누락 — `tokio::spawn` fire-and-forget + 즉시 process 종료 | ✅ **DONE** (commit `13dd3132` on `deploy/m9-m10`, source `e74791df` on `patch/lifecycle-hook-cli-flush`, binary `82b7c81f`) — `pending_nonblocking_hooks()` (`OnceLock<Mutex<Vec<JoinHandle>>>`) + `spawn_tracked_nonblocking_hook` + `flush_nonblocking_hooks(timeout)` 추가. 두 spawn site (tool nonblocking, lifecycle nonblocking) 가 tracked spawn 사용. `cli/startup.rs::run` 가 dispatch 종료 후 `flush_nonblocking_hooks(5s)` 호출. 회귀 테스트 3건 (serial mutex 로 process-global state 보호): tracked handle 대기, 빈 슬롯 short-circuit, timeout 경계 | — |
| M11 | Lifecycle hook (`response.completed`, `session.stop`) 의 `decision.action` JSON 이 무시됨 — `run_blocking_lifecycle_hook` 가 exit code 만 보고 stdout JSON 파싱 안 함. 따라서 timsquad/claude-code 의 `Stop` 패턴 ("hook 이 reason 주입 → AI 다음 응답에 강제 작업") 동등 동작 불가. `tool.execute.before` deny 와 비대칭 (tool hook 만 stdout JSON 파싱) | 🔴 OPEN — 코드 검증 완료 (`src/hooks.rs:302-320`, `src/config/default_file.rs:281` 가 한계 명시). 5단계 patch 필요: (1) lifecycle decision parsing, (2) reason → next-turn system reminder inject (현재 turn 이 break 직전이므로 새 turn 시작 메커니즘 필요), (3) `stop_hook_active` 무한루프 방지 (N회 연속 deny 시 hard-stop), (4) `session.stop` vs `client.disconnect` 의미 분리 (M12 후속), (5) **payload context 보강 — `last_user_message`, `recent_tool_calls` 등 hook script 가 검증할 단서 같이 전달** | High (framework 게이트 강제력) |
| M12 | Anthropic OAuth provider 가 `ToolSearch` 를 AI 에게 광고하는데 dispatch 핸들러 미구현 — AI 가 호출하면 `Unknown tool: ToolSearch` 에러 | ✅ **DONE** (commit `c14ffdf8`, branch `patch/anthropic-oauth-tool-schema-align`) — `crates/jcode-provider-core/src/anthropic.rs` 에 `ToolSearch <-> codesearch` 양방향 매핑 추가 + `src/provider/anthropic.rs` 의 광고 schema 를 `CodeSearchTool` dispatch 와 align (required `query`, dead `max_results` 제거). 회귀 테스트 `test_oauth_tool_search_advertised_schema_matches_codesearch_dispatch` 추가 | — |
| M13 | `schedule` (ScheduleWakeup) 도구 schema 가 AI 호출 형식과 불일치 — AI 가 호출하면 `missing field 'task'` 에러 | ✅ **DONE** (commit `c14ffdf8`, branch `patch/anthropic-oauth-tool-schema-align`) — `src/provider/anthropic.rs` 의 `ScheduleWakeup` 광고 schema 를 `ScheduleTool` dispatch 와 align (required `task`, dead `delaySeconds`/`reason`/`prompt` 제거). 회귀 테스트 `test_oauth_schedule_tool_advertised_schema_matches_dispatch` 추가 | — |
| M14 | `/compact` (대화 컨텍스트 축약 명령) 동작 안 함 — 사용자 보고 (2026-05-10 13:28 UTC). 진단 미착수 | ✅ **DONE** (commit `e71713ba`, branch `patch/compaction-failure-cooldown`) — 진단 결과 "동작 안 함" 의 실체는 **요약기가 한 번 실패한 뒤 proactive/semantic auto-trigger 가 매 turn 마다 재발화** 였음. 실패 path 가 `turns_since_last_compact` 를 reset 안 해서 cooldown 무력화됨. 새 streak counter (`MAX_CONSECUTIVE_COMPACTION_FAILURES=3`) + `note_compaction_success/failure` helper + `should_compact_with` short-circuit 추가. 회귀 테스트 4건 (`test_note_compaction_*`, `test_should_compact_with_short_circuits_after_failure_streak`) | — |
| M14a | **Emergency compaction 무한 루프** — 라이브 22회 연속 emergency compaction (사용자 13:42 UTC 보고). 504k→20k→500k 패턴 반복. retry counter `MAX_CONTEXT_LIMIT_RETRIES=5` 는 이미 존재하지만 무력화됨 — 카운터가 reset 되는 경로가 있다는 뜻 | ✅ **DONE** (commit `e71713ba`, branch `patch/compaction-failure-cooldown`) — M14 와 동일한 streak counter 를 `Agent::try_auto_compact_after_context_limit` 와 `ensure_context_fits` 의 critical-threshold hard-compact 에 공유. per-turn `MAX_CONTEXT_LIMIT_RETRIES` 가 못 보는 session-wide 반복을 streak 가 차단함. 22회 같은 폭주 패턴은 이제 3회 후 emergency 진입 자체가 거부되며 그대로 turn-level retry budget 이 reject → AI 에게 context-limit error propagation. 라이브 재현은 다음 jcode session 에서 검증 필요 | — |
| M15 | Jcode TUI 에 첨부된 이미지가 외부 jcode session (지금 우리 대화하는 session) 에는 alt-text 만 들어오고 실제 image data 가 안 옴 — 디버그 사이클 효율 영향 | 🔴 OPEN — 라이브 재현 다수 (2026-05-10 13:24~13:27 UTC). 코드 분석 완료 (T-M15 2026-05-10 14:50 UTC, searcher 247s): protocol 단의 `HistoryMessage.content` 가 text-only 라 image bytes 가 sibling `ServerEvent::History.images: Vec<RenderedImage>` 로만 흐름. 3개 drop candidate: (A) `crates/jcode-protocol/src/lib.rs:47-56` `HistoryMessage` 에 image 필드 없음, (B) `src/server/client_api.rs:154-159` `Client::get_history()` 가 `images` 필드 명시적으로 버림 (가장 작은 fix), (C) `src/server/client_lifecycle.rs:2658-2669` 라이브 attached 형제 client 에 user-message-with-images 이벤트 fanout 안 함. provider 광고 path (Anthropic/OpenAI/OpenRouter/Gemini) 는 image bytes 정상 보존 → AI 한테는 가지만 외부 client 한테는 안 감 | Low-Medium (debug UX) |
| M16 | **Anthropic OAuth provider 가 hand-rolled JSON 으로 도구 광고** — 다른 provider (OpenAI/Gemini/Copilot/Cursor/Bedrock/Antigravity) 는 모두 `ToolDefinition.input_schema` (ToolRegistry single source of truth) 를 직접 직렬화해서 광고함. Anthropic OAuth 만 hard-coded JSON 11개 도구 화이트리스트 → stale schema 위험 영구 존재 (M12/M13 발생지). 근본 해결: Anthropic OAuth 도 ToolDefinition 기반 광고로 통일 | 🔴 OPEN — 코드 위치: `src/provider/anthropic.rs::format_tools` (is_oauth=true branch). 작업 범위: (a) ToolRegistry 에서 광고할 OAuth 화이트리스트 정의, (b) 도구 이름 매핑 (`bash`→`Bash` 등) 을 광고 단계에서 자동 적용, (c) cache_control breakpoint 로직 보존, (d) 회귀 테스트 — 광고 schema 가 모든 ToolDefinition.input_schema 와 항상 일치 | Medium-High (구조 개선, 미래 회귀 방지) |
| M17 | **Live turn handoff to swarm member (claude-code parity)** — main session 이 긴 turn 도는 동안 다음 요청을 보내면 queue 에 들어감. 이 queue 로 가는 흐름을 swarm 으로 바로 보내고 싶음. fork (turn snapshot 떠서 subagent 화) 는 race + conversation 분기로 복잡 → 대신 **queue 를 swarm 으로 라우팅** 하는 접근 선택. 결과 회수는 swarm completion + M6 idle wake path 가 자동 처리 | 🔴 OPEN — 미설계. **두 가지 옵션 후보**: **(A) queue cancel + swarm 재발행** — 사용자가 평소처럼 다음 메시지 enqueue, 그 후  로 queue 의 마지막 항목을 dequeue + swarm spawn. **(B) queue skip + direct swarm** —  슬래시명령 한 번에 queue 안 거치고 바로 swarm spawn (B1=one-shot, B2=sticky toggle 모드). **B1 이 가장 단순**: 기존 swarm spawn tool 이 이미 있으니 TUI 슬래시명령 한 줄 추가 ~50줄. 결과 회수: swarm completion notification → M6 wake path → 사용자가 보고 싶을 때 확인. **결정 필요한 디테일**: (a) A vs B 중 선택 (또는 둘 다), (b) sticky 모드 필요 여부, (c) main conversation 에 swarm 위임 흔적 표시 방식 (시스템 메시지 한 줄 vs 보이지 않게) | High (사용자 워크플로우) |
| M18 | **SDK `Client::get_history()` 가 image bytes 명시적으로 drop** (M15 의 가장 좁은 fix candidate B) — `src/server/client_api.rs:154-159` 가 `ServerEvent::History { messages, images, .. }` 에서 `messages` 만 반환하고 `images` 는 `..` 로 버림. SDK consumer 가 image 데이터 못 받는 직접 원인. M15 의 sub-set 으로 분리 — fix 가 한 함수 추가 (~10줄) 로 해결 가능 | 🟡 PATCH READY — `patch/sdk-history-images` commit `5b7f6172 fix(m18): preserve images in SDK history API`. `Client::get_history_with_images() -> Result<(Vec<HistoryMessage>, Vec<RenderedImage>)>` 추가, 기존 `get_history()` 는 backward compat 유지. 회귀 테스트 2건 (`server::client_api::tests::m18_*`) 추가. 검증: `cargo +nightly test --lib server::client_api::tests::m18 -- --test-threads=1 --nocapture` → 2 passed. Default stable rustc 1.90.0 은 현재 upstream AWS crates MSRV 1.91.1 요구로 test 불가 | Low (작은 SDK 추가) |
| M19 | **Config hot-reload** — `~/.jcode/config.toml` 편집 시 `OnceLock<Config>` 가 옛 값을 캐시한 채로 process 종료까지 반영 안 됨. 장시간 떠 있는 TUI server 의 root cause 로, M9 hook fix 가 binary 는 새 것이지만 config 는 옛 것이라 효과를 못 보는 사례 발생 | ✅ **DONE** (commit `96445a80` fix + `da4fe48f` test on `deploy/m9-m10`, source `93282b44`/`bdf63f5c` on `patch/config-hot-reload`, binary `lazydino-da4fe48f`) — `OnceLock<Mutex<ConfigCache>>` + 매 호출 `stat()` + 500ms debounce + `Box::leak` (기존 `&'static Config` API 보존). last-good fallback 으로 toml 파싱 깨짐에 대해 default 로 안 떨어짐. test 5건 (`test_m19_config_*`) 모두 통과 (`config::tests::test_m19 → 5 passed`) | — |
| M20 | **jcode bash tool 의 2분 hard timeout** — `src/tool/bash.rs:23` 의 `DEFAULT_TIMEOUT_MS: u64 = 120000` 하드코딩 때문에 cargo test/build, 긴 sleep, stress 검증이 모두 2분에 강제 background 이동. nohup+disown 으로 process 자체는 살릴 수 있지만 호출 메시지가 끊겨 결과 폴링이 강제됨 | ✅ **DONE** (commit `28b524ba` fix + `e6f98968` test on `deploy/m9-m10`, source `11edbb91`/`10db6a96` on `patch/bash-tool-timeout`, binary `lazydino-da4fe48f`) — 새 `[tool.bash]` config 섹션 (`ToolConfig`/`BashToolConfig`) 도입. default 5min, max 20min (`HARD_CAP_MS`). `resolve_timeout_ms(requested)` helper 가 세 사용처 (foreground/agent-turn/background) 통합. schema description 에 5min default / 20min cap / `[tool.bash]` config knob 명시. test 5건 (`tool::bash::tests::test_m20_*`) 모두 통과 | — |
| M21 | **upstream rebase + dedupe** — `fork/master` 가 `origin/master (1jehuang/jcode)` 보다 226 commit 뒤짐. 그 226 안에 우리 fix 와 같은 영역의 commit 다수: `Move compaction emergency/estimation/contracts into core` (M14/M14a 영역), `Keep restored remote queues pending across reload` (M7 영역), 그리고 가장 위험한 대규모 "Extract/Move into core crates" refactor 30+건 — 우리 patch 들이 변경한 `src/*` 위치가 새 `crates/*-core/` 로 이동. fork 에 우리 작업 push 시도하면 `workflow` scope 부족 + (그 후) reapply 시 path mismatch 충돌 폭탄 가능 | 🔴 OPEN — 백업 완료 (tag `backup/pre-upstream-rebase-20260511-0136/{deploy-m9-m10, <patch-name>...}` 48개). 절차: (1) PAT 에 `workflow` scope 추가, (2) `fork/master` 를 `origin/master` 로 fast-forward push (226 commit), (3) deploy/m9-m10 를 새 origin/master 위로 rebase — 충돌 풀면서 upstream 이 이미 한 fix 와 우리 fix 중복인지 검증 (예: M14 의 emergency cooldown 이 upstream `4ac1df1b` 에 흡수됐는지), (4) 살아남은 patch 만 새로 push. 작업량 큼 (예상 2-4h, dedupe 분석 포함) | High (upstream 동기화 안 하면 다음 fix 들도 같은 path 충돌 누적) |

⚠️ **운영 노트** (2026-05-10): 모든 fix 가 binary 까지 깔려도 **이미 띄워진 jcode server process 는 옛 binary 를 메모리에 들고 있음**. 새 patch 효과를 보려면 사용자가 `/reload` 또는 `/restart` 로 server 재시작 필요. 이걸 안 해서 M5/M1 효과가 안 보이는 것처럼 느껴진 적이 있음 (현재 PID 479123 = 18:12 시작 server, M5 빌드 이전).

핵심 인과 관계:
- ✅ M5 (완료): Alt+B race fix.
- ✅ M1 (완료): Background task delivery 가 parent/report-back chain 을 따라가도록 라우팅 수정. parent chain resolver + headless drain false-live check + delivery_session_id field 도입.
- M4 는 upstream 원설계. M5+M1 완료로 사용자 입장에서 "Alt+B 동작 + 결과 회수" 정상 동작 회복. interleave 자체의 latency 는 여전히 by-design.
- M6 는 upstream 의도 변경이라 보류.
- 다음 작업: **M2 (swarm)** 또는 **M3 (hook)** 중 선택.

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

상태: **🔴 OPEN — 증상/원인 일부만 정리, 추가 재현 필요**
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

