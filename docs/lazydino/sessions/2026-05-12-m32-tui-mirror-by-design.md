# M32 재확인 — TUI mirror 는 BY-DESIGN 으로 미지원

작성일: 2026-05-12 (Round 25)
관련 milestone: M32, M11, M15
관련 문서: `docs/MULTI_SESSION_CLIENT_ARCHITECTURE.md`

## 사용자 라이브 재현

사용자 보고: "term 으로 하나 띄우고 다른거 해당 term 에 연결해서 두개 같은 세션 띄운 다음에 한쪽에서 채팅쓰면 거기의 대답도 다른 세션에 보여야하는거 아니냐? 안 된다."

즉 같은 `session_id` 에 두 TUI 동시 attach 후 한쪽 입력 → 양쪽 streaming mirror 를 기대.

라이브에서 안 됨 (M32 Round 23 fix 가 deploy 됐는데도).

## 진단 — upstream design 이 명시적으로 막음

### `docs/MULTI_SESSION_CLIENT_ARCHITECTURE.md` 인용

**Non-Goal (line 60-62)**:
> "Supporting fully concurrent editing from multiple interactive attachments to the same session in the first version."

**v1 design rule (line 226-244)**:
> "For an initial implementation, a session should have one active interactive surface at a time."
> "This avoids synchronization problems with: multiple input drafts, racing submissions, cursor/focus conflicts, duplicate interactive ownership of the same session"
> "A future design may allow richer mirroring or passive previews, but v1 should prefer a single active controller per session."

### 코드 동작 — `handle_resume_session` 의 takeover 분기

`src/server/client_session.rs:798-846`:

```rust
let conflicting_live_client = {
    let connections = client_connections.read().await;
    connections
        .values()
        .find(|info| {
            info.client_id != client_connection_id && info.session_id == session_id
        })
        .cloned()
};
// ...
if can_take_over_live_session {
    // ...
    if let Some(disconnect_tx) = disconnect_tx {
        let _ = disconnect_tx.send(());   // <- 기존 client 강제 disconnect
    }
}
```

즉 `jcode --resume <id>` 로 두 번째 TUI 가 같은 session 에 붙으면 첫번째 TUI 가 takeover 로 disconnect 됨. **sibling 두 client 가 동시에 존재하는 상태 자체가 안 됨**.

## M32 Round 23 fix 의 진짜 의미

`src/server/client_lifecycle.rs::start_processing_message` 에 추가된 fanout wrapper task (commit `97d58e67`) 는 다음 경로에서만 효과 있음:

1. **swarm worker → parent UI** (M38 의 worker report 가 이 path 로 도착)
2. **SDK client + TUI 가 같은 session 에 동시** (SDK 는 takeover 분기 안 탐)
3. **debug attach** (`jcode debug attach`)

즉 **TUI ↔ TUI 동시 mirror 는 처음부터 동작 안 했고**, Round 23 "동시에 보여 이제" 의 라이브 PASS 는 위 1/2/3 케이스 중 하나였던 것으로 추정 (정확한 시나리오 미기록).

## 사용자 결정 — (A) upstream 의도대로 닫음

옵션:
- **(A) BY-DESIGN 으로 닫음** ← 사용자 선택
- (B) fork 로 multi-attach concurrent mirror 추가 (race / input draft 충돌 떠안기)
- (C) read-only follow (`--follow <session>`) 새 milestone 등록

사용자 결정: **(A)**. M32 fanout 코드는 SDK/swarm/debug fanout 용으로 유지. 진짜 mirror 가 나중에 필요해지면 (C) 같은 새 milestone 으로 분리 등록.

## 변경

- `LAZYDINO_MILESTONES.md` M32 행을 `🟢 DONE (fanout 코드는 deploy 됨) + 🟡 PARTIAL (TUI mirror 는 BY-DESIGN 미지원)` 으로 정정.
- 코드 변경 없음. fanout 코드 유지 (SDK/swarm 용도).

## 향후 만약 mirror 가 필요해진다면

새 milestone (예: M39) 으로 `--follow <session>` read-only stream 모드 추가 권고:
- 두 번째 client 가 `--follow` 로 붙으면 takeover 안 함, 입력 차단, server event 만 수신
- input draft / racing submission 문제 회피
- 1방향 mirror 만 지원 (관전 모드)

이게 upstream "future design may allow richer mirroring or passive previews" 가 가리키는 방향.
