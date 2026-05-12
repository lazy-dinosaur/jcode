# M31 — Background Tool Auto-Inject (bg completion → LLM auto-turn)

## TL;DR

Background tool (`bash run_in_background=true` 또는 `bg` tool) 이 완료되면, 그 결과 (stdout/exit_code/duration) 가 **자동으로 다음 LLM turn 의 system_reminder injection 으로 들어가고, turn 도 자동으로 trigger** 됩니다. 사용자가 "어떻게 됐어?" 물어볼 필요 없음. 사용자 round 14 요구사항: "릴리즈하고 bg 에서 돌리는 툴 결과를 너가 바로 받아서 결과로 사용해야 한다" 충족.

**Status**: 🟢 DONE Round 23 (라이브 PASS, 사용자 confirm).

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│ bg task 완료 (src/background.rs)                                          │
│   - 206: launch_with_pid_attached 완료 분기                              │
│   - 477: status writer drain 분기                                        │
│       ▼                                                                  │
│   send_bg_completion(BackgroundCompletion { task_id, exit_code, stdout, │
│                       stderr, duration_ms, session_id, notify,          │
│                       auto_inject, auto_inject_format,                  │
│                       auto_inject_max_bytes })                          │
└─────────────────────────────────────────────────────────────────────────┘
                          │  notify=true 만 send
                          ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ BG_COMPLETION_SENDERS (src/turn/bg_completion.rs)                       │
│   HashMap<session_id, mpsc::UnboundedSender<BackgroundCompletion>>      │
│   - register_bg_completion_receiver(session_id)  →  mpsc::Receiver      │
│   - send_bg_completion(completion)               →  tx.send(...)        │
│   - unregister_bg_completion_receiver(session_id)                       │
└─────────────────────────────────────────────────────────────────────────┘
                          │  receiver 가 살아있어야 forward
                          ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ client_lifecycle.rs::handle_client (TUI socket connection loop)         │
│   1063  let mut bg_completion_rx =                                       │
│             register_bg_completion_receiver(&client_session_id)?;       │
│   1216  bg_completion = bg_completion_rx.recv() => {                    │
│   1222    enqueue_bg_completion_injection(&client_session_id, &c)       │
│             → InjectedContext { source: BackgroundTask{...},            │
│                                  format: SystemReminder,                │
│                                  dedupe_key: "bg:<task_id>" }           │
│             → enqueue_injection(session_id, ctx)                        │
│   ~1240   if client_is_processing { pending_bg_completions.push_back }  │
│   ~1245   else { start_processing_message(empty content, ...) }         │
│             → 다음 LLM turn 시작 시 injected_context drain 되어 user    │
│               message 앞에 system_reminder 로 prepend                    │
│   2740  unregister_bg_completion_receiver (loop 종료 시 cleanup)         │
└─────────────────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ LLM 다음 turn 의 input 에 다음 형태로 prepend:                            │
│   <system-reminder>                                                      │
│   Background task task_id=... completed (exit=0, duration=3.0s).        │
│   stdout: BG_TEST_OK_...                                                 │
│   stderr: (empty)                                                        │
│   </system-reminder>                                                     │
└─────────────────────────────────────────────────────────────────────────┘
```

## tool field reference

`bg` tool 의 input field (`src/tool/bg.rs:77-89`):

| field | type | default | 설명 |
|---|---|---|---|
| `notify` | `bool` | `true` (auto_inject 시 자동 true) | 완료 시 alarm/completion event 발사 |
| `wake` | `bool` | `true` | quiescent 세션을 깨움 (M30) |
| `auto_inject` | `bool` | **`true`** | 완료 시 stdout/stderr/exit_code 를 다음 turn 에 inject |
| `auto_inject_format` | `"system_reminder"` 또는 `"user_message"` | `"system_reminder"` | injection wire format |
| `auto_inject_max_bytes` | `usize` | `InjectedContext::MAX_BODY_BYTES` (~6KB) | stdout 잘림 한도. stderr 는 `min(this, 2048)` |

`bash` tool 의 `run_in_background=true` 도 동일하게 위 field 들을 받습니다 (`src/tool/bash.rs`).

## 검증 절차

### Unit tests

```
cargo test --lib bg_completion
# 9 tests, 9 passed:
#   bg_completion_with_auto_inject_false_does_not_enqueue
#   bg_completion_with_auto_inject_true_enqueues_context
#   bg_completion_with_notify_true_sends_to_channel
#   bg_completion_with_notify_false_does_not_send
#   session_loop_wakes_on_bg_completion_recv
#   multiple_completions_queue_in_channel_order
#   stderr_uses_smaller_cap_than_stdout
#   inject_truncates_stdout_at_max_bytes
#   inject_dedupe_key_is_bg_task_id
```

### 라이브 검증 (TUI)

1. 새 tmux + jcode TUI 세션 시작 (`tmux new-session ... jcode`)
2. 프롬프트로 LLM 에게 시키기:
   ```
   Step 1) bash run_in_background=true notify=true wake=true 로 `sleep 3 && echo BG_MARKER_xxx`
   Step 2) "STAGE1_DONE" 만 출력하고 stop. polling 금지.
   Step 3) BG_MARKER_xxx 가 들어오면 M31_PASS 출력.
   ```
3. ~5초 후 자동 turn 발생 확인:
   - jcode log: `[bg-completion] received bg completion: <task_id>` + `[bg/inject] enqueued completion task_id=<task_id> session_id=<sess>`
   - TUI: "M31_PASS:..." 자동 추가

**Round 23 실제 PASS 증거**:
```
[2026-05-12 15:40:00.273] [INFO] received bg completion: 997266nodw
[2026-05-12 15:40:00.274] [INFO] [bg/inject] enqueued completion
                                  task_id=997266nodw
                                  session_id=session_snake_1778567986327_91ef12fe6ad4c3fa
```
사��자 confirm: "내가 새로 열어서 해보니까 잘된다"

## Edge cases / 의도된 한계

### A) Headless / `jcode run` single-shot 모드

`jcode run "..."` 또는 `jcode debug create_session` + `message_async` 같은 short-lived client 는 첫 응답 후 바로 disconnect → `client_lifecycle::handle_client` loop 종료 → `unregister_bg_completion_receiver` 호출.

이후 bg task 완료가 도착해도 log 에 `[bg-completion] no receiver for task_id=...` 만 남고 drop.

**해결**: TUI 또는 persistent client 가 살아있어야 함. 이는 M31 scope 밖이며, headless inject 가 필요하면 별도 milestone 으로 다룬다 (e.g. `jcode run --keep-alive`).

### B) Multi-attach (sibling client)

같은 session 에 client A 와 B 가 attach 한 상황에서, 어떤 client 가 receiver 인가? — `BG_COMPLETION_SENDERS` 는 **per session_id 단일 sender** (`HashMap<String, UnboundedSender>`) 라서 가장 최근 `register_bg_completion_receiver` 호출 시 이전 sender 가 overwrite 됨. Sibling fanout 은 M32 (assistant streaming broadcast) 와 묶어서 다룬다.

### C) `client_is_processing` 중

LLM 이 이미 turn 을 진행 중에 bg 가 완료되면, `pending_bg_completions.push_back` 으로 queue 에 쌓이고 다음 quiescent 시점에 `start_processing_message` 로 새 turn trigger. 즉 **race-free**.

### D) `auto_inject=false`

LLM 이 명시적으로 `auto_inject: false` 로 bg 를 띄우면 `enqueue_bg_completion_injection` 가 즉시 `Ok(false)` 반환 → injection 안 됨. 단, `notify=true` 면 channel send 는 됨 (UI 알림용).

### E) Truncation

`auto_inject_max_bytes` 가 작거나 default (~6KB) 일 때 큰 stdout 은 잘림. 잘림 위치는 char boundary 보장 (`truncate_for_inject`). stderr 는 추가로 2KB 상한 (`max_bytes.min(2048)`).

### F) Dedup

`InjectedContext.dedupe_key = "bg:<task_id>"` → 같은 task 의 completion 이 두 번 enqueue 돼도 한 번만 LLM 에 보임. (bg send_bg_completion 자체가 한 번만 발사되므로 일반적으로 발생 안 함, 안전망)

## Related code

| 파일 | 역할 |
|---|---|
| `src/turn/bg_completion.rs` | `BackgroundCompletion` struct + mpsc channel + `enqueue_bg_completion_injection` |
| `src/turn/bg_completion_tests.rs` | 9 unit tests |
| `src/background.rs:206, 477` | bg task 완료 시 `send_bg_completion` 발사 |
| `src/server/client_lifecycle.rs:1063, 1216, 1222, 2740` | receiver register/drain/cleanup |
| `src/tool/bg.rs:28, 77-89, 246-248` | `bg` tool 의 `auto_inject*` field schema |
| `src/tool/bash.rs:560` | `bash` tool 의 `notify` field 와 bg launch path |
| `src/turn/injected_context.rs` | `InjectedContext::BackgroundTask` variant, dedup, format wire |

## 관련 milestones

- **M30** (bg wake/notify path) — wake event 자체. M31 는 M30 위에 inject + auto-turn 을 추가.
- **M35** (lifecycle hook inject + claude-code 호환 cap=0) — `enqueue_injection` 의 동일 injected_context 인프라를 hook 도 사용. wire format 동일 (system_reminder JSON).
- **M32** (assistant streaming sibling broadcast) — M31 의 turn 으로 생성된 응답이 sibling client 에는 안 보이는 문제. 별개 milestone.
- **M11** (multi-attach), **M15** (user-message fanout) — multi-client 기초.

## Round history

- **Pre-M21 ver.2**: 사용자 round 14 보고 — 결과 안 받음. → 개념 단계.
- **commit a459b0a1** "Add background completion wake channel" — mpsc channel + register/unregister API.
- **commit b085caef** "Implement background completion auto-inject" (Tue May 12 12:33 2026) — inject path 완성, `client_lifecycle` wire, unit tests 9개.
- **Round 23 (이 문서)** — 라이브 검증, PASS 확인, 문서화.
