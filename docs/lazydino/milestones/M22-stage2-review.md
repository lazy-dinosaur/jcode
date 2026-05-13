# M22 Stage 2 검수 보고서

**검수자**: 메인 세션 (lazydino orchestrator)
**대상**: `patch/m22-stage2-turn-loop-fanout` (tip `0fe7462f`)
**일자**: 2026-05-13
**baseline**: `origin/master`

---

## TL;DR

**Phase A/B (helpers + broadcast)**: ✅ 명세서대로, 안전.
**Phase C (mpsc mirror)**: 🔴 **regression** 1건 — Alt+B background handoff + graceful reload bash handoff 가 통째로 제거됨. 명세서가 mpsc-only 특수 경로를 캡쳐하지 못해 작업자가 "단순화" 로 처리.
**Phase C (loops mirror)**: 검수 필요.
**Phase D (tests)**: ✅ 5개 T1-T5 PASS, M22 격리 정확.

**조치**: 머지 보류. **mpsc 의 Alt+B / reload handoff 복원 patch** 1건 추가 후 머지.

---

## 1. 변경 범위 (diff stat)

```
17 files changed, 817 insertions(+), 535 deletions(-)
```

| 분류 | 파일 | 평가 |
|---|---|---|
| 핵심 | `src/agent/turn_execution.rs` (+129) | ✅ 명세서 §3.1-3.2 그대로 |
| 핵심 | `src/agent/turn_streaming_broadcast.rs` (±213) | ✅ I7 의미 보존 (작은 차이 P3, 허용) |
| 핵심 | `src/agent/turn_streaming_mpsc.rs` (±378) | 🔴 **Alt+B 회귀** |
| 핵심 | `src/agent/turn_loops.rs` (±243) | ⚠️ 추가 확인 필요 |
| 인프라 | `src/agent.rs` (+3), `Cargo.toml` (+1) | ✅ `write_serializer`, `tokio-util` 추가 |
| 테스트 | `tests/m22_stage2_parallel_tools.rs` (+298) | ✅ T1-T5 PASS |
| 부수 | 기타 7개 파일 clippy 자동 fix | ✅ 의미 보존 |

---

## 2. Invariants 재검증

| ID | 보고서 | 검수 결과 | 비고 |
|---|---|---|---|
| I1 ordering | PASS | ✅ PASS | `out.sort_by_key(|r| r.index)` + main loop 의 `for tool_index in tool_calls.iter().enumerate()` |
| I2 validation error | PASS | ✅ PASS | `classify_tool_calls` 에서 분리 |
| I3 urgent interrupt | PARTIAL | ✅ PARTIAL (수용) | bash subprocess kill 불가 한계는 기존과 동일 |
| I4 sdk pre-provided | PASS | ✅ PASS | `PresetToolResult::SdkProvided`, native+error fall-through 보존 |
| I5 save 빈도 | PASS | ✅ PASS | `tool_results_dirty` 조건 유지 |
| I6 generated_image_contexts | PASS | ✅ PASS | drain 위치 유지 |
| I7 INJECTION POINT C/D | PARTIAL | ⚠️ **PARTIAL with semantic drift** | tools_remaining 카운트 사라짐 (§3 P3) |
| I8 unlock_tools_if_needed | PASS | ✅ PASS | dispatched result 처리 직후 호출 |
| I9 telemetry count | PASS | ✅ PASS | result 당 1회 |
| I10 ToolStarted index 순 | PASS | ✅ PASS | `per_tool_start` 가 future collect 시 동기 호출 |
| I11 path write race | PASS | ✅ PASS | global mutex 옵션 A, ~5줄 |
| I12 build/clippy | PASS | ✅ PASS | 양쪽 다 통과 |
| I13 mermaid count | PASS | ✅ PASS | 683 → 683 |

**추가 invariant 누락 발견 — 명세서 자체 결함**:
| Inew | 항목 | 결과 |
|---|---|---|
| **I14 (누락)** | mpsc 의 Alt+B background_tool_signal handoff 보존 | 🔴 **FAIL** |
| **I15 (누락)** | mpsc 의 graceful reload bash 750ms handoff 보존 | 🔴 **FAIL** |

---

## 3. 주요 우려 (P1-P4)

### P1 — Urgent interrupt 사용자 메시지 단순화
- 원본: `"[User interrupted: {N} remaining tool(s) skipped]"` (정확한 카운트)
- 신규: `"[User interrupted: pending tool(s) cancelled or skipped]"` (카운트 없음)
- 영향: 모델이 받는 텍스트가 덜 informative.
- 평가: 허용 가능. 카운트가 필요하면 후속.

### P2 — Cancel future 의 ToolDone 이벤트
- cancel 된 future 가 `Err("[Cancelled/Skipped: user interrupted]")` 로 반환.
- main loop 의 Err 분기에서 `ServerEvent::ToolDone { error: Some(...) }` send.
- per_tool_start 는 future 만들 때 동기 호출 → ToolStarted/ToolDone 1쌍 유지. ✅

### P3 — INJECTION POINT C 의미 미세 변경 (I7 PARTIAL)
- 원본: tool 사이에 한 번씩 urgent 체크 → 첫 tool 끝나고 두 번째 직전 ESC 받으면 두 번째 즉시 skip.
- 신규: dispatch 전체 한 번 + dispatch 안에서 cancel propagation.
- 부작용: ESC 가 dispatch 시작 직후 들어오면 모든 future 가 cancel propagation 받기 전에 시작될 수 있음 (특히 fast tool). 이건 **batch.rs 와 같은 동작** 이므로 사용자 기대 일치. ✅ 수용.

### P4 — 🔴 mpsc Alt+B / graceful reload handoff 제거 (회귀)

**증상**:
- 원본 mpsc `src/agent/turn_streaming_mpsc.rs:830-880` 에 다음 `tokio::select!` 분기:
  ```rust
  let tool_handle = tokio::spawn(async move { registry.execute(...).await });
  self.background_tool_signal.reset();
  let bg_signal = self.background_tool_signal.clone();
  let shutdown_signal = self.graceful_shutdown.clone();
  let allow_reload_handoff = tc.name == "bash";
  tokio::select! {
      biased;
      res = &mut tool_handle => { tool_result = Some(res...); }
      _ = async {
          tokio::select! {
              _ = bg_signal.notified() => {}
              _ = shutdown_signal.notified() => {}
          }
      } => {
          if self.is_graceful_shutdown() && allow_reload_handoff {
              // bash 만 750ms 추가 대기 후 detach
          } else {
              tool_result = None; // background 로 detach
          }
      }
  }
  ```
- 신규 mpsc `0fe7462f:src/agent/turn_streaming_mpsc.rs:764` 에는 `self.background_tool_signal.reset();` 한 줄만 남음. `bg_signal.notified()` select 분기 통째로 사라짐.

**영향**:
1. **Alt+B 가 동작 안 함** — 사용자가 long-running tool 을 background 로 보내려 해도 dispatch_tools_parallel 이 끝까지 await.
2. **graceful reload 시 bash 강제 종료** — server reload 가 750ms grace 없이 cancel.

**원인**:
- 명세서가 broadcast 기준이라 mpsc 의 special path 를 invariant 로 명시하지 않음.
- 보고서 "설계 변경 / mpsc 기존 Alt+B background handoff 와 graceful reload handoff 의 per-tool special path 는 parallel helper 경로로 단순화됨" 으로 작업자 자가 보고. 정직하지만 **영향이 명세서 invariant 에 안 잡혀 있어서** 위험 인식 약함.

**조치 제안**: §5 참고.

---

## 4. loops mirror 검수 (별도 확인)

`turn_loops.rs` 도 검사 필요:
- `BusEvent::SubagentStatus(running ...)` 등 special event 가 보존되는지
- `print_output` 결과 ordering 이 자연스러운지

→ 본 검수 보고서에서는 grep 정도만 했고, 회귀 가능성 가장 큰 P4 가 우선이라 P4 fix 후 별도 pass.

---

## 5. 머지 전 조치

### 조치 A (필수) — mpsc Alt+B / reload handoff 복원

옵션 A-1 (간단, 권장):
- mpsc 에서는 dispatch_tools_parallel 호출 후 future 들을 직접 spawn 하지 말고, **dispatch_tools_parallel 자체에 `background_signal: Option<&Notify>` 와 `shutdown_signal: Option<&Notify>` 콜백을 추가** 하여, cancel_token 외에도 두 신호로 select 가능하게 확장.
- 또는 mpsc 의 main loop 에서 `dispatch_tools_parallel` 호출을 `tokio::select!` 로 감싸고, `bg_signal.notified()` 시 cancel_token.cancel() + tool_result 들을 detach 표시.

옵션 A-2 (mpsc 만 분기):
- mpsc 는 단일 tool 케이스가 압도적으로 많으므로 (모델 emit 패턴), `to_execute.len() == 1` 일 때만 기존 spawn+select 경로 사용하고, `>= 2` 일 때 dispatch_tools_parallel 사용. 약간의 코드 중복이지만 회귀 risk 최소.

**권장**: A-2. M22 Stage 2 의 본질은 "N≥2 tool 일 때 병렬" 이고 N=1 의 Alt+B 케이스가 사용자 일상 사용 경로이므로 보수적 분기가 안전.

### ��치 B (선택) — I7 tools_remaining 카운트 복원
- urgent_interrupted 일 때 `to_execute.len()` 으로 카운트 복원. 1줄 변경.

### 조치 C (필수) — loops mirror full pass
- `turn_loops.rs` 의 `BusEvent::SubagentStatus`, `print_output` ordering 등 확인.

### 조치 D (확인) — full `cargo test --release` 실패 원인 분류
- 보고서 "browser/auth/provider/openrouter/session/swarm 관련 환경 의존 실패" 가 정말 환경성인지, 아니면 M22 ���경의 부작용인지 차분.
- 우선 `cargo test --release --lib` (lib only) 와 `--test m22_*` 분리해 보고 환경성 확정 필요.

---

## 6. 결론

- 핵심 설계 (helper + classify + dispatch + write_serializer + cancel_token) **명세서 §3 그대로 구현됨**. 품질 높음.
- broadcast/turn_loops mirror 는 거의 안전.
- **mpsc mirror 에서 Alt+B / reload handoff 회귀** — 머지 차단.
- M22 격리 테스트 5/5 PASS.

**다음 단계**: 작업자에게 조치 A (필수), B (선택), C (필수), D (확인) 요청하는 follow-up spec 작성 + 본인 또는 별도 세션에서 진행.
