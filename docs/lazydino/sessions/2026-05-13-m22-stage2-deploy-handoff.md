# 2026-05-13 — M22 Stage 2 deploy handoff

## 현재 상태

- Working repo: `/home/lazydino/dev/jcode`
- Current branch: `deploy/m9-m27-catchup`
- Current deploy HEAD: `93e25dae docs(m22): mark Stage 2 fan-out fix done`
- Fork pushed:
  - `fork/deploy/m9-m27-catchup` → `93e25dae`
  - `fork/patch/m22-stage2-turn-loop-fanout` → `b0a6b344`
- Installed binary: `jcode v0.12.274-dev (93e25dae)`
  - `/home/lazydino/.jcode/builds/current/jcode` symlink
  - `/home/lazydino/.jcode/builds/stable/jcode` symlink
  - `/home/lazydino/.local/bin/jcode`
- Active server was **not killed**. User must close/reopen TUI to attach to new binary.

## 이번 세션에서 완료한 내용

### M22 — Subagent Same-Round Spawn Defer

M22 를 by-design 이 아니라 fixable implementation limitation 으로 재분류하고 완료했다.

원인 layer:
1. OpenAI provider emit layer: Responses API `parallel_tool_calls` flag 필요.
2. jcode turn loop dispatch layer: `for tool_index ... await` 로 local tool dispatch 가 순차 실행.
3. subagent/task tool semantics: sub turn 완료까지 await 하는 것은 의도된 동작.

적용된 fix:
- OpenAI `parallel_tool_calls` default true.
- `ClassifiedTools`, `PresetToolResult`, `DispatchedToolResult`, `dispatch_tools_parallel` helper 도입.
- broadcast/mpsc/loops turn loop 를 `FuturesUnordered` fan-out 으로 전환.
- tool result append 는 원래 tool_use index 순서로 정렬하여 Anthropic ordering 보존.
- SDK-provided result / validation error 는 preset 으로 처리.
- urgent interrupt 에서 `tools_remaining` count 복원.
- write/edit/apply_patch/multiedit 는 global write serializer 로 직렬화 유지.

Stage 2.1 에서 복원한 회귀:
- mpsc Alt+B background handoff.
- mpsc graceful reload bash 750ms handoff.
- selfdev reload clean message.
- loops SubagentStatus/ToolUpdated/print_output mirror.

## 검증 로그 요약

통과:
- `cargo +nightly test --release --test m22_stage2_parallel_tools`
  - 9/9 PASS
  - coverage: parallel timing, validation preset, urgent interrupt skip, write race guard,
    result ordering, Alt+B handoff, reload handoff, selfdev reload message,
    `tools_remaining` for mpsc+broadcast.
- `cargo +nightly build --release` PASS.
- `git diff --check` PASS.
- Mermaid string count unchanged.
- `"Mermaid rendering is disabled"` disabled-string baseline unchanged.

주의:
- `cargo +nightly clippy --release --all-targets -- -D warnings` 는 M22 무관 기존 deploy-branch lint 다수로 실패.
- 잠깐 unrelated lint 2개를 고쳤다가 M22 scope clean 유지를 위해 revert 했다.

## 문서 최신화

갱신된 문서:
- `docs/lazydino/milestones/M22.md`
- `docs/lazydino/milestones/README.md`
- `docs/lazydino/MEMORY.md`
- `docs/lazydino/sessions/2026-05-13-m22-stage2-deploy-handoff.md` (this file)

관련 spec/review:
- `docs/lazydino/milestones/M22-stage2-spec.md`
- `docs/lazydino/milestones/M22-stage2-review.md`
- `docs/lazydino/milestones/M22-stage2.1-spec.md`

## 다음 추천 작업

### NEXT: M40 Phase 4 — Opus 1m main picker advertisement

이유:
- 메인 세션 context 한계에 직접 영향.
- `[1m]` 모델은 코드/카탈로그에 이미 등록되어 있음.
- subagent `variant=max` 는 이미 `[1m]` 변환으로 동작 중.
- 남은 scope 는 main picker advertisement / usage gate 쪽으로 비교적 선명함.

시작 지점:
- `docs/lazydino/milestones/M40.md`
- 특히 Candidate D / Phase 4.

확인할 파일:
- `src/provider/anthropic.rs`
- `src/provider/routing.rs`
- `src/usage/accessors.rs`
- `src/tool/task.rs`
- model picker path: `src/tui/inline_interactive.rs` 및 anthropic model list hooks.

권장 순서:
1. 실제 코드로 main picker advertisement path 와 `has_extra_usage()` gate 를 재확인.
2. 현재 동작을 테스트로 고정: unknown/false extra usage 일 때 `[1m]` 이 숨겨지는지.
3. 정책 선택: 숨김 대신 disabled/hint 표시 또는 명시 선택 시 attempt 허용.
4. fix/test/docs commit 분리.
5. deploy branch 에 cherry-pick, targeted test + build, fork push.

### 그 다음: M40 Phase 1-2 — image attach silent fail

- 먼저 `clipboard_image()` 에 단계별 INFO/WARN 로그와 사용자-visible 실패/성공 피드백 추가.
- live paste 로 실제 실패 지점 확인 후 Phase 2 root cause fix.

## 운영 주의사항

- 절대 active server kill 금지.
- deploy 는 atomic version dir + symlink 갱신만.
- patch branch 는 `origin/master` 기준.
- commit author: `lazydino <lazydino@users.noreply.github.com>`.
- 한국어 응답 유지.
