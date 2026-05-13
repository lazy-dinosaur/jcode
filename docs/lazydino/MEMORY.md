# Lazydino agent memory — 항상 기억할 운영 원칙

> 다음 세션 시작 시 카드 들어가기 전에 이 파일 먼저 읽기.
> 짧고 절대 어기지 말 것.

## 절대 원칙

1. **활성 server kill 금지.** 배포는 atomic mv + symlink 갱신만.
2. **author**: `-c user.name=lazydino -c user.email=lazydino@users.noreply.github.com`
3. **branch**: `patch/<name>`, `deploy/<name>`. `fix:` / `test:` / `docs:` commit 분리.
4. **language**: 한국어 대화. 사용자가 잘못 본 거 정정해주면 즉시 사과하고 다시 확인.
5. **추측 금지**: 이전 스크린샷/세션 잔여 정보로 새 사실 추측하지 말 것.
6. **bg cargo**: `nohup ... > log 2>&1 & disown` + 폴링.
7. **subagent**: M22 완료. 같은 turn 에 여러 tool/subagent 가 emit 되면 `FuturesUnordered` fan-out. 단 multi-emit 여부는 모델/provider 결정.
8. **세션 기록**: 진행은 `docs/lazydino/sessions/YYYY-MM-DD-*.md`,
   재사용 가능한 큰 작업은 `docs/lazydino/milestones/Mxx.md`.

## 운영 중인 알려진 버그 (요약 — 자세한 건 카드)

- **M43 — subagent 에서 `bg`/`swarm` tool 광고 누락** (2026-05-13 fix)
  - ✅ DONE (2026-05-13). deploy `lazydino-07905799`.
  - Root cause: Anthropic OAuth `format_tools(..., is_oauth=true)` 가
    hardcoded 10개 Claude-Code tool 만 광고하고 `tools` 인자를 무시.
  - Fix: OAuth path 도 실제 allowed `tools` 를 순회한다. KNOWN OAuth
    tool 은 schema-only source 로 쓰고, jcode-only tool (`bg`, `swarm` 등)
    은 raw name + local schema 로 광고. `Agent` 는 local `subagent` 가
    allowed 일 때만 광고되어 recursion guard 누수도 회복.
  - Canary: isolated socket 에서 Claude `bg action=list`, `swarm action=list`
    tool_start/tool_exec 확인. server-side unknown-tool 4xx 없음.

- **M42 — `checking websocket` stale label** (2026-05-13 fix)
  - ✅ DONE (2026-05-13). deploy `lazydino-6d81399a`.
  - Root cause: `StatusDetail { detail: String }` 가 set-only, clear
    semantics 없음 → healthcheck 성공 후에도 `"checking websocket"` 이
    Thinking 동안 stale 하게 렌더. 실제 hang 아님 (bg tool 91s 실행
    중이었던 게 thinking 108.3s 와 일치).
  - Fix: 빈 string detail 을 explicit clear 로 contract 정립
    (provider/UI/agent 3 곳 동시에 mirror).

- **M22 — same-round 두 번째 subagent deferred** (2026-05-13 fix)
  - ✅ DONE. 1차 by-design 판정 취소 후 실제 fix 완료.
  - Stage 1: OpenAI Responses API `parallel_tool_calls` toggle default true.
  - Stage 2: turn loop `for tool_index ... await` 순차 실행을
    `FuturesUnordered` fan-out 으로 전환 (broadcast/mpsc/loops mirror).
  - 보존: Anthropic tool_result ordering, SDK-provided results, validation
    errors, urgent interrupt remaining count, global write_serializer,
    mpsc Alt+B background handoff, graceful reload bash 750ms handoff,
    selfdev reload clean message, loops SubagentStatus/ToolUpdated.
  - Deploy integration: `deploy/m9-m27-catchup` at `93e25dae`, fork pushed.
    Installed binary: `jcode v0.12.274-dev (93e25dae)` via current/stable symlinks.
  - 검증: `cargo +nightly test --release --test m22_stage2_parallel_tools`
    9/9 PASS, `cargo +nightly build --release` PASS, `git diff --check` PASS,
    mermaid string count unchanged.
  - 주의: `clippy --all-targets -D warnings` 는 deploy branch 의 M22 무관
    기존 lint 다수로 실패. M22 targeted/build 는 PASS.
- **M41 — server-initiated turn 첫 stream event 가 client redraw 안 깨움**
  - ✅ DONE (2026-05-12 라이브 검증). deploy `m41-eefa3744`.
  - 잔여 검증: thought-line + woke 조합 회귀 테스트, sibling fanout
    다른 attached client redraw, auto-poke 동일 경로 확인 (모두
    선택적, 메인 케이스는 OK).
- **M40 — image paste / leading-space slash / Claude 1m / GPT-5.5 context**
  - ✅ DONE (2026-05-13). image paste live screenshot 검증 통과: 외부/attached client 에 실제 pixel bytes 전달 확인.
  - Slash: leading whitespace before `/` 는 Chat 모드.
  - Claude 1m: Opus/Sonnet `[1m]` picker advertise + disabled hint 정책 정리.
  - GPT-5.5: API-key 1.05M, Codex/OAuth 400K, Fast/Priority opt-in.
- **NEXT: M44 — MCP OAuth/authenticated remote MCP support**
  - 목적: Figma 등 OAuth-required remote MCP 서버 연결.
  - 시작 파일: `docs/lazydino/milestones/M44.md`.
  - 우선순위: config/transport foundation → bearer/static token HTTP/SSE/streamable client → OAuth discovery/login.
- **M16 (구조 개선) 은 M44 이후**

## 배포 절차 요약

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
./scripts/fork-push.sh deploy/m9-m27-catchup patch/<name>
```

## 새 버그 발견 시

1. **재현 케이스 수집** (사용자 발언/스크린샷 직접 인용)
2. **카드 검색**: `grep -l <keyword> docs/lazydino/milestones/*.md`
3. **이미 카드 있으면**: priority 재평가 + "재현 history" 항목에 날짜+원문 추가
4. **없으면**: 새 Mxx 카드 작성 (자유 번호, 충돌 안 나게 README 표 확인)
5. **README 우선순위 표 갱신** 필수
