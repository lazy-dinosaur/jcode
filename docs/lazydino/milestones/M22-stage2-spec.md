# M22 Stage 2 — Turn Loop Tool Fan-Out 명세서

**대상 브랜치**: `patch/m22-stage2-turn-loop-fanout`
**베이스**: `origin/master` (현재 tip 확인: `git fetch origin && git rev-parse origin/master`)
**목표**: 한 turn 안의 여러 `tool_use` 를 **진짜 병렬** 로 실행. 기존 `src/tool/batch.rs` 의 `FuturesUnordered` 패턴을 그대로 turn loop 로 이식.
**언어**: 한국어 commit 본문은 영문 위주 + 한국어 보조 OK (기존 관행 따라).
**author**: `lazydino <lazydino@users.noreply.github.com>`

---

## 1. 배경 요약 (self-contained)

### 1.1 증상
한 turn 안에 모델이 N≥2 개 tool_use 를 emit 했을 때 (또는 한 round 에 2개 subagent 가 호출됐을 때), tool 들이 **순차로** 실행되어 두 번째 이상이 첫 번째 완료까지 대기. 사용자 체감으로 "한 쪽 silence".

### 1.2 원인 위치
3 mirror 파일 모두 동일한 `for tool_index in 0..tool_count { ... .await ... }` 패턴 사용.

| 파일 | 라인 | 사용처 |
|---|---|---|
| `src/agent/turn_streaming_broadcast.rs` | 702 | broadcast 채널 기반 streaming turn |
| `src/agent/turn_streaming_mpsc.rs` | 784 | mpsc 채널 ���반 streaming turn |
| `src/agent/turn_loops.rs` | 1188 (tool exec) / 959 (count) | non-streaming turn (CLI/print 경로) |

### 1.3 이미 적용된 부분 (Stage 1)
- OpenAI Responses API `parallel_tool_calls = true` (default)
- env `JCODE_OPENAI_PARALLEL_TOOL_CALLS` 토글
- 파일: `src/provider/openai.rs:712`, `src/config/default_file.rs:182`, `src/config/env_overrides.rs:427`

### 1.4 참조 구현 (그대로 베껴 옴)
`src/tool/batch.rs:225` 부터 `FuturesUnordered` 로 parallel exec + `BatchProgress` 이벤트 + 순서 보존 (`results.sort_by_key`) + 에러 집계. **이 파일을 reference 로 삼고 turn loop 에 맞게 어댑팅**.

---

## 2. 비기능 요구사항 (Invariants)

다음을 **반드시** 지켜야 함. 위��� 시 회귀.

| ID | Invariant | 근거 |
|---|---|---|
| I1 | Anthropic 메시지 시퀀스에서 같은 assistant turn 의 모든 `tool_use` 직후 같은 인덱스 순서로 `tool_result` 가 와야 함 | Anthropic API 명세 |
| I2 | `validate_tool_allowed` 실패 / `validation_error` 가 있는 tool 은 실행 안 함, 에러 tool_result 만 추가 | 보안/계약 |
| I3 | `urgent_interrupt` 발생 시 (a) 시작 안 한 tool 은 `[Skipped: user interrupted]` 결과, (b) 시작한 tool 은 cancel 시도 후 (잘 안 되면 결과 받아서 처리) | 기존 UX 보존 |
| I4 | `sdk_tool_results` 에 이미 존재하는 (provider-내장 실행) tool 은 추가 실행하지 않음. native tool + sdk_error 인 경우만 fall-through | 기존 분기 보존 |
| I5 | `tool_results_dirty = true` 일 때만 `session.save()` 호출 | 디스크 I/O 절감 |
| I6 | `generated_image_contexts` drain 은 모든 tool 실행 후, 기존 위치 그대로 | 이미지 흐름 |
| I7 | `INJECTION POINT C` (urgent 도중) 와 `D` (모두 끝난 뒤) 의 의미 보존 | 사용자 입력 주입 점 |
| I8 | `unlock_tools_if_needed(tc.name)` 는 각 tool 완료 직후 (병렬이면 각자 완료 시점) 호출 | tool lock 상태 |
| I9 | `record_tool_call()` 텔레메트리는 tool 당 정확히 1번 | 카운팅 |
| I10 | `ToolStarted` / `ToolDone` 이벤트 (broadcast/mpsc) 는 tool 당 정확히 1쌍 emit, **시작 순서는 인덱스 순** 으로 emit | UI 가독성 |
| I11 | 같은 path 에 동시 write 하는 두 tool (`write`/`edit`/`apply_patch`/`multiedit`) 은 직렬화 | race 방지 — 자세한 건 §4.4 |
| I12 | 빌드: `cargo +nightly build --release` 무경고 | 기존 정책 |
| I13 | mermaid string count 변동 없음 (UI 회귀 없음), `"Mermaid rendering is disabled"` grep 0건 | 사용자 정책 |

---

## 3. 설계

### 3.1 Helper 함수 신설 (turn_execution.rs)

기존 `pub(super) fn unlock_tools_if_needed`, `pub(super) fn validate_tool_allowed` 옆에 신설:

```rust
// src/agent/turn_execution.rs

/// Per-tool 실행 결과. Tool 실행 직후 main turn loop 가 받아서
/// (a) Bus/event 발행, (b) message 추가, (c) telemetry/unlock 처리.
pub(super) struct DispatchedToolResult {
    pub index: usize,
    pub tc: ToolCall,                    // 원본 tool_call (id/name/input 유지)
    pub started_at: Instant,
    pub elapsed: Duration,
    pub result: Result<ToolOutput>,
}

impl Agent {
    /// 한 assistant turn 의 tool_calls 를 분류:
    ///   - skipped: validation_error / sdk_tool_results 로 결정된 것 (실행 안 함)
    ///   - to_execute: 실제 registry.execute() 가 필요한 것 (병렬 dispatch 대상)
    /// 호출자는 to_execute 만 FuturesUnordered 로 돌리고, 결과를 인덱스 순으로 재정렬.
    pub(super) fn classify_tool_calls(&self, tool_calls: &[ToolCall], sdk_tool_results: &HashMap<String, (String, bool)>) -> ClassifiedTools { ... }
}
```

`ClassifiedTools` 는 다음을 담음:

```rust
pub(super) struct ClassifiedTools {
    /// 인덱스 i 가 곧 원본 tool_calls 인덱스. content 는 사전 결정된 결과
    /// (validation error 또는 sdk-precomputed). main loop 가 그대로 추가.
    pub presets: Vec<(usize, PresetToolResult)>,
    /// 실제 dispatch 가 필요한 (인덱스, ToolCall) 쌍.
    pub to_execute: Vec<(usize, ToolCall)>,
}

pub(super) enum PresetToolResult {
    ValidationError(String),
    SdkProvided { content: String, is_error: bool },
}
```

### 3.2 Dispatch helper (turn_execution.rs)

```rust
impl Agent {
    /// to_execute 를 FuturesUnordered 로 병렬 실행.
    /// urgent_interrupt 발생 시 즉시 종료하고, 시작 안 한 tool 은
    /// "[Skipped: user interrupted]" 결과로 채워 반환.
    ///
    /// 같은 path 에 쓰는 tool 들 (write/edit/apply_patch/multiedit) 은
    /// PathWriteGuard (§4.4) 로 직렬화.
    pub(super) async fn dispatch_tools_parallel(
        &self,
        to_execute: Vec<(usize, ToolCall)>,
        ctx_factory: impl Fn(&ToolCall) -> ToolContext + Send + Sync,
        per_tool_start: impl Fn(&ToolCall) + Send + Sync,
        cancel_token: &CancellationToken,
    ) -> Vec<DispatchedToolResult> { ... }
}
```

핵심 본문:

```rust
use futures::stream::FuturesUnordered;
use futures::StreamExt;

let mut stream: FuturesUnordered<_> = to_execute
    .into_iter()
    .map(|(i, tc)| {
        let registry = self.registry.clone();
        let ctx = ctx_factory(&tc);
        let guard = self.write_guard_for(&tc); // §4.4 (None 이면 동시 진행 허용)
        let cancel = cancel_token.child_token();
        per_tool_start(&tc);
        let started_at = Instant::now();
        async move {
            let _permit = match guard {
                Some(g) => Some(g.acquire().await),
                None => None,
            };
            // cancel 체크: 시작도 안 한 채 취소되면 skip 결과 만들기
            if cancel.is_cancelled() {
                return DispatchedToolResult {
                    index: i,
                    tc: tc.clone(),
                    started_at,
                    elapsed: started_at.elapsed(),
                    result: Err(anyhow::anyhow!("[Skipped: user interrupted]")),
                };
            }
            let result = tokio::select! {
                r = registry.execute(&tc.name, tc.input.clone(), ctx) => r,
                _ = cancel.cancelled() => Err(anyhow::anyhow!("[Cancelled: user interrupted]")),
            };
            DispatchedToolResult {
                index: i,
                tc,
                started_at,
                elapsed: started_at.elapsed(),
                result,
            }
        }
    })
    .collect();

let mut out: Vec<DispatchedToolResult> = Vec::with_capacity(stream.len());
while let Some(r) = stream.next().await {
    if self.has_urgent_interrupt() {
        cancel_token.cancel();
        // 계속 drain 해서 남은 future 들의 결과 (cancelled or completed) 까지 받음
    }
    out.push(r);
}
// 인덱스 순서로 정렬
out.sort_by_key(|r| r.index);
out
```

### 3.3 Main loop 변경 (3 mirror 파일 동일하게)

기존:
```rust
for tool_index in 0..tool_count {
    if tool_index > 0 && self.has_urgent_interrupt() { ... break; }
    let tc = &tool_calls[tool_index];
    if let Some(err) = tc.validation_error() { ... continue; }
    self.validate_tool_allowed(&tc.name)?;
    if let Some((sdk_content, sdk_is_error)) = sdk_tool_results.remove(&tc.id) { ... continue; }
    // local execute
    let ctx = ToolContext { ... };
    let result = self.registry.execute(...).await;
    ...
}
```

신규 (의사코드):
```rust
let classified = self.classify_tool_calls(&tool_calls, &sdk_tool_results);

// 1) preset 결과 (validation error / sdk-provided) 먼저 message 추가
for (i, preset) in &classified.presets {
    match preset {
        PresetToolResult::ValidationError(err) => {
            event_tx.send(ServerEvent::ToolDone { id: tool_calls[*i].id.clone(), name: tool_calls[*i].name.clone(), output: err.clone(), error: Some(err.clone()) });
            self.add_message(Role::User, vec![ContentBlock::ToolResult { tool_use_id: tool_calls[*i].id.clone(), content: err.clone(), is_error: Some(true) }]);
            tool_results_dirty = true;
        }
        PresetToolResult::SdkProvided { content, is_error } => { /* 기존 분기 그대로 */ }
    }
}

// 2) Urgent interrupt 사전 체크 (모든 to_execute 가 skip 될 수 있음)
if !classified.to_execute.is_empty() && self.has_urgent_interrupt() {
    // 기존 INJECTION POINT C 로직 그대로
    for (i, tc) in &classified.to_execute {
        self.add_message(Role::User, vec![ContentBlock::ToolResult {
            tool_use_id: tc.id.clone(),
            content: "[Skipped: user interrupted]".into(),
            is_error: Some(true),
        }]);
    }
    // soft_interrupt 주입 + persist + break
} else if !classified.to_execute.is_empty() {
    let cancel_token = CancellationToken::new();
    let ctx_factory = |tc: &ToolCall| ToolContext {
        session_id: self.session.id.clone(),
        message_id: assistant_message_id.clone().unwrap_or_else(|| self.session.id.clone()),
        tool_call_id: tc.id.clone(),
        working_dir: self.working_dir().map(PathBuf::from),
        stdin_request_tx: self.stdin_request_tx.clone(),
        graceful_shutdown_signal: Some(self.graceful_shutdown.clone()),
        execution_mode: ToolExecutionMode::AgentTurn,
    };
    let per_tool_start = |tc: &ToolCall| {
        // ToolStarted 이벤트 — 시작 시점은 dispatch_tools_parallel 안에서
        // 인덱스 순서 보장 위해 collect 단계에서 호출.
        event_tx.send(ServerEvent::ToolStarted { ... });
        logging::info(&format!("Tool starting: {}", tc.name));
    };

    let results = self.dispatch_tools_parallel(
        classified.to_execute,
        &ctx_factory,
        &per_tool_start,
        &cancel_token,
    ).await;

    // 3) 결과를 인덱스 순서대로 message 추가 (I1)
    for r in results {
        crate::telemetry::record_tool_call();
        self.unlock_tools_if_needed(&r.tc.name);
        logging::info(&format!("Tool finished: {} in {:.2}s", r.tc.name, r.elapsed.as_secs_f64()));
        match r.result {
            Ok(output) => { /* 기존 OK 분기 그대로 */ }
            Err(e) => { /* 기존 Err 분기 그대로 */ }
        }
        tool_results_dirty = true;
    }
}

// 4) INJECTION POINT D / save / generated_image_contexts — 기존 그대로
```

### 3.4 broadcast vs mpsc vs loops 차이

| 측면 | broadcast | mpsc | loops (non-streaming) |
|---|---|---|---|
| event 채널 | `event_tx: broadcast::Sender<ServerEvent>` | `event_tx: mpsc::Sender<...>` | Bus 직접 publish |
| ToolStarted | `ServerEvent::ToolStarted` send | 동일 | `BusEvent::ToolUpdated(Running)` |
| ToolDone | `ServerEvent::ToolDone` send | 동일 | `BusEvent::ToolUpdated(Completed)` |
| print_output | 없음 | 없음 | `print!("\n  → ")` 등 stdout 출력 |
| SubagentStatus | 없음 | 없음 | `Bus::global().publish(BusEvent::SubagentStatus(...))` |

→ helper `dispatch_tools_parallel` 자체는 channel-agnostic 으로 만들고, `per_tool_start` / `per_tool_done` 콜백을 각 mirror 가 다르게 주입. loops 의 `print_output` 케이스는 ordering 어려우므로 (인덱스 순서로 print 해야 자연스러움) **결과 정렬 후 한 번에 출력** 으로 변경. 사용자 체감엔 차이 없음.

---

## 4. 위험 항목별 처리

### 4.1 Tool result ordering (I1)
`DispatchedToolResult.index` 로 `sort_by_key`. preset 결과는 별도 vec 에서 인덱스 보존. 최종 message 추가 시 (preset + dispatched) 합쳐서 인덱스 순.

```rust
let mut all: Vec<(usize, MessageBlocks)> = vec![];
for (i, preset) in classified.presets { all.push((i, build_blocks_from_preset(preset))); }
for r in results { all.push((r.index, build_blocks_from_result(r))); }
all.sort_by_key(|(i, _)| *i);
for (_, blocks) in all { self.add_message_with_duration(Role::User, blocks, duration); }
```

### 4.2 Urgent interrupt cancel
`tokio_util::sync::CancellationToken` 사용 (이미 deps 에 있을 가능성 큼, 없으면 추가). 각 future 는 `tokio::select!` 로 cancel 감지. 단,
- 일부 tool 은 cancel 무시하고 끝까지 실행 (예: bash subprocess) → 이건 기존 동작과 동일. cancel signal 보내고 결과는 받아서 처리.
- 모든 dispatched future drain 후 `out` 정렬.

`tokio_util` 의존성 확인:
```bash
grep tokio_util Cargo.toml
```
없으면:
```toml
tokio-util = { version = "0.7", features = ["rt"] }
```

### 4.3 SDK pre-provided tool results
classify 단계에서 `sdk_tool_results.remove(&tc.id)` 처리. native tool + sdk_error 인 경우만 to_execute 로 fall-through.

```rust
fn classify_tool_calls(&self, tool_calls: &[ToolCall], sdk_tool_results: &HashMap<String, (String, bool)>) -> ClassifiedTools {
    let mut presets = vec![];
    let mut to_execute = vec![];
    let mut sdk_taken = sdk_tool_results.clone(); // remove 대용
    for (i, tc) in tool_calls.iter().enumerate() {
        if let Some(err) = tc.validation_error() {
            presets.push((i, PresetToolResult::ValidationError(err)));
            continue;
        }
        if self.validate_tool_allowed(&tc.name).is_err() {
            // 기존 코드는 ? 로 propagate. 동일 시맨틱 유지: 호출자에서 ? 전 단계로 처리.
            // → classify 가 Result<ClassifiedTools> 를 반환하게 변경.
        }
        let is_native = JCODE_NATIVE_TOOLS.contains(&tc.name.as_str());
        if let Some((content, is_error)) = sdk_taken.remove(&tc.id) {
            if !(is_native && is_error) {
                presets.push((i, PresetToolResult::SdkProvided { content, is_error }));
                continue;
            }
        }
        to_execute.push((i, tc.clone()));
    }
    ClassifiedTools { presets, to_execute }
}
```

**주의**: `validate_tool_allowed` 가 `Err` 면 기존 코드는 `?` 로 propagate 했음. classify 함수도 `Result<ClassifiedTools>` 반환하도록 변경.

### 4.4 Same-path write race
**옵션 A (보수적, 권장)**: write/edit/apply_patch/multiedit 은 무조건 **하나만** 동시 진행 (path 무관 단순 mutex).

```rust
// Agent state 에 추가
write_serializer: Arc<tokio::sync::Mutex<()>>,
```

`dispatch_tools_parallel` 의 future 안에서:
```rust
let needs_write_lock = matches!(tc.name.as_str(), "write" | "edit" | "apply_patch" | "multiedit");
let _write_guard = if needs_write_lock {
    Some(self.write_serializer.clone().lock_owned().await)
} else { None };
```

**옵션 B (정교)**: path 추출 → path 별 mutex. 복잡도 높음. M22-3 로 미루고 옵션 A 로 시작.

**결정**: 옵션 A. 코드 ~5줄. 회귀 위험 최소.

### 4.5 Event ordering for UI
`per_tool_start` 콜백을 `FuturesUnordered::collect` **이전** 에 인덱스 순서로 호출 (즉 future 안이 아니라 future 만드는 단계에서 동기적으로 호출). 그러면 `ToolStarted` 는 항상 인덱스 순, `ToolDone` 은 완료 순. 사용자 UI 가 자연스럽게 interleave.

```rust
let stream = to_execute.into_iter().map(|(i, tc)| {
    per_tool_start(&tc); // ← 여기서 동기 호출, 인덱스 순서 보장
    async move { ... }
}).collect::<FuturesUnordered<_>>();
```

---

## 5. 작업 순서

### Phase A — Helper 도입 (코드 변경 최소, 빌드 통과)
1. `src/agent/turn_execution.rs` 에 `ClassifiedTools`, `PresetToolResult`, `DispatchedToolResult`, `classify_tool_calls`, `dispatch_tools_parallel` 추가. **호출하지 않음**, dead code 허용 (`#[allow(dead_code)]` 가능, 다음 phase 에서 사용).
2. `Agent` 구조체에 `write_serializer: Arc<tokio::sync::Mutex<()>>` 필드 추가. `Agent::new` 등 생성자에서 초기화.
3. `cargo +nightly build --release` 통과 확인.

### Phase B — broadcast turn 적용
4. `src/agent/turn_streaming_broadcast.rs:702` 의 `for tool_index in 0..tool_count` 블록을 §3.3 신규 구조로 교체.
5. 기존 INJECTION POINT C/D 로직 보존.
6. 빌드 + 기존 단위 테스트 통과 확인:
   ```bash
   cargo +nightly test --release --lib agent::
   ```

### Phase C — mpsc 와 loops mirror 적용
7. `src/agent/turn_streaming_mpsc.rs:784` 동일하게 교체.
8. `src/agent/turn_loops.rs:1188` 동일하게 교체. `print_output` 분기는 결과 정렬 후 한꺼번에 print.

### Phase D — 회귀 테스트 추가
9. 새 통합 테스트: `tests/m22_stage2_parallel_tools.rs`
   - **T1**: mock provider 가 한 turn 에 read tool 2개 emit → 두 tool 이 ~동시 실행 (timing 측정, 합산 시간이 max(t1,t2)*1.5 이내).
   - **T2**: validation_error 가 섞인 경우 → preset 으로 처리, 나머지 정상 실행.
   - **T3**: urgent_interrupt 발생 → 시작 안 한 tool 은 skipped 결과, 시작한 tool 은 cancel 시도.
   - **T4**: write + read 동시 → write 가 직렬화 (write_serializer mutex 효과 확인 — race 없음).
   - **T5**: tool_use/tool_result 순서 — `messages` 끝 N 개가 ToolResult 이고 id 순서가 tool_use 인덱스 순.

10. 실 모델 canary (사용자 측, 명세서 §7).

### Phase E — Stage 1 의 OpenAI parallel emit 도 함께 검증
11. `tests/m22_stage1_openai_parallel.rs` 가 없으면 mock 추가 (선택).

---

## 6. 빌드 / 배포

### 6.1 빌드
```bash
cargo +nightly build --release
# 무경고 확인
cargo +nightly clippy --release --all-targets -- -D warnings
# 테스트
cargo +nightly test --release
```

### 6.2 배포 (검증 후)
```bash
TIP=$(git rev-parse --short=8 deploy/m9-m27-catchup)
install -m 0755 target/release/jcode \
  "$HOME/.jcode/builds/versions/lazydino-${TIP}/jcode"
ln -sfn "$HOME/.jcode/builds/versions/lazydino-${TIP}/jcode" \
  "$HOME/.jcode/builds/current/jcode"
ln -sfn "$HOME/.jcode/builds/versions/lazydino-${TIP}/jcode" \
  "$HOME/.jcode/builds/stable/jcode"
install -m 0755 target/release/jcode "$HOME/.local/bin/jcode"
```
**활성 jcode server kill 금지**. atomic mv + symlink + 사용자 본인이 TUI close+reopen.

### 6.3 fork push
```bash
./scripts/fork-push.sh deploy/m9-m27-catchup patch/m22-stage2-turn-loop-fanout
```

### 6.4 commit 분리
- `fix(m22): introduce ClassifiedTools + dispatch_tools_parallel helpers` (Phase A)
- `fix(m22): use FuturesUnordered in broadcast turn loop` (Phase B)
- `fix(m22): mirror parallel dispatch into mpsc and loops` (Phase C)
- `test(m22): parallel tool dispatch + ordering + interrupt` (Phase D)
- `docs(m22): mark Stage 2 done; update README/MEMORY` (마무리)

---

## 7. 사용자 검증 시나리오 (canary)

배포 후 사용자에게:

```
1) jcode TUI 재시작 (이전 빌드 강제 해제)
2) 새 세션, claude-opus 또는 gpt-5.5 모델
3) 프롬프트:
   "두 파일 README.md 와 Cargo.toml 의 첫 30 줄을 동시에 보여줘"
   → 모델이 read tool 두 번 한 turn 에 emit 하는지 관찰
4) 로그 확인:
   tail -f ~/.jcode/logs/jcode.log | grep "Tool starting\|Tool finished"
   - 두 "Tool starting: read" 가 1초 이내 인접
   - "Tool finished: read" 두 줄이 거의 동시
5) subagent 2개 시나리오:
   "general subagent 두 개에게 각각 다른 디렉터리 ls 시켜줘"
   - "Tool starting: subagent" 두 줄이 ~동시
   - 둘 다 진행 (한 쪽 silence 없음)
6) ESC 중간:
   subagent 2개 도중 ESC → 둘 다 cancel 또는 background.
   "ESC 후 한 쪽이 영원히 silent" 가 사라졌는지 확인
7) write 동시 시나리오 (옵션):
   "/tmp/a.txt 와 /tmp/b.txt 에 각각 hello 써줘"
   - 두 write 가 차례로 (직렬) 진행되지만 race 없음
   - 같은 path 동시 write 시도 시도 회귀 없음
```

보고서 양식:

```markdown
## M22 Stage 2 canary report

- 빌드 tip: lazydino-<sha8>
- 모델: <claude-opus / gpt-5.5 / 등>
- 시나리오 3: <PASS/FAIL> 로그 snippet
- 시나리오 5: <PASS/FAIL> 로그 snippet
- 시나리오 6: <PASS/FAIL>
- 시나리오 7: <PASS/FAIL>
- 회귀 의심: <있다면 description>
- 기타 관찰: <free text>
```

---

## 8. 회귀 위험 체크리스트

- [ ] I1 ordering: 통합 테스트 T5 + 실측 로그 message 시퀀스 검증
- [ ] I2 validation error: T2
- [ ] I3 urgent interrupt: T3 + canary 6
- [ ] I4 sdk pre-provided: 기존 native tool 경로 수동 확인 (Anthropic SDK 가 read/write 등 emit 했을 때)
- [ ] I5 save 빈도: tool count 가 0 이면 save 안 함 확인
- [ ] I6 generated_image_contexts: tool 이 image 반환하는 경로 (multimodal-looker 등) 수동 확인
- [ ] I7 INJECTION POINT C/D: T3 + INJECTION POINT D 가 tool 결과 추가 후 1회 실행
- [ ] I8 unlock_tools_if_needed: tool lock 후 다음 turn 사용 가능 확인
- [ ] I9 telemetry count: T1 후 record_tool_call 호출 횟수가 to_execute.len()
- [ ] I10 ToolStarted 인덱스 순: per_tool_start 호출 순서 로그
- [ ] I11 path write race: T4
- [ ] I12 빌드 무경고
- [ ] I13 mermaid count (전체 grep 비교)

---

## 9. 명세서 외 참고 파일 (수정 안 함)

- `src/tool/batch.rs` — reference 패턴 (수정 X, 그대로 참고)
- `src/config/default_file.rs:182` — Stage 1 의 `openai_parallel_tool_calls` (수정 X)
- `src/provider/openai.rs:712` — Stage 1 (수정 X)
- `src/agent/interrupts.rs` — `has_urgent_interrupt`, `inject_soft_interrupts` (수정 X, 호출만)

---

## 10. 마무리

- 작업 완료 후 `docs/lazydino/milestones/M22.md` 의 "Stage 2 — Turn loop FuturesUnordered (남음)" 섹션을 ✅ DONE 으로 갱신.
- `docs/lazydino/milestones/README.md`, `docs/lazydino/MEMORY.md` mirror 갱신.
- 별개 카드 후보 M22-3 (path-aware write mutex) 는 옵션 A 로 충분하면 닫음.

**예상 코드 변경량**: ~250-350 줄 (helper 100 + 3 mirror 각 80-100). 테스트 ~150 줄.
**예상 작업 시간**: 4-6 시간 (테스트 포함).
