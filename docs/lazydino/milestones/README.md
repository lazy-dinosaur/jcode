# Lazydino Milestone Cards — 다른 세션 픽업용

각 OPEN 마일스톤마다 **그 세션에서 그대로 작업을 시작**할 수 있는
self-contained handoff card. 다음 세션 시작 시 사용 패턴:

1. 사용자가 "M11 진행" 같이 요청
2. 에이전트가 `docs/lazydino/milestones/M11.md` 를 읽음
3. 거기에 적힌 worktree / branch / 분석 문서 / verification 으로 바로 진행

## 우선순위 (2026-05-13 M22 deploy integration 이후)

| 순위 | ID | 우선도 | 추정 작업량 | 카드 |
|---|---|---|---|---|
| 1 | M11 | ✅ DONE (framework 게이트) | 6 stages complete, latest Stage 6 deployed | [M11.md](./M11.md) |
| 2 | M41 | ✅ DONE (라이브 검증 + 배포 완료) | fix + 4 회귀 테스트, deploy `m41-eefa3744`, fork pushed | [M41.md](./M41.md) |
| 3 | M42 | ✅ DONE 2026-05-13 (deploy `lazydino-6d81399a`) | stale `checking websocket` label clear (StatusDetail empty-string contract) | [M42.md](./M42.md) |
| 4 | M40 | ✅ DONE 2026-05-13 | image paste live validation passed, leading-space slash fixed, Claude 1m picker advertise fixed, GPT-5.5 context split fixed | [M40.md](./M40.md) |
| 5 | M17 | **High** (사용자 워크플로우) | A vs B1 결정 + ~50줄 ~ 수백줄 | [M17.md](./M17.md) |
| 6 | M22 | ✅ DONE 2026-05-13 (deploy `lazydino-93e25dae`) | OpenAI parallel_tool_calls + turn loop FuturesUnordered fan-out, 9 targeted tests PASS, mpsc Alt+B/reload preserved, fork pushed | [M22.md](./M22.md) |
| 7 | M43 | ✅ DONE 2026-05-13 (deploy `lazydino-07905799`) | OAuth path 에서 `tools` 기반 광고 회복, bg/swarm canary 실측 통과 | [M43.md](./M43.md) |
| 8 | M44 | **High — NEXT** (MCP ecosystem/auth compatibility) | OAuth-required MCP for Figma/remote services: config foundation → authenticated HTTP/SSE/streamable transport → OAuth discovery/login | [M44.md](./M44.md) |
| ✅ | M45 | — | DONE 2026-05-13: private `.jcode/` instruction stack 강화 completed: visibility, custom globs, nested private rules with turn dedup | [M45.md](./M45.md) |
| 10 | M48 | **High - Planned** (long-context reliability) | Opencode-style durable compaction marker, anchored summary, token tail, pruning, overflow replay | [M48.md](./M48.md) |
| 11 | M49 | **High - Planned** (safe interrupt lifecycle) | Opencode-style cooperative cancel: typed turn stop reasons, provider/tool cancellation, interrupted transcript finalization | [M49.md](./M49.md) |
| 12 | M16 | Medium-High (구조 개선) | 4 sub-step | [M16.md](./M16.md) |
| 13 | M2  | Medium-High | 재현부터 | [M2.md](./M2.md) |
| 14 | M4  | Medium (BY-DESIGN) | UX caveat 정리 | [M4.md](./M4.md) |
| 15 | M15 | Low-Medium (debug UX) | 작은 fix | [M15.md](./M15.md) |
| 16 | M23 | Low (디스크 충분 동안) | tool 작성 | [M23.md](./M23.md) |

## 공통 환경 (모든 카드에 적용)

- **언어**: 한국어 대화
- **author**: `lazydino <lazydino@users.noreply.github.com>`
  ```bash
  git -c user.name=lazydino -c user.email=lazydino@users.noreply.github.com commit ...
  ```
- **commit 분리**: `fix(mXX):`, `test(mXX):`, `docs:` 각각 별도 commit
- **patch 베이스**: 항상 `origin/master` (deploy 위에 stack 안 함)
- **toolchain**: `cargo +nightly` 필수
- **integration**: 현재 catch-up deploy branch 는 `deploy/m9-m27-catchup`
- **빌드 + 배포**:
  ```bash
  cargo +nightly build --release
  TIP=$(git rev-parse --short=8 deploy/m9-m27-catchup)
  install -m 0755 target/release/jcode \
    "$HOME/.jcode/builds/versions/lazydino-${TIP}/jcode"
  ln -sfn "$HOME/.jcode/builds/versions/lazydino-${TIP}/jcode" \
    "$HOME/.jcode/builds/current/jcode"
  ln -sfn "$HOME/.jcode/builds/versions/lazydino-${TIP}/jcode" \
    "$HOME/.jcode/builds/stable/jcode"
  install -m 0755 target/release/jcode "$HOME/.local/bin/jcode"
  ```
- **fork push**: `./scripts/fork-push.sh deploy/m9-m27-catchup patch/<name>`
- **사용자 액션**: TUI close+reopen 은 *사용자 본인이* (절대 server kill 금지)
- **subagent**: M22 완료. 같은 turn 에 여러 tool/subagent 가 emit 되면 turn loop 가 `FuturesUnordered` 로 fan-out. 단 모델이 multi-emits 할지는 provider/model 결정.

## 우리 fork 상태 (참고)

- `origin = https://github.com/1jehuang/jcode.git` (upstream)
- `fork   = https://github.com/lazy-dinosaur/jcode.git` (개인)
- fork `deploy/m9-m27-catchup` contains M22 code/binary deploy commit `93e25dae`
  plus later docs-only handoff refresh commits
- fork `patch/m22-stage2-turn-loop-fanout` pushed at `b0a6b344`
- latest installed binary: `jcode v0.12.274-dev (93e25dae)` via current/stable symlinks
- backup tags retained for rollback safety

## 현재 handoff 요약 (2026-05-13)

- 방금 완료: **M22 Stage 2 same-round tool/subagent fan-out**.
- 검증 완료: `cargo +nightly test --release --test m22_stage2_parallel_tools`
  9/9 PASS, `cargo +nightly build --release` PASS, `git diff --check` PASS.
- 의도적으로 하지 않은 것: active server kill/restart. TUI 는 사용자가 close/reopen 해야
  새 binary 를 붙잡음.
- 방금 완료: **M40** — image paste live validation, leading-space slash, Claude 1m picker advertisement, GPT-5.5 context policy/UI.
- 다음 추천: **M44 — MCP OAuth/authenticated remote MCP support**.
  이유: Figma 등 OAuth-required remote MCP 를 붙이려면 HTTP/SSE/streamable transport, bearer token, OAuth discovery/login foundation 이 필요함.
