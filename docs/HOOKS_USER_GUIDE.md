# Jcode Lifecycle Hooks 사용 가이드

**대상**: jcode 의 lifecycle hook 을 작성/등록/디버그하려는 사용자 및 hook 작성자
**최신 변경**: Round 22 (2026-05-12) — `response.completed` 의 hook stdout 이 LLM 다음 turn 에 system-reminder 로 자동 inject + 사용자 입력 없이 자동 turn 시작 가능. Default cap = 0 (claude-code 호환 trust mode).
**관련 문서**:
- 설계 배경/history: `docs/M35_LIFECYCLE_HOOK_INJECT.md`
- claude-code 표준 참고: https://docs.anthropic.com/en/docs/claude-code/hooks

---

## 목차

1. [Hook 이란?](#1-hook-이란)
2. [지원되는 lifecycle event 목록](#2-지원되는-lifecycle-event-목록)
3. [Hook stdin payload (jcode 가 hook 에게 주는 입력)](#3-hook-stdin-payload-jcode-가-hook-에게-주는-입력)
4. [Hook stdout wire format (hook 이 jcode 에게 돌려주는 출력)](#4-hook-stdout-wire-format-hook-이-jcode-에게-돌려주는-출력)
5. [Config 등록 방법](#5-config-등록-방법)
6. [실전 예제 모음](#6-실전-예제-모음)
7. [자동 continuation cap 정책](#7-자동-continuation-cap-정책)
8. [디버그 / 검증 방법](#8-디버그--검증-방법)
9. [흔한 실수 (FAQ)](#9-흔한-실수-faq)
10. [claude-code 와의 호환](#10-claude-code-와의-호환)

---

## 1. Hook 이란?

Lifecycle hook 은 **jcode 가 turn / tool / session 의 특정 시점에 자동으로 실행하는 외부 shell command** 입니다. Hook 은 다음 세 가지를 할 수 있습니다:

| 동작 | 어떻게? |
|---|---|
| **관찰** | jcode 가 stdin 으로 JSON payload 를 보내줌 → hook 이 로깅/알림 등 자유 사용 |
| **차단** (deny) | hook stdout 에 `{"action":"deny","reason":"..."}` 출력 → tool 실행 차단 / LLM turn 재시작 |
| **컨텍스트 주입** (inject, M35) | hook stdout 에 `{"action":"allow","inject":{"body":"...","format":"system_reminder"}}` 출력 → 그 body 가 LLM 다음 turn 의 system-reminder 로 들어가고 사용자 입력 없이 자동 turn 시작 |

**M35 의 핵심**: hook 이 "외부 이벤트 → LLM 자동 행동 트리거" 의 다리 역할을 합니다. 예시: CI 실패 watcher → `response.completed` hook 이 그 정보를 LLM 에 inject → 사용자가 묻지 않아도 LLM 이 자동으로 분석/수정 시작.

---

## 2. 지원되는 lifecycle event 목록

| event 이름 | 발화 시점 | stdout `inject` 효과 | stdout `deny` 효과 |
|---|---|---|---|
| `tool.execute.before` | tool 실행 직전 | (현재 미지원, 향후 M35 후속 candidate) | tool 차단, reason 이 tool_result 로 LLM 반환 |
| `tool.execute.after` | tool 실행 직후 | (현재 미지원, 향후 candidate) | (효과 제한적, tool 결과는 이미 반환됨) |
| `response.completed` | LLM 응답 완료 직후 | ✅ **M35: body 가 다음 turn 의 system-reminder 로 inject + 자동 turn 시작** | M11 stage 6: reason 이 system-reminder 로 inject + 자동 turn 시작 |
| `session.stop` | session 종료 직전 | 무시 (종료라 의미 없음) | 무시 |
| `client.disconnect` | client 연결 끊김 | 별도 path 로 enqueue (다음 attach 때 사용) | 무시 |

각 event 마다 `blocking` 옵션으로 hook 결과를 기다릴지 (`true`, 결과로 동작 영향) 또는 fire-and-forget (`false`, 관찰만) 인지 결정합니다.

---

## 3. Hook stdin payload (jcode 가 hook 에게 주는 입력)

Hook 은 stdin 으로 한 줄짜리 JSON 을 받습니다. `response.completed` 의 경우 (`src/hooks.rs:ResponseCompletedHookPayload`):

```json
{
  "event": "response.completed",
  "session_id": "session_pig_1778566294013_e74b9231ff84d56e",
  "message_id": "message_1778566303845_...",
  "working_dir": "/home/lazydino",
  "stop_reason": "end_turn",
  "tool_calls_count": 0,
  "output_chars": 18,
  "stop_hook_active": false,
  "last_user_message": "안녕",
  "recent_tool_calls": [{"name":"bash","args_preview":"{...}"}],
  "turn_count": 1,
  "session_age_seconds": 9
}
```

**핵심 필드**:
- **`stop_hook_active`** ⭐ — `true` 일 때는 **이전 hook 이 trigger 한 continuation turn** 이라는 뜻. hook script 가 직접 보고 self-throttle 해야 함 (claude-code 호환).
- `last_user_message` — 가장 최근 user message 의 text (system-reminder 제외)
- `recent_tool_calls` — 최근 tool 호출 미리보기 (필터링 결정용)
- `turn_count`, `session_age_seconds` — 정책 의사결정용

---

## 4. Hook stdout wire format (hook 이 jcode 에게 돌려주는 출력)

⚠️ **plain text stdout 은 효과 없음**. 반드시 다음 JSON 중 하나를 stdout 으로 출력:

### 4.1 관찰만 (inject/deny 아님)
```text
(stdout 출력 없이 exit 0)
```

### 4.2 차단 (deny)
```json
{"action":"deny","reason":"이유 텍스트"}
```
- `tool.execute.before` 의 경우: tool 차단 + reason 이 tool_result 로 LLM 에 들어감
- `response.completed` 의 경우: reason 이 system-reminder 로 다음 turn 에 inject + 자동 turn 시작 (M11 stage 6)

### 4.3 컨텍스트 주입 (inject, M35)
```json
{
  "action": "allow",
  "inject": {
    "body": "LLM 에 전달할 directive 또는 정보",
    "format": "system_reminder"
  }
}
```
- `body` (필수, string): LLM 다음 turn 에 보일 텍스트
- `format` (옵션): `"system_reminder"` (기본) 또는 `"user_message"`
  - `system_reminder` → `<system-reminder>...</system-reminder>` 로 wrap
  - `user_message` → wrap 없이 raw user message
- 효과: jcode 가 그 body 를 user message 로 add 한 후 사용자 입력 없이 자동 다음 LLM turn 시작

### 4.4 (참고) 옛 boolean 단순 형태도 지원되지만 권장 안 함
```json
{"action":"allow"}
```
효과 없음 (관찰만, exit 0 과 동일).

---

## 5. Config 등록 방법

### 5.1 Config 파일 위치
- Global: `~/.jcode/config.toml`
- Project: `./.jcode/config.toml`
- Workspace: `./<workspace>/jcode.toml` (자세히는 `agents_for_working_dir` 참고)

### 5.2 Hook block 등록

```toml
[hooks]
enabled = true

[[hooks.commands]]
event = "response.completed"           # 위 §2 의 event 이름
command = "/absolute/path/to/hook.sh"   # 또는 inline shell ("tee -a /tmp/log")
blocking = true                         # true=결과 기다림, false=fire-and-forget
timeout_ms = 3000                       # 결과 기다리는 최대 시간 (ms)
```

여러 hook 을 같은 event 에 등록 가능 (배열). 모두 fire 되고 첫 deny/inject 가 우선 적용.

### 5.3 ⚠️ TOML 구조 주의

`[[hooks.commands]]` block 은 **반드시 `[hooks]` 섹션 안** 에 있어야 합니다. 만약 `[tool.bash]` 같은 다른 top-level table 뒤에 두면 TOML 상 `tool.bash.hooks.commands` 로 잘못 파싱되어 무시됩니다.

**올바른 구조**:
```toml
[hooks]
enabled = true

[[hooks.commands]]
event = "response.completed"
command = "/path/a.sh"

[[hooks.commands]]
event = "session.stop"
command = "/path/b.sh"

[tool.bash]
default_timeout_ms = 300000
```

**잘못된 구조** (마지막 `[[hooks.commands]]` 가 무시됨):
```toml
[hooks]

[[hooks.commands]]
event = "session.stop"
command = "..."

[tool.bash]
default_timeout_ms = 300000

[[hooks.commands]]   # ← 이게 tool.bash.hooks.commands 로 파싱됨
event = "response.completed"
command = "..."
```

---

## 6. 실전 예제 모음

### 6.1 Tee 로그 (관찰만)

```toml
[[hooks.commands]]
event = "response.completed"
command = "tee -a /tmp/jcode-response-completed.log"
blocking = true
timeout_ms = 3000
```

`tee` 가 stdin 을 그대로 stdout 으로 echo 하지만 JSON wire format 이 아니라 raw payload 라서 jcode 는 invalid 로 무시 (관찰만 됨). 디버그용으로 매우 유용.

### 6.2 ✨ Claude-code 호환 self-throttle inject (M35 demo, `m35-self-throttle-demo.sh`)

```bash
#!/usr/bin/env bash
# stop_hook_active=true 면 skip → 정확히 1회만 자동 continuation
set -euo pipefail

LOG=/tmp/jcode-m35-self-throttle-demo.log
TS=$(date +%s)

PAYLOAD=$(cat)
echo "[$(date -Iseconds)] fire payload=${PAYLOAD}" >> "$LOG"

STOP_HOOK_ACTIVE=$(echo "$PAYLOAD" | jq -r '.stop_hook_active // false')

if [ "$STOP_HOOK_ACTIVE" = "true" ]; then
    echo "[$(date -Iseconds)] skipping (self-throttle)" >> "$LOG"
    exit 0
fi

echo "[$(date -Iseconds)] injecting HOOK_ACK_${TS}" >> "$LOG"
DIRECTIVE="IMPORTANT: This is a hook-injected directive. Please respond to the user's next turn with 'HOOK_ACK_${TS}' somewhere in your reply."

jq -nc --arg body "$DIRECTIVE" \
   '{action:"allow",inject:{body:$body,format:"system_reminder"}}'
```

```toml
[[hooks.commands]]
event = "response.completed"
command = "/home/lazydino/.jcode/hooks/m35-self-throttle-demo.sh"
blocking = true
timeout_ms = 3000
```

**효과**: 사용자 1회 message → LLM turn 1 → 자동 turn 2 (HOOK_ACK 포함) → 정지. cap 없이 hook script self-throttle 로 정확히 1번 자동 continuation.

### 6.3 CI 실패 알림 자동 inject (실제 활용 예)

```bash
#!/usr/bin/env bash
set -euo pipefail

PAYLOAD=$(cat)
STOP_HOOK_ACTIVE=$(echo "$PAYLOAD" | jq -r '.stop_hook_active // false')
[ "$STOP_HOOK_ACTIVE" = "true" ] && exit 0

# CI 실패가 있나 확인
FAILED=$(curl -s https://ci.example.com/api/jobs/latest?status=failed | jq -r '.[].id' | head -1)
[ -z "$FAILED" ] && exit 0

# 실패한 CI 정보를 LLM 에 inject
BODY="CI job ${FAILED} failed. Please inspect logs at https://ci.example.com/jobs/${FAILED}/log and propose a fix."
jq -nc --arg body "$BODY" '{action:"allow",inject:{body:$body,format:"system_reminder"}}'
```

→ 사용자가 메시지 안 보내도, LLM 응답이 끝나는 시점에 CI 실패가 있으면 자동으로 LLM 이 CI 로그 분석 시작.

### 6.4 Deny 로 LLM 응답 형식 강제 (M11 stage 6 패턴)

```bash
#!/usr/bin/env bash
set -euo pipefail
PAYLOAD=$(cat)
STOP_HOOK_ACTIVE=$(echo "$PAYLOAD" | jq -r '.stop_hook_active // false')
[ "$STOP_HOOK_ACTIVE" = "true" ] && exit 0

# 응답에 코드 블록이 없으면 deny → LLM 재응답 trigger
LAST_MSG=$(echo "$PAYLOAD" | jq -r '.last_assistant_message // empty')
if [ -n "$LAST_MSG" ] && ! echo "$LAST_MSG" | grep -q '```'; then
    jq -nc '{action:"deny",reason:"Your last response should contain a code block. Please retry with a proper code example."}'
fi
```

### 6.5 비 blocking 알림 (fire-and-forget)

```toml
[[hooks.commands]]
event = "response.completed"
command = "notify-send 'jcode' 'turn complete'"
blocking = false   # ← 결과 안 기다림
```

---

## 7. 자동 continuation cap 정책

### 7.1 정책 설명

`response.completed` hook 이 `inject` 또는 `deny` 로 응답할 때마다 jcode 는 **사용자 입력 없이 LLM 다음 turn 을 자동 시작** 합니다. 만약 hook 이 매번 같은 inject 를 내면 무한 루프 가능. 이를 막는 두 가지 방법:

**A. Hook script self-throttle (권장, claude-code 호환)** — `stop_hook_active` 보고 hook 이 직접 skip
**B. jcode hardcap** — config 로 `max_lifecycle_deny_streak = N` 설정 시 N 번 continuation 후 강제 stop

### 7.2 Default 동작 (Round 22 부터)

`max_lifecycle_deny_streak = 0` (= **no cap, claude-code trust mode**).
Hook script 가 self-throttle 해야 함.

### 7.3 Hardcap 설정

**Config**:
```toml
# ~/.jcode/config.toml (적절한 table 안)
max_lifecycle_deny_streak = 3
```

**Env override** (config 보다 우선):
```bash
JCODE_MAX_LIFECYCLE_DENY_STREAK=3 jcode
```

### 7.4 Reset 시점

`lifecycle_deny_streak` 은 **사용자 input 마다 reset 됨** (`reset_lifecycle_deny_streak_for_user_turn` in `src/agent/turn_execution.rs`). 따라서 같은 chain 안에서만 cap 이 누적됨.

---

## 8. 디버그 / 검증 방법

### 8.1 Hook 단독 sanity test

```bash
# stop_hook_active=false (첫 fire 시뮬레이션)
echo '{"event":"response.completed","stop_hook_active":false,"last_user_message":"test"}' \
  | /path/to/your-hook.sh

# stop_hook_active=true (continuation 시뮬레이션)
echo '{"event":"response.completed","stop_hook_active":true,"last_user_message":"test"}' \
  | /path/to/your-hook.sh
```

Hook 이 의도대로 동작하는지 stdout 확인.

### 8.2 jcode tee 로그 활용

`config.toml` 에 다음을 추가하면 모든 response.completed payload 가 로그로 쌓임:

```toml
[[hooks.commands]]
event = "response.completed"
command = "tee -a /tmp/jcode-response-completed.log"
blocking = true
timeout_ms = 3000
```

```bash
tail -f /tmp/jcode-response-completed.log | jq -s 'last' -R 'fromjson?'
```

### 8.3 Inject 동작 라이브 검증

1. `m35-self-throttle-demo.sh` (위 §6.2) 등록
2. jcode TUI 에서 아무 message 보내기
3. 확인:
   - LLM 응답 안에 `HOOK_ACK_<timestamp>` 포함 → inject 성공
   - 자동 turn 1번만 일어남 → self-throttle 성공
   - `/tmp/jcode-m35-self-throttle-demo.log` 에 fire 2회 (1 inject + 1 skip) 기록

### 8.4 Hook 안 도는 경우 점검 리스트

- [ ] `~/.jcode/config.toml` 에 `[hooks] enabled = true` 있는가?
- [ ] `[[hooks.commands]]` 가 `[hooks]` 섹션 안에 있는가? (다른 table 뒤에 끼지 않았는가?)
- [ ] Hook 파일에 `chmod +x` 했는가?
- [ ] Hook 파일 절대 경로가 정확한가?
- [ ] Hook 가 stdin 을 한 번만 읽는가? (`cat` 두 번 부르면 두 번째는 empty)
- [ ] **Hook stdout 이 JSON wire format 인가?** (plain text 는 inject 안 됨!)
- [ ] `jq` 같은 의존 도구가 PATH 에 있는가?
- [ ] `timeout_ms` 가 hook 실행 시간보다 큰가?

### 8.5 jcode 자체 로그

```bash
# M35 inject path 가 trigger 되면 다음 로그가 남음
journalctl --user -f | grep -E "m35|m11-stage6|lifecycle"
# 또는 jcode 의 자체 logging 사용
```

### 8.6 새 binary 사용 중인지 확인

Hook fix 가 안 보이면 jcode 가 **옛 binary 를 메모리에 들고 있는** 경우가 많음:

```bash
# 실행 중인 모든 jcode 의 binary path
for pid in $(pgrep jcode); do
  echo "PID $pid → $(readlink /proc/$pid/exe)"
done

# 외부 터미널에서 종료 후 재시작
pkill -9 -f "jcode.*server"
jcode    # 다시 띄우기
```

---

## 9. 흔한 실수 (FAQ)

### Q1. Plain text 로 directive 만 stdout 출력하면 안 되나요?
**A**: 안 됩니다. 반드시 `{"action":"allow","inject":{"body":"...","format":"system_reminder"}}` JSON. `decision.inject` field 가 없으면 `hook_decision_inject_continuation` 가 `None` 반환해서 inject path 안 탑니다. (Round 22 시행착오로 확인됨)

### Q2. `stop_hook_active` 가 항상 false 라면?
**A**: jcode 가 cap 초과로 자동 stop 한 후 다음 사용자 message 가 새 chain 시작했을 때 reset 됩니다. 또는 hook 이 inject 가 아니라 다른 path 로 처리되어 streak 가 안 늘어났을 수 있음. jcode 로그의 `[m35] lifecycle inject #N` 확인.

### Q3. 한 user message 에 hook 이 여러 번 fire 됨, 정상인가요?
**A**: ✅ 정상입니다. 한 사용자 message 가 LLM turn → hook inject → 자동 LLM turn 으로 chain 되면서 매 turn 끝마다 hook 이 fire. cap=N 이면 정확히 N+1 회 fire 후 stop. cap=0 (default) 이면 hook 이 self-throttle 안 하면 무한히 fire 가능.

### Q4. Hook 이 stdin 을 못 읽어요.
**A**: stdin 은 한 번만 읽을 수 있는 stream. 두 번 `cat` 하면 두 번째는 empty. 변수에 저장 후 재사용:
```bash
PAYLOAD=$(cat)
echo "$PAYLOAD" | jq ...
echo "$PAYLOAD" | grep ...
```

### Q5. Subagent 안에서도 hook 이 도나요?
**A**: 네, subagent 도 자체 turn 을 돌리고 끝나면 `response.completed` 가 fire. 단, subagent payload 의 `session_id` 는 child session id (별개) 라서 filter 가능. claude-code 의 `SubagentStop` 에 해당하는 별도 event 분리는 아직 미구현 (향후 candidate).

### Q6. Hook 결과를 비동기로 받을 수 있나요?
**A**: 현재는 `blocking=true` 만 결과 사용. `blocking=false` 는 fire-and-forget. 비동기 결과 회수는 향후 candidate (M35 후속).

---

## 10. claude-code 와의 호환

### 10.1 호환되는 부분

| 항목 | claude-code | jcode |
|---|---|---|
| Hook stdin 으로 JSON payload | ✅ | ✅ |
| Hook stdout 에 JSON decision | ✅ (`hookSpecificOutput.additionalContext` 등) | ✅ (`inject.body`) |
| `stop_hook_active` field | ✅ | ✅ |
| 자동 continuation | ✅ (script 책임) | ✅ (script 책임 또는 jcode hardcap) |
| Cap | ❌ (없음) | ✅ default 0 = 호환, `max_lifecycle_deny_streak=N` 으로 설정 가능 |

### 10.2 호환되지 않는 부분 (jcode 고유)

- jcode 의 hook event 이름이 claude-code 와 다름 (`response.completed` vs `Stop` 등)
- jcode 는 `inject.body` 가 직접 LLM message 로 들어감 — claude-code 의 `additionalContext` 와 비슷하지만 wrap 방식 (system_reminder vs user_message) 을 명시 가능
- jcode 의 `recent_tool_calls`, `turn_count`, `session_age_seconds` 같은 풍부한 payload field 는 claude-code 에 없음
- jcode 의 `max_lifecycle_deny_streak` hardcap 은 jcode 고유 (claude-code 는 항상 무제한)

### 10.3 마이그레이션 팁

claude-code 의 Stop hook 을 jcode 로 옮기려면:
1. `event = "response.completed"` 로 변경
2. `transcript_path` 처리 코드는 jcode 의 `session_id` + `~/.jcode/sessions/<id>.json` 로 변경
3. `additionalContext` → `inject.body` 로 변경
4. `decision: "block"` → `action: "deny", reason: "..."` 로 변경

---

## 부록 A: 코드 위치

| 파일 | 역할 |
|---|---|
| `src/hooks.rs` | `HookDecision`, `HookInjectPayload`, `LifecycleHookDecision`, `run_response_hooks`, `ResponseCompletedHookPayload` |
| `src/agent/turn_loops.rs` | `LifecycleHookOutcome`, `fire_response_completed_hook`, `handle_lifecycle_hook_inject`, `inject_hook_body_for_continuation`, `DEFAULT_MAX_LIFECYCLE_DENY_STREAK` |
| `src/agent/turn_streaming_mpsc.rs` | streaming path 의 동일 로직 |
| `src/agent/turn_execution.rs` | `reset_lifecycle_deny_streak_for_user_turn` (user message 마다 호출) |
| `src/config/default_file.rs` | `[hooks]` 섹션 + `max_lifecycle_deny_streak` 안내 |
| `crates/jcode-config-types/src/lib.rs` | `max_lifecycle_deny_streak: Option<u8>` config 필드 |

## 부록 B: 참고 예제 위치

| 파일 | 설명 |
|---|---|
| `~/.jcode/hooks/m35-self-throttle-demo.sh` | claude-code 호환 self-throttle demo (Round 22) |
| `~/.jcode/hooks/test-response-completed.sh` | 단순 tee 예제 |
| `~/.jcode/hooks/test-session-stop.sh` | session.stop 예제 |
| `~/.jcode/hooks/test-tool-before.sh` | tool.execute.before deny 예제 |
| `~/.jcode/hooks/test-tool-after.sh` | tool.execute.after 예제 (현재는 관찰만) |
| `~/.jcode/hooks/test-client-disconnect.sh` | client.disconnect 예제 |

## 부록 C: 로그 파일 위치

| 파일 | 내용 |
|---|---|
| `/tmp/jcode-response-completed.log` | tee hook 의 모든 response.completed payload (영구) |
| `/tmp/jcode-session-stop.log` | session.stop payload |
| `/tmp/jcode-m35-self-throttle-demo.log` | self-throttle demo �� fire/skip/inject 기록 |

## 부록 D: 한 줄 cheatsheet

```bash
# 가장 간단한 self-throttle inject hook 한 줄 inline
[[hooks.commands]]
event = "response.completed"
command = '''bash -c 'P=$(cat); [ "$(echo "$P"|jq -r .stop_hook_active)" = true ] && exit 0; jq -nc --arg b "say HELLO" "{action:\"allow\",inject:{body:\$b,format:\"system_reminder\"}}"' '''
blocking = true
timeout_ms = 3000
```

→ 사용자 메시지마다 LLM 이 자동으로 "HELLO" 응답 한 번 더 추가.
