# M38 — Swarm worker report body delivery to parent

날짜: 2026-05-12 (Round 24, 라이브 19:48 UTC).
브랜치: `deploy/m9-m27-catchup`.
Commits:

- fix: `3671fae7` — `fix(m38): swarm worker report body 가 status-unchanged 일 때 부모에게 전달 안 되던 문제 수정`
- test: `a562067e` — `test(m38): worker report body 가 status 안 바뀔 때도 부모에게 전달되는지 회귀 테스트 추가`

## 사용자 라이브 보고 (한 줄)

> "지금 swarm 의 작업이 끝나거나 부모에게 전송이 되어야할 정보들이 전달이 안되고있는거잔아"

증상 매트릭스:

- ✅ swarm DM (`action=dm` / `message` / `channel`) path: 정상
- ❌ swarm worker → parent **completion/report body 전달 path**: 실패
  - worker 가 "작업 끝내고 대기중" 상태인데 큰 audit body 가 parent UI 로 안 옴
- 라이브 status bar: `(stalled 19s) · 2m 3s · ↑105k ↓14 · https fallback: stream timeout`

## Root cause

`src/server/swarm.rs::update_member_status_with_report` 의
`should_notify_coordinator` 분기가 **`status_changed` 만 게이트** 로 사용.

```rust
let should_notify_coordinator = status_changed
    && ((status == "completed")
        || (report_back_to_session_id.is_some()
            && old_status == "running"
            && matches!(status, "ready" | "failed" | "stopped")));
```

실제 worker 흐름:

1. running → ready, body=짧은 상태문 → `status_changed=true` → 부모 받음 ✅
2. ready → ready, body=full audit report → `status_changed=false` → **fanout 안 됨** ❌
   - `member_changed=true` (because `report_changed=true`) 라서 함수가 early-return 도 안 함
   - `latest_completion_report` 에 새 body 가 덮어쓰기만 되고 거기 갇힘
   - 즉 데이터는 server 메모리 안에는 있지만 parent 의 mpsc tx 로 안 흐름

`CommReport` request 의 dispatch (`src/server/client_lifecycle.rs:479-512`) 는
오로지 `update_member_status_with_report` 한 경로에만 의존하므로 위 빈틈이
프로토콜 수준에서 그대로 노출됨.

## Fix

`src/server/swarm.rs` `update_member_status_with_report`:

1. 내부 mutable scope 에서만 정의되던 `report_changed` 를 outer tuple 로
   끌어올려 if-block 까지 propagate.
2. `should_notify_coordinator` 에 `report_changed` 분기 추가:

```rust
let should_notify_coordinator = (status_changed
    && ((status == "completed")
        || (report_back_to_session_id.is_some()
            && old_status == "running"
            && matches!(status, "ready" | "failed" | "stopped"))))
    || (report_changed
        && report_back_to_session_id.is_some()
        && matches!(status, "ready" | "failed" | "stopped" | "completed"));
```

`report_changed` 정의:

```rust
let report_changed =
    completion_report.is_some() && member.latest_completion_report != completion_report;
```

이미 `member.latest_completion_report` 와 다를 때만 true 이므로 **같은
본문 재발사는 자동 dedup**. spam 위험 없음.

## 검증

### 단독 reproduction (fix 전)

```text
running 1 test
test server::swarm::tests::update_member_status_with_report_notifies_when_only_body_changes ... FAILED

failures:
---- update_member_status_with_report_notifies_when_only_body_changes stdout ----
panicked: coordinator must also receive the second report body even though status did not change;
got events: []
```

→ `got events: []` 로 라이브 증상 (parent 가 두 번째 body 못 받음) 정확히 재현.

### Fix 후

```text
test server::swarm::tests::update_member_status_with_report_notifies_when_only_body_changes ... ok
test server::swarm::tests::update_member_status_with_report_dedups_identical_body ... ok
```

둘 다 PASS. 두 번째 dedup 테스트는 같은 본문 재발사 시 추가 fanout 0건 보장.

### 회귀 (parallel)

```text
cargo test --lib server::swarm::    → 17 passed; 0 failed
cargo test --lib server::client_lifecycle  → 6 passed; 0 failed
cargo test --lib server::comm_control      → 19 passed; 0 failed
cargo test --lib server::comm_session      → 31 passed; 0 failed
```

총 73건 0 fail.

## 가설 진단 결과

| 가설 | 결과 |
|-----|------|
| 1: status-unchanged report 가 fanout 안 됨 | ✅ 확정. fix 됨 |
| 2: report_back_to_session_id None 인 worker 의 fallback coordinator 탐색 실패 | 해당 코드 path 분석함 — 가능은 하지만 라이브 시나리오에서 worker 는 `report_back_to_session_id=Some(parent)` 였으므로 1차 원인 아님 |
| 3: event_txs 비어 fanout drop ("delivered to 0 siblings" 0 카운트) | `fanout_session_event` (`src/server/state.rs:333`) 가 event_txs 비면 fallback `event_tx` 로 보냄. 본 fix 와 무관. 라이브에서 `0 siblings` 로그 보였다면 별도 진단 필요 |
| 4: parent UI 가 swarm Notification 렌더링 안 함 | 1차 원인 아님 — DM path 는 같은 `Notification` 으로 가는데 정상 표시됨 |

## 라이브 재현 가이드 (회귀 시 사용)

1. swarm spawn 으로 worker 한 명 spawn (auto: `report_back_to_session_id` 설정됨).
2. worker 에서 `swarm action="report" status="ready" message="short"` 한 번 보냄.
   parent 가 첫 알림 받는지 확인.
3. worker 에서 status 그대로 두고 같은 action 으로 더 큰 body 한 번 더 보냄.
   parent 에 새 알림이 오면 OK. 안 오면 회귀.

또는 unit-level 로:

```bash
cargo test --lib server::swarm::tests::update_member_status_with_report_notifies_when_only_body_changes
```

## 후속

- `M32 fanout 'delivered to 0 siblings'` 로그가 매 라이브 발생하는지 확인 필요 (가설 3 잔여).
- Fix 가 production 에 깔리려면 `scripts/lazydino/install-custom-jcode.sh` 실행 + 사용자가 server kill/restart. atomic mv + stable/current symlink 갱신만 본 작업에서 수행, **active server 는 kill 하지 않음**.
