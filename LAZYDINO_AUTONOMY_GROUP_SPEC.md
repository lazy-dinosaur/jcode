# Autonomy Group Spec — M30 + M31 + M35

**Owner**: lazydino
**Date**: 2026-05-12
**Status**: Round 18, design phase
**Predecessor**: M28 (mermaid inline rendering) ✅ DEPLOYED `v0.12.198-dev (a645aa43)`

---

## 1. Problem statement (사용자 한 줄)

> "릴리즈하고 bg 에서 돌리는 툴 결과를 너가 바로 받아서 결과로 사용해야 하는데 그것도 안 됐고… 훅을 다시 결과로 받아서 진행하는게 중요한거지??"

현재 jcode 의 autonomy 는 **사용자가 매번 손으로 결과를 물어야** 진행됩니다. 다음 세 길목에서 자동화가 끊겨 있어요:

1. **M30** — background task 완료 → session 으로 wake 신호 전달 누락 (사용자가 37초 늦게 물어보고서야 발견된 라이브 사례)
2. **M31** — background task 의 stdout 을 LLM 의 다음 turn 컨텍스트에 자동 주입할 path 없음 (사용자가 `bg action="output"` 또는 자연어로 물어야만 받음)
3. **M35** — lifecycle hook (`tool.execute.after`, `response.completed`, `session.stop` 등) 의 stdout 을 LLM 의 다음 turn 에 inject 할 action 없음 (deny 만 가능)

세 길목 모두 **공통 mechanism**: "어떤 외부 이벤트의 결과물을 LLM 의 다음 turn 의 system-reminder 또는 user-injected message 로 자동 추가하고, agent 가 idle 이면 wake 시킨다".

---

## 2. 현재 코드의 사실 관계

### 2.1 Background task 흐름 (`src/background.rs`, `src/background/model.rs`)

- `nohup ... &` / `Bash run_in_background=true` → `spawn_background` (`src/background.rs:200~`).
- 완료 시 `BackgroundTaskStatus.event_history` 에 `BackgroundTaskEventRecord { kind: "completed", status: "completed", exit_code, ts }` push (`src/background/model.rs:99`).
- `notify=true` 옵션이 있긴 하지만 **wake 전달 path 가 없음** (라이브 사례: `event_history` 에 `kind=wake|notify` 가 한 번도 안 찍힘).
- LLM 이 결과를 보려면:
  - 명시적 `bg action="output|wait" task_id="..."` 호출, 또는
  - 사용자가 자연어로 묻고 → LLM 이 위 도구 호출.
- **자동 fan-in 없음**.

### 2.2 Hook 흐름 (`src/hooks.rs`)

```rust
struct HookDecision {
    action: String,      // "allow" | "ask" | "deny"
    reason: Option<String>,
}
```

- `tool.execute.before` (alias `pre_tool_use`) → `deny` ⇒ tool 차단 + `reason` 이 `tool_result` 로 LLM 에 반환 ✅
- `tool.execute.after` (alias `post_tool_use`) → **stdout 회수 안 함**. 결과는 hook 에 input 으로만 흐름. ❌
- `response.completed` → `deny` ⇒ `inject_lifecycle_reminder_for_continuation` (M11 stage 6) 로 다음 user message 에 reminder 삽입 ✅ (이미 동작)
- `session.stop` / `client.disconnect` → 종료 직전, stdout 의미 거의 없음
- `blocking=false` hook → spawn 후 await 안 함 (M10 의 `flush_nonblocking_hooks` 는 종료 직전 정리만)

**Gap**: `tool.execute.after` 의 stdout 을 다음 turn 컨텍스트로 흘려보낼 `inject` action 부재. async hook 결과 회수 path 부재.

### 2.3 LLM 다음 turn trigger 흐름

- 현재 turn 끝 → 사용자가 enter / 슬래시 / Alt+B 등으로 새 input 보내야 다음 inference.
- `inject_lifecycle_reminder_for_continuation` (M11 stage 6) 만 예외적으로 자동 user-message 를 enqueue + turn trigger.
- 즉 "system-injected message" 메커니즘이 이미 1개 존재 ✅ — 이걸 M30/M31/M35 가 재사용하면 됨.

---

## 3. 통합 설계

### 3.1 공통 mechanism: `InjectedContext`

세 milestone 모두 동일한 path 를 씁니다.

```rust
// src/turn/injected_context.rs (신규)

pub enum InjectedSource {
    BackgroundTask { task_id: String, exit_code: i32, stdout: String, stderr: String },
    LifecycleHookStdout { hook_kind: String, tool_name: Option<String>, stdout: String },
    Custom { reason: String, body: String },
}

pub struct InjectedContext {
    pub source: InjectedSource,
    pub timestamp: SystemTime,
    pub format: InjectionFormat,  // SystemReminder | UserMessage
}

/// Enqueue an injection that will be consumed at the start of the next
/// LLM inference turn. If the session is idle (no active turn), this also
/// wakes the agent so the turn starts immediately.
pub fn enqueue_injection(session_id: &SessionId, ctx: InjectedContext) -> Result<()>;
```

**Storage**: `Session.pending_injections: Vec<InjectedContext>` (mem) + `event_history` audit log (이미 있음).

**Wake**: M11 stage 6 의 `inject_lifecycle_reminder_for_continuation` 가 호출하는 동일한 wake-channel 사용.

### 3.2 M30 — Background notify wake

**Symptom**: bg task `notify=true` 가 완료 시점에 session wake 안 함.

**Root cause** (예상, 확정 필요):
- `BackgroundTaskStatus` 완료 push (`background/model.rs:99`) 가 단순 in-mem mutation. session loop 의 wake-channel 에 신호 안 보냄.

**Fix**:
1. `BackgroundTaskHandle` 가 `notify=true` 이면 `tokio::sync::mpsc::Sender<BackgroundCompletion>` (session 단위) 을 가짐.
2. 완료 시 `sender.send(BackgroundCompletion { task_id, exit_code, ... })`.
3. Session loop 의 select! 에 `bg_completion_rx.recv()` 추가 → wake → next inference turn 시작 (단, 다음 LLM call 에 사용자 input 이 없으면 system-reminder-only turn).
4. **단독으로는 LLM 행동 없을 수 있음** — M31 과 결합되어야 의미. 즉 M30 = "신호선", M31 = "신호 받았을 때 context inject".

**검증**:
- 단위 테스트: `notify=true` 로 bg spawn → handle 의 sender 에 메시지 도착.
- 통합 테스트: session 의 wake counter 가 bg complete 후 +1.

### 3.3 M31 — Background result auto-inject

**Symptom**: bg task 완료된 stdout 을 LLM 이 자동으로 안 받음.

**Fix**:
1. `bg` tool input schema 에 새 필드:
   ```rust
   #[serde(default)]
   pub auto_inject: bool,  // default false (opt-in, 사용자 통제)
   #[serde(default)]
   pub auto_inject_format: AutoInjectFormat,  // SystemReminder | UserMessage, default SystemReminder
   #[serde(default)]
   pub auto_inject_max_bytes: usize,  // truncate stdout, default 8000
   ```
2. `auto_inject=true` 면 `notify=true` 도 자동 활성화 (의존성).
3. 완료 시 (`M30` 의 sender 가 message 보냄) → session loop 이 `InjectedContext::BackgroundTask` 를 만들어 `enqueue_injection` 호출.
4. 다음 inference turn 시작 시 (이미 M11 stage 6 가 쓰는 path) 시스템 리마인더로 prepend:
   ```
   [system-reminder]
   Background task completed.
   - task_id: 297670ykyb
   - exit_code: 0
   - duration: 128.4s
   - stdout (truncated to 8000 bytes):
   <stdout body>
   - stderr (truncated to 2000 bytes):
   <stderr body>
   ```
5. LLM 이 자연스럽게 다음 응답에서 그 결과를 활용.

**Edge cases**:
- session 이 다른 turn 진행 중 → injection 큐에 쌓아두고, 다음 turn 시작 시 일괄 prepend.
- bg task 가 매우 많이 동시 완료 → 시간순으로 모두 prepend (최근 5개 max).
- exit_code ≠ 0 → 동일 format, LLM 이 알아서 retry/abort 결정.
- 사용자가 input 입력 중 → bg complete event 가 queue 에 쌓이고 사용자 turn 시작 시 prepend.

**API 변경**:
- `bg` tool schema 만 추가. 기존 호출자 backward-compat (default false).

### 3.4 M35 — Lifecycle hook stdout auto-inject

**Symptom**: `tool.execute.after` 등 hook 의 stdout 이 LLM 에 안 전달됨.

**Fix**:
1. `HookDecision` 에 새 필드:
   ```rust
   struct HookDecision {
       action: String,                    // 기존: "allow" | "ask" | "deny"
       reason: Option<String>,            // 기존
       inject: Option<HookInjectPayload>, // 신규
   }
   struct HookInjectPayload {
       body: String,                      // 다음 turn 에 prepend 될 텍스트
       #[serde(default)]
       format: InjectionFormat,           // SystemReminder | UserMessage
   }
   ```
2. Hook command (사용자 스크립트) 가 stdout 으로 JSON 출력하면:
   ```json
   { "action": "allow", "inject": { "body": "user repo has lint errors:\n- ...", "format": "system_reminder" } }
   ```
3. `parse_hook_decision_stdout` 이 `inject` 필드 인식 → `enqueue_injection` 호출.
4. **`tool.execute.after` 의 hook stdout 도** decision-JSON parse 대상 (현재는 `tool.execute.before` 만 parse). 모든 hook kind 에서 동일 path.
5. `blocking=false` hook 의 결과 회수:
   - spawn 시 join handle 보관.
   - 다음 turn 시작 직전에 `try_join_all_with_timeout(remaining_handles, 50ms)` → 완료된 것만 inject, 미완료는 다음 turn 으로.

**보안**: `inject.body` 는 max 16KB truncate, control char strip.

**검증**:
- 단위 테스트: `HookDecision` deserialize — `{"action":"allow","inject":{"body":"x"}}` 정상 parse.
- 통합 테스트: 실제 hook 스크립트 (echo JSON) 가 다음 turn 에서 system-reminder 로 보임.

---

## 4. 마이그레이션 / 순서

| 단계 | 작업 | 의존 | 검증 |
|---|---|---|---|
| 1 | `InjectedContext` 모듈 + `enqueue_injection` API (M11 wake path 재사용) | 없음 | unit test: 큐 push/pop, wake counter |
| 2 | M30 — bg `notify=true` 완료 시 mpsc 신호 | 1 | unit + integration: bg complete → session wake +1 |
| 3 | M31 — `bg.auto_inject=true` 가 InjectedContext::BackgroundTask 만들어 enqueue | 1, 2 | E2E: nohup → 완료 → 다음 LLM turn 의 system-reminder 에 stdout 포함 |
| 4 | M35 — `HookDecision.inject` 필드 + 모든 hook kind 의 stdout parse | 1 | unit: deserialize, E2E: hook 스크립트 → 다음 turn 컨텍스트에 body 등장 |
| 5 | M35 — `blocking=false` hook async 결과 회수 (timeout join) | 4 | unit: 50ms timeout, 통합: 빠른 hook 결과 즉시, 느린 hook 다음다음 turn |
| 6 | 통합 라이브 검증 + 문서 update + commit + push | 1~5 | 사용자 실제 워크플로우로 검증 |

---

## 5. 단위 테스트 명세

```rust
// tests/autonomy_group.rs (신규)

#[tokio::test]
async fn injected_context_queue_appends_in_order() { ... }

#[tokio::test]
async fn enqueue_injection_wakes_idle_session() { ... }

#[tokio::test]
async fn m30_background_task_completion_signals_session_wake() {
    let session = test_session().await;
    let task_id = spawn_bg_with_notify(&session, "sleep 0.1 && echo done").await;
    wait_for_event(&session, EventKind::Wake, Duration::from_secs(2)).await;
    let task = session.bg_status(&task_id).unwrap();
    assert_eq!(task.exit_code, Some(0));
    assert!(task.event_history.iter().any(|e| e.kind == "wake"));
}

#[tokio::test]
async fn m31_background_auto_inject_appears_in_next_turn() {
    let session = test_session().await;
    spawn_bg_with_auto_inject(&session, "echo 'hello from bg'", auto_inject = true).await;
    let next_turn_prompt = session.next_turn_system_reminders().await;
    assert!(next_turn_prompt.contains("hello from bg"));
    assert!(next_turn_prompt.contains("Background task completed"));
}

#[tokio::test]
async fn m35_hook_inject_action_propagates_to_next_turn() {
    let session = test_session_with_hook(
        "tool.execute.after",
        r#"echo '{"action":"allow","inject":{"body":"hook says hi","format":"system_reminder"}}'"#,
    ).await;
    session.run_tool_call("Read", json!({"file_path":"/etc/hosts"})).await;
    let next_turn_prompt = session.next_turn_system_reminders().await;
    assert!(next_turn_prompt.contains("hook says hi"));
}

#[tokio::test]
async fn m35_nonblocking_hook_result_collected_with_timeout() { ... }

#[tokio::test]
async fn m35_hook_inject_body_truncated_at_max_bytes() { ... }
```

---

## 6. 위험 및 완화

| 위험 | 영향 | 완화 |
|---|---|---|
| Injection 폭주 (bg 100개 동시 완료) | LLM context overflow | max 5개/turn, 나머지 다음 turn 으로 |
| Injection 이 사용자 입력보다 먼저 prepend 되어 혼동 | UX 저하 | system-reminder format 명확히 분리, "[system]" prefix |
| Hook 이 악의적 큰 inject 시도 | context 폭주 | max 16KB hard truncate |
| `auto_inject=true` 가 default false 인데 사용자 발견 못 함 | UX, autonomy 안 살아남 | `~/.jcode/config.toml` `[bg] default_auto_inject = true` 옵션 |
| M30 wake 가 이미 진행 중인 turn 을 중단 | race | wake 는 idle 일 때만 trigger, busy 는 큐만 |
| Hook 호환성 (기존 사용자 hook 스크립트) | regression | `inject` 가 optional field — 없으면 기존 동작 |
| `blocking=false` hook 의 timeout 이 너무 길면 turn 지연 | latency | 기본 50ms, 사용자 설정 가능 |

---

## 7. Out of scope (별개 milestone)

- M32 (streaming sibling fanout 회귀) — autonomy 와 무관
- M17 (main↔swarm queue) — autonomy 와 결이 비슷하지만 별개 큐
- M27 (busy-agent history) — 다른 visibility 문제
- M24 (tool 병렬화) — 성능 최적화
- M16 (OAuth ToolDefinition 통일) — schema 문제
- M33 (클립보드) — UX
- M23 (build retention) — infra

---

## 8. 성공 기준 (verifiable)

1. ✅ `nohup ... &` 띄운 후 `auto_inject=true` 옵션으로 → 완료되면 사용자가 묻지 않아도 LLM 의 다음 turn 첫 줄에 결과가 prepend 되어 LLM 이 자연스럽게 다음 행동 결정.
2. ✅ Hook 스크립트가 `{"action":"allow","inject":{"body":"..."}}` 출력 → 다음 turn 에서 LLM 이 그 body 를 본 것처럼 응답.
3. ✅ session 이 사용자 입력 대기 (LLM idle) 상태에서 bg 완료 → 사용자 input 없어도 자동 wake 후 inject 만으로 turn 진행.
4. ✅ 기존 `bg` / hook 사용자 스크립트 변경 없이 동작 (backward compat).
5. ✅ 5개 새 단위 테스트 PASS, 1개 통합 테스트 PASS.
6. ✅ 사용자 라이브 검증 — 한 round 안에 "결과 어떻게 됐어?" 라고 묻지 않고 끝나는 워크플로우 1개 이상 시연.

---

## 9. 구현 순서 추정

| 단계 | 예상 시간 | 비고 |
|---|---|---|
| 1. InjectedContext 모듈 | 30~45min | M11 path 재사용 |
| 2. M30 wake signal | 30~45min | mpsc 추가, select! 분기 |
| 3. M31 bg auto_inject | 45~60min | schema + flow + test |
| 4. M35 HookDecision.inject + parse | 45~60min | 모든 hook kind 적용 |
| 5. M35 async hook 회수 | 30~45min | timeout join |
| 6. 통합 라이브 검증 | 30min | E2E 워크플로우 1개 |
| **총** | **~4~5시간** | 한 round 안에 완료 가능 |

---

## 10. 결정 필요 사항 (사용자 confirm 요청)

- [ ] **A.** `bg.auto_inject` 의 default 값: **false (opt-in)** vs **true (opt-out)** — 추천: false (안전), config 로 항상 true 가능.
- [ ] **B.** Injection format default: `SystemReminder` vs `UserMessage` — 추천: SystemReminder (LLM 이 reminders 채널 더 잘 따름).
- [ ] **C.** Hook stdout truncation 한도: 16KB vs 32KB — 추천: 16KB.
- [ ] **D.** M30 단독으로 가시화 (예: log) 만 하고 M31 자동 inject 와 분리 배포할지 vs 같이 — 추천: 같이 (M30 단독은 의미 없음).
- [ ] **E.** 한 turn 당 max injections: **5개** vs **무제한** — 추천: 5개.

위 5개에 답해주시면 바로 구현 시작합니다. 없어도 추천값으로 진행 가능.
