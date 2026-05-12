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

- **M42 — `checking websocket` 무한 thinking hang** (2026-05-13 00:00 재현)
  - 메인 세션이 subagent 결과 기다리며 thinking 무한 대기
  - status: `thinking… 108.3s · checking websocket · existing websocket · +1 queued`
  - **진단 미수행, 기록만**. 다음 재현 시 즉시 debug socket dump 필요.

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
  - fix 후보: `composer_mode` 에서 leading space 면 무조건 Chat
  - 미착수
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
