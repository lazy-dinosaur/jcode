# Lazydino Milestone Cards — 다른 세션 픽업용

각 OPEN 마일스톤마다 **그 세션에서 그대로 작업을 시작**할 수 있는
self-contained handoff card. 다음 세션 시작 시 사용 패턴:

1. 사용자가 "M11 진행" 같이 요청
2. 에이전트가 `docs/lazydino/milestones/M11.md` 를 읽음
3. 거기에 적힌 worktree / branch / 분석 문서 / verification 으로 바로 진행

## 우선순위 (Round 6 종료 시점)

| 순위 | ID | 우선도 | 추정 작업량 | 카드 |
|---|---|---|---|---|
| 1 | M11 | **High** (framework 게이트) | 5 stage × ~2-4h | [M11.md](./M11.md) |
| 2 | M17 | **High** (사용자 워크플로우) | A vs B1 결정 + ~50줄 ~ 수백줄 | [M17.md](./M17.md) |
| 3 | M16 | Medium-High (구조 개선) | 4 sub-step | [M16.md](./M16.md) |
| 4 | M2  | Medium-High | 재현부터 | [M2.md](./M2.md) |
| 5 | M4  | Medium (BY-DESIGN) | UX caveat 정리 | [M4.md](./M4.md) |
| 6 | M15 | Low-Medium (debug UX) | 작은 fix | [M15.md](./M15.md) |
| 7 | M22 | Low (workaround 있음) | 진단부터 | [M22.md](./M22.md) |
| 8 | M23 | Low (디스크 충분 동안) | tool 작성 | [M23.md](./M23.md) |

## 공통 환경 (모든 카드에 적용)

- **언어**: 한국어 대화
- **author**: `lazydino <lazydino@users.noreply.github.com>`
  ```bash
  git -c user.name=lazydino -c user.email=lazydino@users.noreply.github.com commit ...
  ```
- **commit 분리**: `fix(mXX):`, `test(mXX):`, `docs:` 각각 별도 commit
- **patch 베이스**: 항상 `origin/master` (deploy 위에 stack 안 함)
- **toolchain**: `cargo +nightly` 필수
- **integration**: deploy/m9-m10 으로 cherry-pick
- **빌드 + 배포**:
  ```bash
  cargo +nightly build --release
  TIP=$(git rev-parse --short=8 deploy/m9-m10)
  install -m 0755 target/release/jcode \
    "$HOME/.jcode/builds/versions/lazydino-${TIP}/jcode"
  ln -sfn "$HOME/.jcode/builds/versions/lazydino-${TIP}/jcode" \
    "$HOME/.jcode/builds/current/jcode"
  ln -sfn "$HOME/.jcode/builds/versions/lazydino-${TIP}/jcode" \
    "$HOME/.jcode/builds/stable/jcode"
  install -m 0755 target/release/jcode "$HOME/.local/bin/jcode"
  ```
- **fork push**: `./scripts/fork-push.sh deploy/m9-m10 patch/<name>`
- **사용자 액션**: TUI close+reopen 은 *사용자 본인이* (절대 server kill 금지)
- **subagent**: 한 round 에 1 개만 spawn (M22 회피)

## 우리 fork 상태 (참고)

- `origin = https://github.com/1jehuang/jcode.git` (upstream)
- `fork   = https://github.com/lazy-dinosaur/jcode.git` (개인)
- fork == origin/master sync 완료, deploy/m9-m10 + 48 patch 모두 push 됨
- backup tag 51개 (rollback 안전)
