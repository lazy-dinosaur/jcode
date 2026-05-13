# M22 Stage 2.1 — mpsc Alt+B / Reload Handoff 복원 + loops mirror 검수 + 테스트 환경성 분류

**대상 브랜치**: `patch/m22-stage2-turn-loop-fanout` (기존 작업 위에 append commit)
**베이스**: 현재 tip `0fe7462f` (4 commits already there)
**author**: `lazydino <lazydino@users.noreply.github.com>`
**언어**: 한국어 응답, 영문 코드/commit body.

---

## 0. 배경 — 1차 명세서의 결함

`M22-stage2-spec.md` 는 broadcast turn loop 기준으로 작성되어 mpsc 만의 special path 를 **invariant 로 명시 안 했음**. 1차 작업자가 정직하게 "단순화" 라고 보고했으나, 실제로는 사용자 기능 회귀.

회귀 대상 (mpsc 전용 기능, **origin/master 의 `src/agent/turn_streaming_mpsc.rs:830-1023` 에 있던 코드**):

### F1 — Alt+B background handoff
사용자가 long-running tool 실행 중 Alt+B 누르면 `background_tool_signal.notified()` 가 발화 → tool handle 을 `crate::background::global().adopt(...)` 에 넘기고 `bg` tool 로 회수 가능. 1차 작업 후 **사라짐**.

### F2 — Graceful reload bash handoff (750ms grace)
`self.graceful_shutdown.notified()` 가 발화 + tool 이름이 `bash` 면 `tokio::time::timeout(750ms, &mut tool_handle)` 로 추가 대기 후 detach. 1차 작업 후 **사라짐**.

### F3 — Reload 시 selfdev 친화적 메시지 + 나머지 tool skip
`is_graceful_shutdown()` 이고 위 timeout 도 안 풀리면 abort + selfdev 케이스는 깨끗한 메시지 (`"Reload initiated. Process restarting..."`), 그 외엔 `"[Tool 'X' interrupted by server reload after N.Ns]"`. 그리고 `tool_calls[tool_index+1..]` 전부 `[Skipped - server reloading]` 처리 후 `return Ok(())`. 1차 작업 후 **사라짐**.

---

## 1. 추가 Invariants (명세서 보완)

| ID | Invariant | 적용 범위 |
|---|---|---|
| **I14** | mpsc 의 Alt+B handoff (F1) 보존: `background_tool_signal.notified()` 시 실행 중 tool 을 `background::global().adopt()` 로 넘기고 tool_result 에는 task_id + bg tool 사용 안내. | mpsc |
| **I15** | mpsc 의 graceful reload handoff (F2/F3) 보존: shutdown 시 bash 만 750ms grace, 그 외 즉시 abort, selfdev 친화 메시지, 나머지 tool skip, `return Ok(())` | mpsc |
| **I16** | `tools_remaining` 카운트 복원: urgent_interrupted 시 사용자 텍스트에 "{N} remaining tool(s) skipped" 형태 정확히. | broadcast + mpsc |
| **I17** | loops mirror 의 `BusEvent::SubagentStatus(running ...)`, `print_output` ordering 동일 의미 보존. | loops |

---

## 2. 권장 접근 — 옵션 A-2 (분기)

### 2.1 핵심 아이디어
mpsc 에서 `to_execute.len() == 1` 일 때는 **기존 spawn + select 경로 유지**, `>= 2` 일 때만 `dispatch_tools_parallel` 사용.

근거:
- 모델 emit 실측 (사용자 자연 실험 로그): **97% 이상 한 turn 에 tool 1개**.
- N=1 케이스는 parallel 이득 0, Alt+B/reload UX 손실��� 발생.
- 코드 중복 ~80줄 발생하지만 회귀 위험 0.
- 추후 dispatch_tools_parallel 자체에 bg/shutdown 신호를 통합하면 통일 가능 (별개 카드 M22-5).

### 2.2 의사코드 (mpsc 만)

```rust
// 기존 위치: src/agent/turn_streaming_mpsc.rs 의 tool 실행 블록
let mut tool_results_dirty = false;
let classified = self.classify_tool_calls(&tool_calls, &sdk_tool_results)?;
let mut preset_results: HashMap<usize, PresetToolResult> =
    classified.presets.into_iter().collect();
let to_execute = classified.to_execute;
let mut dispatched_results: HashMap<usize, DispatchedToolResult> = HashMap::new();
let mut urgent_interrupted = false;
let mut early_return_for_reload = false;  // F3

if self.has_urgent_interrupt() && !to_execute.is_empty() {
    crate::telemetry::record_user_cancelled();
    urgent_interrupted = true;
    for (index, tc) in to_execute {
        dispatched_results.insert(index, DispatchedToolResult {
            index, tc, started_at: Instant::now(),
            elapsed: Duration::ZERO,
            result: Err(anyhow::anyhow!("[Skipped: user interrupted]")),
        });
    }
} else if to_execute.len() == 1 {
    // === N=1: 기존 spawn + select 경로 복원 (F1/F2/F3) ===
    let (index, tc) = to_execute.into_iter().next().unwrap();
    let message_id = assistant_message_id.clone()
        .unwrap_or_else(|| self.session.id.clone());
    let ctx = ToolContext { /* 기존 그대로 */ };

    if trace { eprintln!("[trace] tool_exec_start name={} id={}", tc.name, tc.id); }
    logging::info(&format!("Tool starting: {}", tc.name));
    let tool_start = Instant::now();

    let registry_clone = self.registry.clone();
    let tool_name_for_spawn = tc.name.clone();
    let tool_input_for_spawn = tc.input.clone();
    let tool_handle = tokio::spawn(async move {
        registry_clone.execute(&tool_name_for_spawn, tool_input_for_spawn, ctx).await
    });

    self.background_tool_signal.reset();
    let bg_signal = self.background_tool_signal.clone();
    let shutdown_signal = self.graceful_shutdown.clone();
    let allow_reload_handoff = tc.name == "bash";
    let tool_result: Option<Result<ToolOutput>>;
    let mut tool_handle = tool_handle;
    tokio::select! {
        biased;
        res = &mut tool_handle => {
            tool_result = Some(match res {
                Ok(r) => r,
                Err(e) => Err(anyhow::anyhow!("Tool task panicked: {}", e)),
            });
        }
        _ = async {
            tokio::select! {
                _ = bg_signal.notified() => {}
                _ = shutdown_signal.notified() => {}
            }
        } => {
            if self.is_graceful_shutdown() && allow_reload_handoff {
                tool_result = match tokio::time::timeout(
                    Duration::from_millis(750), &mut tool_handle,
                ).await {
                    Ok(res) => Some(match res {
                        Ok(r) => r,
                        Err(e) => Err(anyhow::anyhow!("Tool task panicked: {}", e)),
                    }),
                    Err(_) => None,
                };
            } else {
                tool_result = None;
            }
        }
    }

    let tool_elapsed = tool_start.elapsed();
    crate::telemetry::record_tool_call();
    self.unlock_tools_if_needed(&tc.name);

    if let Some(result) = tool_result {
        // 정상 완료 — DispatchedToolResult 로 변환해 통합 처리
        dispatched_results.insert(index, DispatchedToolResult {
            index, tc, started_at: tool_start, elapsed: tool_elapsed, result,
        });
    } else if self.is_graceful_shutdown() {
        // F3: server reload 분기 — abort + selfdev 메시지 + 나머지 skip + return
        // (origin/master 의 logic 그대로 복사)
        tool_handle.abort();
        let is_selfdev_reload = tc.name == "selfdev";
        let interrupted_msg = if is_selfdev_reload {
            "Reload initiated. Process restarting...".to_string()
        } else {
            format!("[Tool '{}' interrupted by server reload after {:.1}s]",
                tc.name, tool_elapsed.as_secs_f64())
        };
        let _ = event_tx.send(ServerEvent::ToolDone {
            id: tc.id.clone(), name: tc.name.clone(),
            output: interrupted_msg.clone(),
            error: if is_selfdev_reload { None } else { Some("interrupted by reload".to_string()) },
        });
        self.add_message_with_duration(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: tc.id.clone(),
                content: interrupted_msg,
                is_error: Some(!is_selfdev_reload),
            }],
            Some(tool_elapsed.as_millis() as u64),
        );
        self.session.save()?;
        // 나머지 preset 들도 skip (실제로는 single tool 케이스라 거의 없지만 안전)
        for (i, tc) in tool_calls.iter().enumerate() {
            if i == index || preset_results.contains_key(&i) {
                continue;
            }
            self.add_message(Role::User, vec![ContentBlock::ToolResult {
                tool_use_id: tc.id.clone(),
                content: "[Skipped - server reloading]".to_string(),
                is_error: Some(true),
            }]);
        }
        self.session.save()?;
        return Ok(());
    } else {
        // F1: Alt+B background 분기 — adopt + bg 안내 메시지
        logging::info(&format!("Tool '{}' moved to background after {:.1}s",
            tc.name, tool_elapsed.as_secs_f64()));
        let bg_info = crate::background::global()
            .adopt(&tc.name, &self.session.id, tool_handle).await;
        let bg_msg = format!(
            "Tool '{}' was moved to background by the user (task_id: {}). \
             Use the `bg` tool with action 'wait' to wait for completion/checkpoints, \
             or action 'status'/'output' to inspect it.",
            tc.name, bg_info.task_id
        );
        let _ = event_tx.send(ServerEvent::ToolDone {
            id: tc.id.clone(), name: tc.name.clone(),
            output: bg_msg.clone(), error: None,
        });
        self.add_message_with_duration(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: tc.id.clone(),
                content: bg_msg,
                is_error: None,
            }],
            Some(tool_elapsed.as_millis() as u64),
        );
        tool_results_dirty = true;
        self.background_tool_signal.reset();
    }
} else if !to_execute.is_empty() {
    // === N>=2: parallel fan-out (기존 1차 작업의 dispatch_tools_parallel) ===
    // 단, urgent_interrupted 시 tools_remaining 카운트 정확히 (I16)
    let message_id = assistant_message_id.clone()
        .unwrap_or_else(|| self.session.id.clone());
    let cancel_token = tokio_util::sync::CancellationToken::new();
    let ctx_factory = |tc: &ToolCall| ToolContext { /* 기존 그대로 */ };
    let per_tool_start = |tc: &ToolCall| {
        if trace { eprintln!("[trace] tool_exec_start name={} id={}", tc.name, tc.id); }
        logging::info(&format!("Tool starting: {}", tc.name));
    };
    self.background_tool_signal.reset();
    let results = self.dispatch_tools_parallel(
        to_execute, ctx_factory, per_tool_start, &cancel_token,
    ).await;
    urgent_interrupted = self.has_urgent_interrupt();
    if urgent_interrupted { crate::telemetry::record_user_cancelled(); }
    dispatched_results = results.into_iter().map(|r| (r.index, r)).collect();
}

// 이하 main loop (preset + dispatched 통합 처리) 는 1차 작업과 동일.
// 단 urgent_interrupted 메시지에서 tools_remaining 복원 (I16):
if urgent_interrupted {
    let tools_remaining = dispatched_results.values()
        .filter(|r| r.result.is_err()  /* skipped/cancelled */)
        .count();
    let injected = self.inject_soft_interrupts();
    if !injected.is_empty() {
        for event in Self::build_soft_interrupt_events(injected, "C", Some(tools_remaining)) {
            let _ = event_tx.send(event);
        }
        self.add_message(Role::User, vec![ContentBlock::Text {
            text: format!("[User interrupted: {} remaining tool(s) skipped]", tools_remaining),
            cache_control: None,
        }]);
    }
    self.persist_session_best_effort("streamed tool output");
}
```

### 2.3 broadcast 도 동일하게 I16 (tools_remaining) 복원
broadcast 는 F1/F2/F3 같은 special path 가 **없음** (확인 완료 — `git grep background_tool_signal src/agent/turn_streaming_broadcast.rs` 0건). 따라서 broadcast 는 **dispatch_tools_parallel 유지 + tools_remaining 카운트만 복원** 하면 됨.

```rust
// broadcast 도 urgent_interrupted 블록만 갱신:
if urgent_interrupted {
    let tools_remaining = dispatched_results.values()
        .filter(|r| r.result.is_err()).count();
    // ... build_soft_interrupt_events(..., Some(tools_remaining))
    // ... format!("... {} remaining tool(s) skipped", tools_remaining)
}
```

---

## 3. loops mirror 검수 (필수 추가 작업)

### 3.1 확인 사항
`git diff origin/master..HEAD -- src/agent/turn_loops.rs` 를 보고 다음 보존 확인:

1. **`Bus::global().publish(BusEvent::SubagentStatus { ... })`** — turn_loops 만의 status 발행
2. **`Bus::global().publish(BusEvent::ToolUpdated(ToolEvent { ... Running/Completed ... }))`** — UI 업데이트
3. **`print_output` 분기** (CLI 경로) — 결과 출력 순서가 인덱스 순인지

### 3.2 보존되어야 할 코드 위치 (origin/master 기준)
- `src/agent/turn_loops.rs:1168-1188` — ToolUpdated(Running) + SubagentStatus(running ...)
- `src/agent/turn_loops.rs:1216-1239` — ToolUpdated(Completed) + print_output preview
- `src/agent/turn_loops.rs:1140-1155` — `print!("\n  → ")` 등 CLI prefix

### 3.3 정책
- loops 는 Alt+B / reload handoff 가 **없음** (broadcast 와 동일). F1/F2/F3 무관.
- 단 SubagentStatus / ToolUpdated / print_output 가 사라졌으면 복원.
- `to_execute.len() == 1` 분기는 불필요 (Alt+B 없음). 그냥 `dispatch_tools_parallel` 사용해도 됨.

---

## 4. 테스트 환경성 분류 (조치 D)

### 4.1 작업
1차 보고서: `cargo +nightly test --release` 가 다수 실패하지만 "browser/auth/provider/openrouter/session/swarm 관련 환경성".

이걸 검증:

```bash
# baseline (origin/master) 에서 같은 테스트 돌려서 비교
git stash  # 또는 worktree 사용
git checkout origin/master
cargo +nightly test --release 2>&1 | tee /tmp/baseline-test.log
git checkout patch/m22-stage2-turn-loop-fanout
cargo +nightly test --release 2>&1 | tee /tmp/m22-test.log
# 실패 테스트 비교
diff <(grep -E "^test .* FAILED" /tmp/baseline-test.log | sort -u) \
     <(grep -E "^test .* FAILED" /tmp/m22-test.log | sort -u)
```

### 4.2 판정
- diff 가 **0** 이면: 환경성 확정 (M22 영향 없음). 보고서에 OK 표기.
- diff 에 새 실패가 있으면: **M22 영향**, 각 테스트 root cause 분석 + fix.

---

## 5. 새 통합 테스트 추가

### 5.1 T6 — mpsc Alt+B handoff (회귀 방지)

`tests/m22_stage2_parallel_tools.rs` 에 추가:

```rust
#[tokio::test]
async fn mpsc_single_tool_alt_b_moves_to_background() {
    // mock long-running tool (e.g. bash sleep 10) 1개만 emit 하는 provider
    // turn 시작 후 100ms 뒤 background_tool_signal.notify_one()
    // 검증:
    //   - background::global() 에 task_id 등록됨
    //   - tool_result content 가 "moved to background" 포함
    //   - turn 이 즉시 return (10초 대기 안 함)
}
```

### 5.2 T7 — mpsc graceful reload bash handoff

```rust
#[tokio::test]
async fn mpsc_single_tool_reload_bash_handoff_750ms() {
    // bash tool 1개 emit, 100ms 후 graceful_shutdown.notify_one()
    // 검증:
    //   - 750ms timeout 안에 끝나면 정상 결과
    //   - 안 끝나면 abort + "interrupted by reload" 메시지
}
```

### 5.3 T8 — mpsc selfdev reload 친화 메시지

```rust
#[tokio::test]
async fn mpsc_single_tool_reload_selfdev_clean_message() {
    // selfdev tool 1개 emit, graceful_shutdown 발화
    // 검증: tool_result is_error=false, content="Reload initiated. Process restarting..."
}
```

### 5.4 T9 — N>=2 + tools_remaining 카운트 (I16)

```rust
#[tokio::test]
async fn parallel_urgent_interrupt_message_has_remaining_count() {
    // tool 3개 emit, dispatch 도중 urgent_interrupt
    // 검증: 마지막 Text message 가 "{N} remaining tool(s) skipped" 포함, N>0
}
```

T6/T7/T8 은 mpsc 한정. T9 은 broadcast + mpsc 둘 다.

---

## 6. 작업 순서

### Phase F — broadcast tools_remaining 복원 (I16)
1. `src/agent/turn_streaming_broadcast.rs` 의 urgent_interrupted 블록만 갱신.
2. 빌드 + clippy.

### Phase G — mpsc N=1 special path 복원 (I14/I15/I16)
3. `src/agent/turn_streaming_mpsc.rs` 의 dispatch 블록을 §2.2 의사코드대로 분기.
4. 빌드 + clippy.

### Phase H — loops mirror 검수 + 필요시 복원 (I17)
5. `git show origin/master:src/agent/turn_loops.rs` vs `git show HEAD:src/agent/turn_loops.rs` diff 정독.
6. SubagentStatus/ToolUpdated/print_output 보존 확인. 없으면 복원.

### Phase I — 추가 테스트 (T6/T7/T8/T9)
7. `tests/m22_stage2_parallel_tools.rs` 에 T6-T9 추가.
8. `cargo +nightly test --release --test m22_stage2_parallel_tools` PASS.

### Phase J — 환경성 테스트 분류
9. §4 의 baseline vs HEAD 비교 스크립트 실행.
10. 결과를 보고서에 첨부.

### Phase K — commit 분리 + push
11. commit:
    - `fix(m22): restore tools_remaining count in urgent interrupt`
    - `fix(m22): restore mpsc Alt+B + reload handoff via N=1 branch`
    - `fix(m22): preserve loops mirror SubagentStatus/ToolUpdated` (필요 시)
    - `test(m22): cover Alt+B, reload handoff, remaining count`
    - `docs(m22): note environment-flake test classification` (선택)
12. push: `git push -u origin patch/m22-stage2-turn-loop-fanout`

---

## 7. 보고서 양식 (이걸로 응답)

```markdown
## M22 Stage 2.1 implementation report

### 브랜치/커밋
- branch: patch/m22-stage2-turn-loop-fanout
- 추가 commits:
  - <sha> fix(m22): tools_remaining count
  - <sha> fix(m22): mpsc N=1 special path
  - <sha> fix(m22): loops mirror restore (했다면)
  - <sha> test(m22): T6-T9
- diff stat: `git diff --stat 0fe7462f..HEAD`

### Invariants 추가 (I14-I17)
- I14 Alt+B handoff: PASS/FAIL + 근거 (T6 결과)
- I15 reload handoff: PASS/FAIL + 근거 (T7/T8)
- I16 tools_remaining: PASS/FAIL + 근거 (T9, broadcast + mpsc)
- I17 loops SubagentStatus 등: PASS/FAIL + 근거 (코드 location)

### 테스트
- T6 (Alt+B): PASS/FAIL
- T7 (reload bash 750ms): PASS/FAIL
- T8 (selfdev clean message): PASS/FAIL
- T9 (tools_remaining count): PASS/FAIL
- 기존 T1-T5 재PASS: yes/no

### 환경성 테스트 분류 (§4)
- baseline 실패 N개, HEAD 실패 M개
- diff: 새 실패 X개 (있다면 list + 진단)

### 설계 변경 / 명세서와의 차이
- ...

### push 여부
- yes/no
- branch: <name>
- 마지막 sha: <sha>
```

---

## 8. 주의 사항

- **N=1 special path 는 origin/master 의 `src/agent/turn_streaming_mpsc.rs:830-1023` 코드를 거의 그대로 복사**. 새로 짜지 말 것.
- broadcast 에는 F1/F2/F3 없음 — 추가하지 말 것. dispatch_tools_parallel 유지.
- `dispatch_tools_parallel` 자체에 bg/shutdown 신호 통합하지 말 것 — 별개 카드 M22-5 후보, 지금은 분기로 우회.
- 빌드: `cargo +nightly build --release` + `cargo +nightly clippy --release --all-targets -- -D warnings`
- 활성 jcode server kill 금지. 배포는 lazydino 측에서.
- author: `lazydino <lazydino@users.noreply.github.com>` 강제.
- 한국어 응답, 영문 코드/commit body.
- **추측 금지**, 실측 우선. 모호한 곳은 보고서 "설계 변경" 섹션에 명시.

---

## 9. 참고 파일

- `docs/lazydino/milestones/M22-stage2-spec.md` (1차 명세서)
- `docs/lazydino/milestones/M22-stage2-review.md` (검수 보고서, 회귀 발견)
- `src/agent/turn_streaming_mpsc.rs` (origin/master 원본 — Alt+B/reload 로직 참조)
- `src/tool/batch.rs` (FuturesUnordered reference, 수정 안 함)

**예상 작업**: ~150 LOC + 테스트 ~120 LOC, 2-3 시간.
