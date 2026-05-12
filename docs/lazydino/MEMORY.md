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
7. **subagent**: round 당 1 spawn 만 (M22 회피).
8. **세션 기록**: 진행은 `docs/lazydino/sessions/YYYY-MM-DD-*.md`,
   재사용 가능한 큰 작업은 `docs/lazydino/milestones/Mxx.md`.

## 운영 중인 알려진 버그 (요약 — 자세한 건 카드)

- **M43 — subagent 에서 `bg` 도구 접근 불일치** (2026-05-13 00:14 라이브)
  - subagent 에 위임한 `bg action="output"` 이 "도구 없음" 으로 실패.
  - 진단 1차 완료 (2026-05-13 07:30): **가설 A 확정**. quick subagent
    (Haiku, OAuth) dump 결과 `bg` 가 LLM 한테 안 보임. 노출된 이름은
    PascalCase Claude-Code 스타일 (`Bash`, `Read`, `Edit`, ...) →
    OAuth tool advertisement 단에서 Claude-Code alias map 못 매기는
    jcode-only tool 들 (`bg`, `bash_output`, `bash_kill` 등) 이
    광고 자체에서 drop 되는 게 가장 유력. 추가 발견: `Agent`
    (subagent alias) 가 노출됨 → `task.rs:431` recursion 차단이
    내부 이름 기준이라 alias path 로 새는 중.
  - Fix 미착수. 별도 swarm worker 에 위임 예정.
  - **workaround**: bg/output/wait 같은 task lookup 류는 메인 세션에서
    직접 호출. subagent 위임 금지.

- **M42 — `checking websocket` stale label** (2026-05-13 fix)
  - ✅ DONE (2026-05-13). deploy `lazydino-6d81399a`.
  - Root cause: `StatusDetail { detail: String }` 가 set-only, clear
    semantics 없음 → healthcheck 성공 후에도 `"checking websocket"` 이
    Thinking 동안 stale 하게 렌더. 실제 hang 아님 (bg tool 91s 실행
    중이었던 게 thinking 108.3s 와 일치).
  - Fix: 빈 string detail 을 explicit clear 로 contract 정립
    (provider/UI/agent 3 곳 동시에 mirror).

- **M22 — same-round 두 번째 subagent deferred** (2026-05-12 재현)
  - **workaround**: round 당 1 spawn 만. 어기지 말 것.
  - 두 개 동시에 띄우면 두 번째가 background 로 deferred 되어
    실시간 출력이 안 보임.
- **M41 — server-initiated turn 첫 stream event 가 client redraw 안 깨움**
  - ✅ DONE (2026-05-12 라이브 검증). deploy `m41-eefa3744`.
  - 잔여 검증: thought-line + woke 조합 회귀 테스트, sibling fanout
    다른 attached client redraw, auto-poke 동일 경로 확인 (모두
    선택적, 메인 케이스는 OK).
- **M40 Phase 3 — `" /tmp/..."` leading-space 가 slash-mode 진입**
  - ✅ DONE (2026-05-13). deploy `m40-862578f1`. `composer_mode` 에
    `leading_space_escapes_slash()` 헬퍼: leading whitespace 면
    무조건 Chat. 17개 ui::input_ui 테스트 통과. fork pushed.
- **M40 Phase 1-2 — 이미지 첨부 silent fail**
- **M40 Phase 4 — Opus 1m 메인 picker 미advertise**
- **M16 (구조 개선) 은 가장 마지막**

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
