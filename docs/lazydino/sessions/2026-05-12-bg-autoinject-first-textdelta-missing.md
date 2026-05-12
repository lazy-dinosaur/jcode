# 2026-05-12 — bg auto-inject 첫 TextDelta 미표시 (B 버그)

> Oracle (gpt-5.5 high) subagent 분석 결과 그대로 보존 + 다음 세션 인수인계.
> **별도 milestone 후보. 아직 patch 미작성, 미빌드.**

## 1. 발견 경위

작업하던 메인 이슈는 "`/` slash-prefix prompt 가 remote 에서 stuck" (commit `3a1a4712` 으로 fix 완료) 이었음. 그 와중에 사용자가 라이브 swarm 사용 중 별개 증상 호소:

> "Stadium remote client 에서 bg auto-inject 로 새 turn 이 시작되어도 화면이 한참 안 갱신된다 (몇 초 후에야 token 들이 한꺼번에 그려짐)."

이를 `subagent_type=oracle, effort=high` 로 약 5분 정밀 분석 의뢰. 결과 요약:

## 2. Oracle 분석 결론

**1차 가설 ("redraw interval 이 idle 이라서 안 그려짐") 은 맞지만 불충분.** server-initiated turn 에서는 client `App.current_message_id` 자체가 안 잡혀서, 마지막 `Done { id }` 이벤트가 "unrelated Done" 으로 무시될 위험까지 있음. 따라서 단순 "TextDelta 마다 redraw 강제 (옵션 A)" 만으로는 불완전.

### 검증 디테일

1. **native Linux + kitty 에서는 `eager_stream_redraw == false`**
   - `perf.rs`: native synthetic profile terminal=`kitty`, tier=`Full`
   - `tui_policy_for`: `enable_decorative_animations = !Minimal` → native kitty 에서 true
   - 따라서 `eager_stream_redraw = !enable_decorative_animations = false`
   - WSL/Windows Terminal 만 eager=true 로 내림

2. **server-initiated turn 에서 client-side `is_processing` 즉시 true 안 됨**
   - Local 경로 `begin_remote_send()` 만:
     - `current_message_id = Some(msg_id)`
     - `is_processing = true`
     - `status = Sending`
   - bg auto-inject 는 server `client_lifecycle.rs` 에서 `start_processing_message()` 호출하지만, **client TUI 는 `Request::Message` 를 보낸 적이 없어서** 위 local state setup 이 안 일어남.
   - server-side `client_connections.info.is_processing = true` 는 server metadata 일 뿐, 이미 연결된 TUI `App` 상태를 직접 바꾸지 않음.
   - `History { activity }` 에서만 client `app.is_processing = true` 가 되는데 이건 bootstrap/resume payload 시점이지 live turn start event 가 아님.

3. **`UserMessage` sibling echo 도 processing state 안 켬**
   - `src/tui/app/server_events.rs:556` branch:
     - `id` 를 버림
     - `is_processing` 설정 없음
     - `current_message_id` 설정 없음
     - `return false`
   - M32 sibling fanout 이후 특히 취약

4. **첫 `TextDelta` 시점 redraw interval 이 idle**
   - 현재 `TextDelta` branch: `is_processing=false && status=Idle` 이면 status 를 Streaming 으로 안 바꿈
   - `stream_buffer.push()` 가 newline/timeout 전이면 `streaming_text` 도 empty
   - return 값: `eager_stream_redraw && needs_redraw` → native full-tier 에서 false
   - loop 가 `event_redraw=false` 로 즉시 draw 안 함, 다음 tick 은 `REDRAW_IDLE=250ms` 또는 deep idle `1000ms`
   - **더 심각**: `Done` 은 `current_message_id == Some(id)` 아니면 무시 → final flush/commit 도 놓칠 수 있음

## 3. 권장 fix 방향

**최소 회귀 fix = "B + Done 보정 + UserMessage id 처리"**.
A 처럼 모든 `TextDelta` 를 항상 redraw=true 로 ��들면 full-tier tick-based throttling 의도를 깨고 token 당 draw 를 유발할 수 있어서 회피.

### 3.1 Live stream event 최초 수신 시 wake-up

```rust
// TextDelta / ToolStart / ToolExec 등 live stream event 최초 수신 시
if !app.is_processing {
    app.is_processing = true;
    app.processing_started.get_or_insert_with(Instant::now);
    app.last_stream_activity.get_or_insert_with(Instant::now);
    if matches!(app.status, ProcessingStatus::Idle) {
        app.status = ProcessingStatus::Thinking(Instant::now());
    }
    needs_redraw = true; // first wake-up redraw
}
```

### 3.2 `TextDelta` branch 추가

```rust
if matches!(
    app.status,
    ProcessingStatus::Idle
        | ProcessingStatus::Sending
        | ProcessingStatus::Connecting(_)
        | ProcessingStatus::Thinking(_)
) {
    app.status = ProcessingStatus::Streaming;
    needs_redraw = true;
}

// preserve throttling: not unconditional per-token redraw
(eager_stream_redraw && needs_redraw) || started_remote_turn
```

### 3.3 `Done` branch 보정

현재:
```rust
if app.current_message_id == Some(id) {
```

대신:
```rust
let completed_current_message =
    app.current_message_id == Some(id)
        || (app.current_message_id.is_none() && app.is_processing);

if completed_current_message {
    // existing flush/commit/idle cleanup
}
```

### 3.4 `UserMessage` branch sibling/client fanout 최소 처리

```rust
app.current_message_id = Some(id);
app.is_processing = true;
app.processing_started.get_or_insert_with(Instant::now);
app.last_stream_activity.get_or_insert_with(Instant::now);
if matches!(app.status, ProcessingStatus::Idle) {
    app.status = ProcessingStatus::Thinking(Instant::now());
}
return true;
```

단, `content.is_empty()` 면 blank user message 는 push 하지 않는 게 좋음.

## 4. 더 좋은 장기 fix (옵션 C)

프로토콜에 명시적 이벤트 추가:

```rust
ServerEvent::TurnStart {
    id,
    session_id,
    origin: ServerInitiated | ClientMessage | BackgroundInject | Sibling,
}
```

Client 가 `current_message_id`, `is_processing`, `status`, redraw 를 정확히 설정 가능. `UserMessage` 를 id 전달용으로 오용 안 해도 됨. 단 protocol/client/server 모두 건드려야 해서 회귀 fix 로는 무거움 → **별도 milestone 으로 분리 권장**.

## 5. 기존 설계 평가

`eager_stream_redraw = !enable_decorative_animations` 의 의도는 합리적:
- full-tier: decorative animations/tick loop 가 있으니 streaming redraw 는 tick 기반 throttle
- WSL/minimal: animations off 라 passive tick 이 느려지므로 event 가 직접 eager redraw

회귀는 이 설계 자체보다 **server-initiated turn 이 client `is_processing`/`current_message_id` state machine 을 우회한 데서 발생**. 따라서 per-token eager redraw 를 복구하기보다 "remote stream start 를 client state 에 반영" 하는 fix 가 안전.

## 6. 관련 파일

- `src/tui/app/server_events.rs` (특히 `:556` UserMessage sibling branch, TextDelta/Done branches)
- `src/tui/app/remote/` (begin_remote_send 경로)
- `src/server/client_lifecycle.rs:2898` (start_processing_message)
- `src/agent/turn_streaming_mpsc.rs:1056-1113` (Done emit 경로)
- `src/perf.rs` (terminal profile / eager_stream_redraw 결정)

## 7. 다음 세션 액션

1. 새 milestone 번호 부여 (M39 후보)
2. 회귀 테스트 먼저 작성 (server-initiated turn → 첫 TextDelta → 즉시 redraw 검증). headless TUI test framework 활용.
3. fix 3.1~3.4 단계별로 commit 분리:
   - `fix(tui): wake up client state on first live-stream event`
   - `fix(tui): handle Done without current_message_id when processing`
   - `fix(tui): set current_message_id from sibling UserMessage echo`
4. M32 BY-DESIGN 결정과의 일관성 재확인 (M32 가 sibling fanout 을 upstream design 이라 봤는데, sibling 측 client state 처리는 별개 이슈)
5. 옵션 C (protocol TurnStart 이벤트) 는 별도 milestone 으로 검토 — 회귀 위험 vs 장기 정확도 tradeoff

## 8. 한계

- Oracle 가 코드 단편 읽고 한 정밀 정적 분석. 실제 native kitty 라이브 reproduce 미수행
- 사용자가 보고한 "한참 후 한꺼번에 그려짐" 증상이 정확히 `REDRAW_IDLE=250ms` 인지 deep idle `1000ms` 인지 미측정
- WSL/minimal profile 에서는 동일 증상이 안 나타날 가능성 — terminal-profile dependent regression
