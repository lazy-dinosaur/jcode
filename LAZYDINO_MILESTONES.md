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
| M6  | Alt+B 후 부모 turn idle 화 (upstream 동작 변경) | 🔵 DEFERRED — upstream 의도 변경이라 기본 버그/문제 정리 후 진행 | Low (deferred) |

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
