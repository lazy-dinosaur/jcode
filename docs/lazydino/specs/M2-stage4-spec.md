# M2 Stage 4 명세서 — Worker Heartbeat + Per-Task Timeout

> 작성: 2026-05-11 (round 10 main session)
> 인계 대상: 별도 implementation 세션 (mythology: `hephaestus` 또는 `coder` profile)
> 검증 대상: 본 메인 세션이 다시 받아 라이브 E2E 까지 verify

## Context

M2 milestone (issue #76 — autonomous swarm bug) 의 4가지 root cause:

- **A. Worker 무응답 / await block** ← Stage 4 가 마지막으로 cover (이번 명세)
- **B. Spawn 폭주 / 10+ 터미널** — Stage 3 ✅ (hard cap)
- **C. 다른 cwd / path 에서 review** — Stage 3 ✅ (cwd allowlist)
- **D. No sandbox / no control** — Stage 1/2 ✅ (telemetry, JCODE_SWARM_NO_TERMINAL)

Oracle 분석 (`docs/lazydino/analysis/M11-stages-3-5.md` 와 별도, `/tmp/m2-stage2-analysis.md` 참조) 의 P0-3:

> Worker/task timeout and stale member transition.
> Location: `src/server/comm_control.rs::spawn_assigned_task_run`; `src/server/comm_session.rs` headless startup task; `src/server/swarm.rs` status refresh.
> What: per-task max runtime and heartbeat; mark member `running_stale` or `failed` after no status/tool events; surface in `await_members` before 60 minutes.
> Complexity: H. Risk: H, provider/tool long-running legitimate tasks.

**위험도 H 이유**: 정상적으로 오래 걸리는 작업 (큰 codebase 분석, 긴 LLM 응답, multi-step tool chain) 을 false positive 로 timeout 시킬 위험. **해결: 공격적 timeout 보다 heartbeat 기반 lazy 감지 + 사용자 surface**. 자동 kill 안 함, 사용자가 결정.

## 설계 결정 (확정)

### Heartbeat 모델

Stage 2 의 plan-level `running_stale` 패턴 (`26a5ccab`) 을 **member-level 로 확장**.

1. **Heartbeat source = 모든 의미 있는 이벤트**:
   - LLM streaming chunk 수신
   - Tool call 시작 / 완료
   - Status 변경
   - 그 외 worker liveness 신호

2. **Storage**: `SwarmMember` 에 3개 필드 추가:
   - `last_heartbeat_at: Option<SystemTime>`
   - `last_tool: Option<String>`
   - `last_checkpoint: Option<String>` (LLM output 첫 80자 truncated)

3. **Update path**:
   - `process_message_streaming_mpsc` 가 chunk/tool 이벤트마다 `swarm.touch_heartbeat(session_id, event)` 호출
   - Status 변경 헬퍼에서 자동 touch
   - Idempotent

### Stale 감지

1. **Threshold default**: 180초 (3분). Env `JCODE_WORKER_HEARTBEAT_STALE_SECS` / config `swarm.heartbeat_stale_secs`.
2. **Rule**: `status == "running" AND now - last_heartbeat_at > threshold` → `status = "running_stale"`
3. **Trigger (lazy 권장)**: `await_members`, `swarm:list`, `swarm:members` 호출 시점에 evaluate. CPU 부담 없음.
4. **`running_stale` reversible**: heartbeat 다시 들어오면 `running` 복귀. False positive 손해 최소화.

### Per-task hard timeout (별도)

Heartbeat 와 독립적으로, **task assignment 명시 `task_timeout_minutes`** 또는 **default cap** 지나면 강제 `failed`.

1. **Config**:
   - `swarm.default_task_timeout_minutes: Option<u32>` (default `None` = unlimited)
   - Env `JCODE_DEFAULT_TASK_TIMEOUT_MINUTES`
   - `assign_task` / `start_task` request schema 에 `task_timeout_minutes: Option<u32>` 추가

2. **Enforcement**: `src/server/comm_control.rs::spawn_assigned_task_run` 에서 `tokio::time::timeout(...)` 으로 `process_message_streaming_mpsc` await wrap.

3. **Timeout 시 액션**:
   - Task → `failed` (reason: `"exceeded task_timeout_minutes=N"`)
   - Member → `failed`
   - Event broadcast
   - 사용자 surface: error reason 명시 + cleanup 추천

4. **무제한 default 유지**: 사용자가 명시 설정해야만 enforce. 큰 작업 안전.

### `await_members` 에 stale surface

기존 deadline 단순 대기 → **추가**:

1. Loop 내부에서 lazy stale evaluation (5초 주기)
2. 응답 schema 에 `stale_members: Vec<{session_id, status, last_heartbeat_age_secs, last_tool, last_checkpoint}>` 추가
3. Tool output formatting 에서 `salvage / replace / cleanup` 추천 메시지 통합

### Telemetry 확장

`swarm:list`, `swarm:members` 결과:
- `last_heartbeat_secs_ago: u64`
- `last_tool: Option<String>`
- `last_checkpoint: Option<String>`

claude-code 의 stop hook 디버깅 메시지처럼 한눈에 보이도록.

## 코드 변경 위치 (정확)

| 파일 | 변경 |
|---|---|
| `src/server/swarm.rs` 또는 member struct 정의 | `SwarmMember.last_heartbeat_at`, `.last_tool`, `.last_checkpoint` |
| `crates/jcode-protocol/src/lib.rs` | `SwarmMemberInfo` 에 heartbeat 필드. `CommAwaitMembers` response 에 `stale_members` |
| `crates/jcode-config-types/src/lib.rs` | `SwarmConfig.heartbeat_stale_secs: Option<u32>`, `.default_task_timeout_minutes: Option<u32>` |
| `src/config/config_file.rs` + `default_file.rs` | partial + docs |
| `src/server/swarm.rs` | `touch_heartbeat`, `evaluate_running_stale` |
| `src/server/process_message_*.rs` (streaming mpsc) | chunk 수신 / tool start 시 touch_heartbeat |
| `src/server/comm_control.rs::spawn_assigned_task_run` | `tokio::time::timeout` wrap, default 적용 |
| `src/server/comm_await.rs` | loop 안 lazy eval, response 에 stale_members |
| `src/server/debug_swarm_read.rs` | swarm:list/members 출력 확장 |
| `src/tool/communicate.rs` | `await_members` 응답 formatting + `assign_task` schema |
| `src/server/comm_session_tests.rs` (+ swarm_tests.rs 신규 가능) | unit tests |

### 의사코드 — heartbeat touch

```rust
// src/server/swarm.rs

pub enum HeartbeatEvent {
    Chunk,
    ToolStart { name: String },
    Checkpoint { summary: String },
}

pub async fn touch_heartbeat(
    members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    session_id: &str,
    event: HeartbeatEvent,
) {
    let mut guard = members.write().await;
    if let Some(m) = guard.get_mut(session_id) {
        m.last_heartbeat_at = Some(SystemTime::now());
        match event {
            HeartbeatEvent::ToolStart { name } => m.last_tool = Some(name),
            HeartbeatEvent::Checkpoint { summary } => {
                m.last_checkpoint = Some(summary.chars().take(80).collect());
            }
            HeartbeatEvent::Chunk => {}
        }
        if m.status == "running_stale" {
            m.status = "running".to_string();
        }
    }
}
```

### 의사코드 — lazy stale eval

```rust
pub fn evaluate_running_stale(
    members: &mut HashMap<String, SwarmMember>,
    threshold: Duration,
) -> Vec<String> {
    let now = SystemTime::now();
    let mut transitioned = Vec::new();
    for (sid, m) in members.iter_mut() {
        if m.status != "running" {
            continue;
        }
        let reference = m.last_heartbeat_at.or(m.joined_at);
        if let Some(t) = reference {
            if now.duration_since(t).unwrap_or_default() > threshold {
                m.status = "running_stale".to_string();
                transitioned.push(sid.clone());
            }
        }
    }
    transitioned
}
```

### 의사코드 — per-task timeout

```rust
// src/server/comm_control.rs::spawn_assigned_task_run

let timeout_secs = task.task_timeout_minutes
    .or_else(resolve_default_task_timeout_minutes)
    .map(|m| Duration::from_secs(m as u64 * 60));

let result = match timeout_secs {
    Some(d) => tokio::time::timeout(d, process_message_streaming_mpsc(...)).await,
    None => Ok(process_message_streaming_mpsc(...).await),
};

match result {
    Ok(Ok(_)) => { /* 기존 success */ }
    Ok(Err(e)) => { /* 기존 error */ }
    Err(_elapsed) => {
        mark_task_failed(task_id, "exceeded task_timeout_minutes").await;
        mark_member_failed(session_id, "task timeout").await;
        broadcast_event(SwarmEvent::TaskFailed { ... }).await;
    }
}
```

### 의사코드 — await_members surface

```rust
// src/server/comm_await.rs

loop {
    tokio::select! {
        evt = swarm_event_rx.recv() => { /* 기존 처리 */ }
        _ = sleep(Duration::from_secs(5)) => {
            let mut guard = swarm_members.write().await;
            let stale = evaluate_running_stale(&mut guard, threshold);
            // surface 정보 누적
        }
        _ = sleep_until(deadline) => break,
    }
}

AwaitResponse {
    completed: ...,
    stale_members: collect_stale_member_info(&guard),
    ...
}
```

## Tests 필수 항목

### Unit tests

1. `touch_heartbeat_updates_last_heartbeat_at`
2. `touch_heartbeat_records_last_tool`
3. `touch_heartbeat_restores_running_from_stale`
4. `evaluate_running_stale_transitions_after_threshold` (mock now + tokio time pause)
5. `evaluate_running_stale_skips_non_running`
6. `evaluate_running_stale_uses_joined_at_when_no_heartbeat`
7. `resolve_heartbeat_stale_secs_env_overrides_config`
8. `per_task_timeout_marks_member_failed`
9. `per_task_timeout_none_means_unlimited`
10. `per_task_timeout_in_assign_task_request_overrides_default`
11. `await_members_response_includes_stale_members`

### Integration test (선택)

12. End-to-end mock: spawn → hang → swarm:members shows running_stale → touch → running 복귀

## Live E2E 시나리오

```bash
mkdir -p /tmp/jcode-stage4-e2e/{root/.jcode,log}
cat > /tmp/jcode-stage4-e2e/root/.jcode/config.toml <<'TOML'
[swarm]
heartbeat_stale_secs = 5
default_task_timeout_minutes = 1
max_active_spawns_per_coordinator = 4
TOML
```

1. **heartbeat stale → 복귀**: spawn worker, mock provider 가 8초 sleep → 5초 후 `running_stale` 표시 → chunk 도착 → `running` 복귀
2. **per-task timeout**: `assign_task task_timeout_minutes=1`, mock 무한 sleep → 60초 후 task/member `failed`
3. **await_members stale surface**: 2 worker hang → await_members 응답에 `stale_members` 2개
4. **explicit override**: default=1, task=5 → 5분 timeout 적용

⚠️ Mock provider 필요 — `--provider jcode` 또는 fake bash tool. 비결정성 줄이려면 unit 이 핵심.

## 변경 규모 추정

- 파일: 10-12
- 줄: +900/-50
- Build: incremental ~5분
- Test: unit 2-3초 추가 (tokio::time::pause 활용)

## Done 기준

- [ ] `SwarmMember` 에 heartbeat 필드 3개 추가
- [ ] `HeartbeatEvent` enum + `touch_heartbeat` helper
- [ ] streaming mpsc + tool call 시점 touch_heartbeat
- [ ] `evaluate_running_stale` lazy eval helper
- [ ] `await_members` / `swarm:list` / `swarm:members` 호출 시 lazy eval
- [ ] `heartbeat_stale_secs` (default 180) + env override
- [ ] `default_task_timeout_minutes` (default None = unlimited) + env override
- [ ] `assign_task` / `start_task` schema 에 `task_timeout_minutes` 추가
- [ ] `spawn_assigned_task_run` 에 tokio timeout 적용
- [ ] Timeout 시 task/member 모두 failed, broadcast, reason 명시
- [ ] `await_members` 응답에 `stale_members` 추가
- [ ] Debug API 출력 에 `last_heartbeat_secs_ago` / `last_tool` / `last_checkpoint`
- [ ] Unit tests 11개, 기존 회귀 없음
- [ ] `cargo +nightly build --release` 성공
- [ ] Live E2E 4 시나리오 검증
- [ ] `default_file.rs` docs (heartbeat / task timeout 섹션)
- [ ] Commit on `patch/swarm-stage4` (from `patch/swarm-stage3`)
- [ ] Cherry-pick to `deploy/m9-m10`
- [ ] 4-path deploy + sha256 일치
- [ ] Fork push (`deploy/m9-m10` + `patch/swarm-stage4`)
- [ ] `docs/lazydino/milestones/M2.md` Stage 4 ✅ + E2E 결과
- [ ] `LAZYDINO_MILESTONES.md` M2 status 갱신 (모든 P0 cleared → M2 complete)
- [ ] 메인 세션에 결과 보고

## 위험 / 주의사항

### Default 값 보수적

- `heartbeat_stale_secs = 180` (3분): 일반 LLM + tool chain 안전. 60초 면 false positive.
- `default_task_timeout_minutes = None`: 사용자 명시 설정만 enforce. Trust model.
- `running_stale` reversible: heartbeat 복귀 시 running 복원.

### Timer mocking

- `tokio::time::pause()` + `advance(Duration)` 필수. Real-time sleep 금지.
- `SystemTime::now()` 직접 호출 대신 trait/closure injection 또는 Tokio Instant.

### Worker hang 양면성

- 정상 hang (오래 걸리는 LLM, 큰 file edit): timeout 금지
- 비정상 hang (deadlock, network freeze, rate limit): surface 필수
- 해결: heartbeat (3분) 으로 **surface 만**. 자동 kill 안 함. Per-task timeout opt-in.

### `last_tool` / `last_checkpoint` 보안

- Tool name 공개 OK
- Tool input arg 노출 X
- `last_checkpoint` 80자 truncate + PII sanitize

### Plan-level vs member-level `running_stale`

Stage 2 가 plan item 에 `running_stale` 도입. Stage 4 는 member-level. 두 개념 분리. Cleanup default 에 둘 다 포함 (`crates/jcode-protocol/src/lib.rs:1434-1444` 갱신).

### `await_members` breaking change?

`stale_members` 필드 **추가**. 기존 client ignore OK. Tool output formatting 에서:

```
Await incomplete. Timed out.
Still waiting on: cow (running_stale, last_tool=read_file, last_heartbeat=4m12s ago), snail (running)
Suggested: jcode swarm cleanup cow  OR  swarm replace cow
```

### 절대 금지 사항

- PID 1770980 메인 server kill 금지
- Default timeout finite 금지
- Heartbeat miss = 자동 kill 금지 (surface only)
- Mojibake hygiene
- 빌드 timeout 300000ms = 5분

## 보고 형식 (완료 시)

```
✅ M2 Stage 4 완료

| 항목 | 값 |
|---|---|
| Source SHA | <commit> on patch/swarm-stage4 |
| Deploy SHA | <commit> on deploy/m9-m10 |
| Binary tag | lazydino-<SHA> |
| sha256 (4-path 일치) | <hash> |
| Files changed | <N>, +<a>/-<b> |
| Unit tests | <N> passed |
| Build | cargo +nightly build --release ✅ |

Live E2E:
| 시나리오 | 결과 |
| heartbeat stale → 복귀 | <pass/detail> |
| per-task timeout (member failed) | <pass/detail> |
| await_members surface stale | <pass/detail> |
| explicit timeout overrides default | <pass/detail> |
```

---

이 명세서 그대로 작업 시작하고 마치면 메인 세션에 결과 보고. 메인이 git diff / unit test / 4-path sha256 / live E2E 직접 재실행 검증.
