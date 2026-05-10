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
| M1  | Background task delivery 가 parent/report-back chain 을 안 따라감 | 🔴 OPEN — 이중 검증 완료, 수정 미착수 | High |
| M2  | Swarm 버그 (phase C diagnostics + upstream #76)    | 🔴 OPEN — 증상 미확정             | Medium-High |
| M3  | Hook 시스템 확장 (`session.stop`, `response.completed`) | 🟡 PARTIAL — 명세만 존재          | Medium      |
| M4  | TUI interleave 가 tool 완료 후에야 흡수됨 (jcode 원설계) | 🟡 BY-DESIGN with caveats — UX 개선 가치 | Medium |
| M5  | Alt+B early race — `background_tool_signal.reset()` 이 너무 늦음 | 🔴 OPEN — root cause 확정 | **Highest** (가장 단순, 효과 큼) |

핵심 인과 관계:
- M5 는 Alt+B 자체가 종종 무시되는 별도 race. 가장 먼저 고침.
- M1 은 Alt+B 가 성공한 뒤 parent 로 알림/회수가 안 가는 라우팅 버그. M5 와 별개.
- M4 는 jcode 원설계지만 M5/M1 이 깨져 있어서 사용자 입장에선 "전부 막힘" 으로 보임. M5+M1 고치면 자연 완화됨. UX 문구/옵션은 별도 작은 patch 로.

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

## 📍 Milestone M5 — Alt+B early race: `background_tool_signal.reset()` 이 너무 늦음

상태: **🔴 OPEN — root cause 확정 (oracle, 2026-05-10)**
우선순위: **Highest** (가장 단순한 수정, 가장 큰 즉각적 UX 개선)

### 증상 (사용자 보고, 2026-05-10)
- 첫 subagent → Alt+B → bg 진입 성공 (`Tool 'subagent' was moved to background by the user`).
- 같은 부모 세션에서 두 번째 subagent (또는 빠른 연속 Alt+B) 시 **bg 진입 자체가 무시됨**.
- TUI 는 "Moving tool to background..." 만 표시하고 풀리지 않음.

### Root cause (oracle 검증, 2026-05-10) — **확정**

`BackgroundToolSignal` 의 reset 타이밍이 ToolStart 노출보다 너무 늦어서 사용자가 빨리 누른 Alt+B 가 reset 으로 지워지는 race.

순서:
1. 모델 stream 처리: `turn_streaming_mpsc.rs:280~312` 에서 `ServerEvent::ToolStart { id, name }` 을 TUI 로 전송.
2. TUI: `src/tui/app/remote/server_events.rs:76~85` 에서 `app.status = ProcessingStatus::RunningTool(name)` 으로 전환.
3. 사용자가 즉시 Alt+B → `src/tui/app/remote/key_handling.rs:333~339` 의 가드 통과 → `remote.background_tool()`.
4. server: `src/server/client_lifecycle.rs:2789~2796` `move_tool_to_background` → `session_control.request_background_current_tool()` → `signal.fire()` 로 SET.
5. 그제서야 agent 코드가 tool spawn 후:
   ```rust
   // src/agent/turn_streaming_mpsc.rs:820~831
   let tool_handle = tokio::spawn(async move { /* tool 실행 */ });
   self.background_tool_signal.reset();   // ← 방금 fire 된 signal 을 지움
   ```
6. 이후 `tokio::select!` 에서 `bg_signal.notified()` 를 기다리지만 signal 은 이미 reset 됨 → tool 끝까지 그대로 진행.

이 race 는 첫 tool 이든 두 번째 tool 이든 발생 가능. 사용자가 "두 번째에서 더 잘 일어남" 으로 느낀 이유는 첫 번째 시도 후 M1 영향으로 부모 turn 이 idle 안 된 상태에서 다음 tool 이 시작될 때 Alt+B 를 더 빨리 누르게 되기 때문일 가능성.

### 정정 사항 (이전 가설 반박)
- "BackgroundToolSignal reset 누락" — **반박**, 두 reset 호출 모두 존재 (line 831 spawn 직후, line 1017 finally).
- "BackgroundTaskManager.adopt 의 동시 task 제한" — **반박**, unique task id 로 insert 만 하며 제한 없음.
- "Alt+B 키 가드가 두 번째 시점에 false" — 가능성 있으나 주된 원인은 아님. tool 사이/streaming 중 `RunningTool` 이 아닐 때 Alt+B 가 word-backward 로 처리되는 부수 케이스는 있음.

### 결정적 코드 근거
- `src/agent/turn_streaming_mpsc.rs:820~831` reset 위치 (너무 늦음).
- `src/agent/turn_streaming_mpsc.rs:280~312` ToolStart 발행 (TUI 가 RunningTool 전환).
- `src/tui/app/remote/server_events.rs:76~85` TUI status 전환.
- `src/tui/app/remote/key_handling.rs:333~339` Alt+B 가드 + `remote.background_tool()`.
- `src/server/client_lifecycle.rs:2789~2796` `move_tool_to_background` 가 무조건 Ack 반환 (실패 시그널 없음).
- `src/server/state.rs:473~480` `request_background_current_tool` 의 fire 동작.

### upstream PR/issue 조사 (2026-05-10)
- **동일 race fix PR/issue 없음.** Alt+B 자체를 추가한 #99ef05cae 이후 timing 패치 없음.

### 해결 방향 (oracle 권고)

1. **reset 위치를 ToolStart 이전으로 이동** (가장 직접적)
   - 새 assistant stream 또는 새 tool collection 시작 전, ToolStart 이벤트가 TUI 로 노출되기 전에만 reset.
   - 이미 fire 된 signal 을 보존하는 latch semantics.
2. **`move_tool_to_background` 의 ack 분리**
   - `request_background_current_tool()` 반환값이 false 면 다른 ack/status 또는 명시적 실패 이벤트.
   - 현재는 무조건 `Ack` 라 사용자가 실패를 알 길이 없음.
3. **TUI status notice 의 acknowledged transition**
   - `Moving tool to background...` 가 실제 `ToolDone` with background message 를 받았을 때 `"Moved tool to background"` 로 명시적 전환.
   - 실패 ack/event 가 오면 실패 표시.
   - 현재는 3 초 TTL 자연 소멸만 됨.

선호: 1 + 2 + 3 모두 작은 패치로 묶어서.

### 회귀 위험
- reset 위치를 잘못 옮기면 이전 tool 의 stale Alt+B 가 다음 tool 을 즉시 background 로 보내는 회귀.
- latch semantics 변경은 double Alt+B, cancel, reload handoff 와 상호작용 가능.
- ToolStart 전엔 사용자가 Alt+B 를 누를 수 없어야 한다는 UI invariant 깨면 예상치 못한 detach 발생.

### 검증 시나리오
1. early Alt+B race 재현:
   - mock provider 가 `ToolStart`/`ToolExec` emit
   - tool wait loop 가 reset 하기 전에 `request_background_current_tool()` 호출
   - 기존 코드: signal 잃음 / 패치 후: background adopt 발생
2. second tool Alt+B:
   - 한 turn 에 tool 2 개. 첫 tool Alt+B → 두 번째 tool Alt+B → 양쪽 모두 background task 등록.
3. status guard false UX:
   - status 가 `Streaming`/`Thinking` 일 때 Alt+B 가 background request 보내지 않음을 명시.
   - status notice 가 misleading 하지 않도록 (이 경우 word-backward 동작이 자연스러움).
4. no stale signal regression:
   - tool 사이 stale Alt+B signal 이 다음 tool 을 자동 background 로 보내지 않아야 함.

### 관련 파일
- `src/agent/turn_streaming_mpsc.rs:280~312, 820~831, 1010~1020` reset 위치, ToolStart 발행
- `src/agent/turn_streaming_broadcast.rs` 동일 구조
- `src/tui/app/remote/key_handling.rs:333~339` Alt+B 가드
- `src/server/client_lifecycle.rs:2789~2796` `move_tool_to_background`
- `src/server/state.rs:473~480` `request_background_current_tool`
- `BackgroundToolSignal` 정의 (위 파일들에서 import 추적)

---

## 우선순위와 작업 순서 (2026-05-10 갱신)

oracle 이중 검증 결과를 반영한 우선순위:

1. **M5 먼저** — Alt+B race. 가장 단순한 수정 (reset 위치 이동 + ack 개선 + status transition). 즉각적 UX 효과 큼.
2. **M1** — background task delivery target. M5 가 고쳐져도 parent 회수가 안 되면 여전히 답답함. 다소 큰 변경 (BackgroundTaskCompleted/Progress 에 delivery_session_id 추가, dispatch 라우팅 변경, headless live check 보정).
3. **M4 옵션 1 만 빠르게** — 상태 메시지 문구 개선 (`"⏭ Will be processed after current tool"`). 옵션 2~4 는 후순위. 한 줄짜리 patch.
4. **M3** — hook event 두 개 추가. M1 끝낸 뒤 작업.
5. **M2** — phase C swarm-diagnostics stash 회수 → upstream #76 시나리오 재현 → 추가 패치.

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
