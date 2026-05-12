# M35: Lifecycle Hook Stdout → LLM Inject + 자동 Continuation

**상태**: ✅ **DONE** (Round 22, 2026-05-12)
**최종 commit**: `65c52089` (`Feat(m35): default cap 3 → 0 = claude-code compatible trust mode`)
**Binary**: `~/.jcode/builds/versions/lazydino-65c52089/jcode` (sha256 `d7a5094dc45f59aa`)
**Fork branch**: `lazy-dinosaur/jcode:deploy/m9-m27-catchup`
**작업 기간**: Round 16 (문제 식별) → Round 20 (1차 fix) → Round 21 (라이브 검증) → Round 22 (claude-code 호환 default + self-throttle demo)

---

## 1. 문제 정의

**원본 사용자 요구사항** (Round 16, 2026-05-12):
> "lifecycle 훅은 잘 되는것 같은데 이거 문제가 훅을 다시 결과로 받아서 진행하는게 중요한거지??"

즉, lifecycle hook 의 본질은:
- **외부 이벤트가 들어왔을 때 사용자 입력 없이 LLM 이 자동으로 다��� 행동을 trigger** 해야 함
- 이를 위해서는 **hook stdout 의 directive 가 LLM context 에 inject** + **LLM turn 자동 시작** 둘 다 필요

**M35 fix 이전 상태**:
- `tool.execute.before` → `HookDecision { action: deny, reason }` → tool 차단 + reason 이 tool_result 로 LLM 반환 ✅ OK
- `response.completed` deny → `inject_lifecycle_reminder_for_continuation` → 다음 user message 로 reminder ✅ M11 stage 6 OK
- 🔴 **`response.completed` 의 hook stdout 이 `inject` 형태로 들어와도 LLM 에 자동 inject 안 됨** ← 이게 M35

---

## 2. M11 stage 6 와의 관계

M11 stage 6 (`f74bffac`, `0e1fcc2b`) 가 이미 만든 인프라:

```rust
pub enum LifecycleHookOutcome {
    Stop,
    ContinueImmediate,                                        // M11 deny path
    ContinueImmediateWithInject(HookInjectContinuation),      // M35 inject path
}
```

M11 의 `Deny(reason)` path:
1. Hook → `{"action":"deny","reason":"..."}` 출력
2. jcode → `inject_lifecycle_reminder_for_continuation` → "A lifecycle hook denied completion..." system-reminder 를 user message 로 add
3. **wake** → 같은 turn loop 계속 진행 → LLM 자동 응답

M35 가 추가한 `Inject(body)` path:
1. Hook → `{"action":"allow","inject":{"body":"...","format":"system_reminder"}}` 출력
2. jcode → `inject_hook_body_for_continuation` → hook 의 body 를 `<system-reminder>` block 으로 wrap → user message 로 add
3. **wake** → 같은 turn loop 계속 진행 → LLM 자동 응답

**핵심 코드** (`src/agent/turn_loops.rs`):

```rust
pub(super) fn inject_hook_body_for_continuation(
    &mut self,
    inject: crate::hooks::HookInjectContinuation,
) {
    let trimmed = inject.body.trim();
    if trimmed.is_empty() {
        return;
    }
    let text = match inject.format {
        InjectionFormat::SystemReminder => {
            format!("<system-reminder>\n{trimmed}\n</system-reminder>")
        }
        InjectionFormat::UserMessage => trimmed.to_string(),
    };
    self.add_message(Role::User, vec![ContentBlock::Text { text, .. }]);
    self.session.save()?;
}
```

---

## 3. 작업 history (Round 별)

### Round 16 (2026-05-12): 문제 식별

- 사용자 라이브 보고 "subagent 결과 안 리턴 같다" → 코드 read 로 subagent path 는 정상 sync return 임을 확인 (`src/tool/task.rs:475`)
- 진짜 missing 은 **`response.completed` 의 hook stdout inject path**
- 6개 설계 옵션 제시: (a) `tool.execute.after` stdout 도 inject, (b) `pre_tool_use` 에 `modify` action, (c) `response.completed` 에도 `inject` action (← 이걸 채택), (d) async hook 결과 회수, ...

### Round 20: 1차 fix 시도

Commits:
- `45993e2f` "Inject lifecycle hook stdout into turns" — `HookDecision.inject` 필드 + `LifecycleHookOutcome::ContinueImmediateWithInject` 추가
- `9b2dfdd5` "fix(m35): wake sessions after hook injection" — wake path 추가

문제: hook 이 stdout 만 출력하면 inject 는 되는데 **LLM 자동 turn 이 안 시작됨** (`ContinueImmediateWithInject` 가 outer turn loop 의 `continue` 까지 도달 안 함).

### Round 21: 2차 fix (진짜 동작)

Commit `4a87de32` "fix(m35): wire hook inject into ContinueImmediate path like M11 deny":
- M11 의 `ContinueImmediate` 패턴을 그대로 재사용 — outer turn loop 의 `match outcome` 안에 `ContinueImmediateWithInject` arm 추가
- `inject_hook_body_for_continuation` 호출 + `continue` 로 outer loop iteration

라이브 검증 (Round 21):
- 3 회 연속 PASS: `HOOK_ACK_1778564759`, `HOOK_ACK_1778564762`, `HOOK_ACK_1778564765`
- 스크린샷 confirm 받음
- 사용자: "잘되는것같다"

### Round 22 (이번): claude-code 호환 + self-throttle demo

#### 사용자 의문: "한 user turn 에 hook 4-5 회 fire — 예전엔 없던 문제로 기억"

분석 결과 (`/tmp/jcode-response-completed.log` rooster session):

| user message | 1st fire (active=false) | 2nd-4th fire (active=true) | 합계 |
|---|---|---|---|
| "dkdk" | inject | 3 회 cap exceeded | 4 |
| "dd" | inject | 3 회 cap exceeded | 4 |
| "안녕" | inject | 3 회 cap exceeded | 4 |

→ cap=3 이 정확히 작동. 사용자가 본 "4-5 회" 는 M35 fix 가 도입한 의도된 cap=3 자동 continuation 동작 (이전엔 hook stdout 이 LLM 에 안 들어가서 자동 turn 0, fire 1 회였음). **버그 아님**.

#### claude-code 표준 비교

Anthropic 공식 Stop hook spec (https://docs.anthropic.com/en/docs/claude-code/hooks):

> "Stop hooks receive `stop_hook_active` and `last_assistant_message`. The `stop_hook_active` field is **true when Claude Code is already continuing as a result of a stop hook**. Check this value or process the transcript to **prevent Claude Code from running indefinitely**."

| 항목 | claude-code | jcode (M35 이후) |
|---|---|---|
| Stop hook 존재 | ✅ | ✅ (`response.completed`) |
| Hook stdout → LLM inject + 자동 continue | ✅ | ✅ |
| `stop_hook_active` flag | ✅ | ✅ (호환) |
| Cap 정책 | ❌ 없음 (script 책임) | **Round 22 부터 cap=0 default** (= claude-code 호환) + hardcap 옵션 유지 |

#### Round 22 변경

**Default cap 3 → 0**:
```rust
// src/agent/turn_loops.rs
pub(super) const DEFAULT_MAX_LIFECYCLE_DENY_STREAK: u8 = 0;
```

**Config docs 갱신** (`src/config/default_file.rs`):
```toml
# Default (M35 Round 22): 0 = no cap (claude-code compatible trust mode).
# Hook scripts must self-throttle using the response.completed payload field
# `stop_hook_active = true` on continuation turns, e.g.:
#   STOP_HOOK_ACTIVE=$(jq -r '.stop_hook_active' <&0)
#   [ "$STOP_HOOK_ACTIVE" = "true" ] && exit 0
# Set to N (e.g. 3) for a hardcap.
# Environment override:
#   JCODE_MAX_LIFECYCLE_DENY_STREAK=3 jcode
# max_lifecycle_deny_streak = 0
```

**Self-throttle demo hook** (`~/.jcode/hooks/m35-self-throttle-demo.sh`):
```bash
#!/usr/bin/env bash
set -euo pipefail
LOG=/tmp/jcode-m35-self-throttle-demo.log
TS=$(date +%s)
PAYLOAD=$(cat)
echo "[$(date -Iseconds)] fire payload=${PAYLOAD}" >> "$LOG"
STOP_HOOK_ACTIVE=$(echo "$PAYLOAD" | jq -r '.stop_hook_active // false')

if [ "$STOP_HOOK_ACTIVE" = "true" ]; then
    echo "[$(date -Iseconds)] skipping (stop_hook_active=true, self-throttle)" >> "$LOG"
    exit 0
fi

echo "[$(date -Iseconds)] injecting HOOK_ACK_${TS}" >> "$LOG"
DIRECTIVE="IMPORTANT: This is a hook-injected directive. Please respond ..."
jq -nc --arg body "$DIRECTIVE" \
   '{action:"allow",inject:{body:$body,format:"system_reminder"}}'
```

#### Round 22 라이브 검증 PASS

elephant session 의 hook log:
```
[15:15:57] fire payload={..."stop_hook_active":false,"last_user_message":"안녕","turn_count":1...}
[15:15:57] injecting HOOK_ACK_1778566557
[15:15:59] fire payload={..."stop_hook_active":true,..."turn_count":1...}
[15:15:59] skipping (stop_hook_active=true, self-throttle)
```

**사용자 화면**: "안녕" → turn 1 응답 → 자동 turn 2 (HOOK_ACK 포함) → **정지, 사용자 입력 대기**. cap 없이도 정확히 1회 continuation.

---

## 4. Hook Wire Format (중요!)

**plain text stdout 으로는 inject 안 됩니다.** 반드시 JSON wire format 으로 stdout 에 출력해야 함:

```json
{
  "action": "allow",
  "inject": {
    "body": "directive text that will become a system-reminder in LLM's next turn",
    "format": "system_reminder"
  }
}
```

또는 deny path:
```json
{
  "action": "deny",
  "reason": "denial reason becomes system-reminder"
}
```

### 코드 위치 (parse path)

`src/hooks.rs:540`:
```rust
fn hook_decision_inject_continuation(decision: &HookDecision) -> Option<HookInjectContinuation> {
    decision.inject.as_ref().and_then(|payload| {
        let body = payload.body.trim();
        if body.is_empty() { return None; }
        Some(HookInjectContinuation {
            body: body.to_string(),
            format: parse_hook_inject_format(payload.format.as_deref()),
        })
    })
}
```

`decision.inject` 가 None (= plain text stdout, 또는 JSON 이지만 `inject` field 없음) 이면 inject path 안 탐.

### Round 22 의 시행착오

1차 demo hook 은 plain text 로 `IMPORTANT: ...` 만 stdout 출력 → inject path 안 탐 → 사용자 화면에 자동 turn 안 보임 ("훅이 안들어와")
2차 fix: `jq -nc '{action:"allow",inject:{body:$body,format:"system_reminder"}}'` 로 JSON wire format 출력 → PASS.

이 내용은 `src/config/default_file.rs` 의 `[hooks]` 섹션 docs 에도 정확히 명시되어 있어야 함 (향후 보강 candidate).

---

## 5. 사용자 향 사용 가이드

### A. Hook script 작성 패턴 (권장: claude-code 호환 self-throttle)

```bash
#!/usr/bin/env bash
set -euo pipefail
PAYLOAD=$(cat)
STOP_HOOK_ACTIVE=$(echo "$PAYLOAD" | jq -r '.stop_hook_active // false')

# self-throttle: continuation turn 에서는 즉시 종료
if [ "$STOP_HOOK_ACTIVE" = "true" ]; then
    exit 0
fi

# 첫 fire 일 때만 inject
DIRECTIVE="여기에 LLM 에 전달할 directive"
jq -nc --arg body "$DIRECTIVE" \
   '{action:"allow",inject:{body:$body,format:"system_reminder"}}'
```

### B. Config 등록 (`~/.jcode/config.toml`)

```toml
[hooks]
enabled = true

[[hooks.commands]]
event = "response.completed"
command = "/absolute/path/to/your-hook.sh"
blocking = true
timeout_ms = 3000
```

**주의**: `[[hooks.commands]]` block 들은 모두 `[hooks]` 섹션 안 (다른 top-level table 들 사이에 끼지 않게) 두어야 함. 예를 들어 `[tool.bash]` 뒤에 두면 TOML 상 `tool.bash.hooks.commands` 로 잘못 파싱됨.

### C. Hardcap 원하면

env 또는 config 로 override:
```bash
JCODE_MAX_LIFECYCLE_DENY_STREAK=3 jcode    # env override
```
```toml
# ~/.jcode/config.toml 어느 곳에든 (단, 적절한 table 안)
max_lifecycle_deny_streak = 3
```

---

## 6. 검증 자료

### Unit tests (`src/agent_tests.rs`)

- `lifecycle_deny_cap_three_allows_three_immediate_continuations_then_stops` — cap=3 정확 동작
- `lifecycle_deny_cap_zero_allows_unlimited_immediate_continuations` — cap=0 무제한
- `lifecycle_deny_streak_resets_at_new_user_turn_start` — user input 마다 reset
- `lifecycle_deny_streak_env_override_beats_config` — env > config priority
- `lifecycle_deny_streak_config_and_default_resolution` — default = `DEFAULT_MAX_LIFECYCLE_DENY_STREAK`

모두 PASS (6/6).

### Round 21 라이브 검증

3 회 연속 HOOK_ACK PASS (스크린샷 confirm): `HOOK_ACK_1778564759`, `HOOK_ACK_1778564762`, `HOOK_ACK_1778564765`.

### Round 22 라이브 검증

elephant session: user "안녕" 1회 → 자동 turn 1회 inject (`HOOK_ACK_1778566557`) → 2nd fire skip → 정지. cap 없이 self-throttle 정확 동작.

### Hook log (영구 evidence)

- `/tmp/jcode-response-completed.log` — 모든 response.completed fire 의 full payload (tee hook)
- `/tmp/jcode-m35-self-throttle-demo.log` — self-throttle demo hook 의 fire/skip/inject 기록
- `/tmp/jcode-session-stop.log` — session.stop event log

---

## 7. 관련 코드 파일

| 파일 | 역할 |
|---|---|
| `src/agent/turn_loops.rs` | `LifecycleHookOutcome`, `fire_response_completed_hook`, `handle_lifecycle_hook_inject`, `handle_lifecycle_hook_deny`, `inject_hook_body_for_continuation`, `inject_lifecycle_reminder_for_continuation`, `DEFAULT_MAX_LIFECYCLE_DENY_STREAK` |
| `src/agent/turn_streaming_mpsc.rs` | streaming path 의 동일 fire/inject/continue 로직 |
| `src/agent/turn_execution.rs` | `reset_lifecycle_deny_streak_for_user_turn` — 모든 `run_once_*` 진입점에서 호출 |
| `src/hooks.rs` | `HookDecision`, `HookInjectPayload`, `LifecycleHookDecision`, `hook_decision_inject_continuation`, `run_response_hooks`, `ResponseCompletedHookPayload (stop_hook_active 포함)` |
| `src/config/default_file.rs` | `[hooks]` 섹션 docs, `max_lifecycle_deny_streak` 설명 |
| `src/agent_tests.rs` | M11/M35 unit tests (cap=0/3, env override, reset, default) |
| `crates/jcode-config-types/src/lib.rs` | `max_lifecycle_deny_streak: Option<u8>` config field |

---

## 8. 향후 개선 candidate (M35 와 별개 milestone)

| Idea | 근거 |
|---|---|
| `tool.execute.after` stdout → next turn inject | Round 16 옵션 (a). 현재 `run_tool_hooks` 가 stdout 회수 안 함. tool 결과에 추가 컨텍스트 부착에 유용. |
| `pre_tool_use` 에 `modify` action | Round 16 옵션 (b). tool input 을 hook 으로 수정 가능하게. |
| Async (non-blocking) hook stdout 회수 | Round 16 옵션 (d). 다음 turn 시작 전 collect. |
| Hook script docs 에 JSON wire format 강조 | Round 22 시행착오 방지 |
| `response.completed` 가 한 user turn 에 N 회 fire 하는 것 이 정상임을 LAZYDINO_STATUS 에 명시 | 사용자 의문 재발 방지 |

---

## 9. 표준 vs jcode 정책 결정 기록

**Round 22 의 결정**: claude-code 와 동일한 방향으로 가기로 함.
- Default `max_lifecycle_deny_streak = 0` (= claude-code trust mode, 무제한)
- Hook script 가 `stop_hook_active` 를 보고 self-throttle 책임
- Hardcap 이 필요한 사용자는 `max_lifecycle_deny_streak = 3` 같이 명시 설정

**사용자 발화 (Round 22)**:
> "claude-code 와 같은 방향으로 가자 cap 없애고"
> "제대로된 훅이 돈다면 1번만 돌꺼라는거지 w금??"
> "됬다 !!"

---

## 10. 한 줄 요약

> **M35 = response.completed hook 이 stdout JSON `{action,inject:{body,format}}` 으로 응답하면 jcode 가 그 body 를 LLM 다음 turn 의 system-reminder 로 inject 하고 사용자 입력 없이 자동 turn 을 시작한다. Default 는 claude-code 호환 무제한 cap (=0) 이고, hook script 가 `stop_hook_active` 로 self-throttle 한다.**
