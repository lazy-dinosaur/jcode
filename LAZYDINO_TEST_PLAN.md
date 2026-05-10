# Lazydino Jcode — Patch Effect Manual Test Plan

마지막 업데이트: 2026-05-10
관련 문서: `LAZYDINO_MILESTONES.md`, `LAZYDINO_MAINTENANCE.md`

이 문서는 우리가 적용한 patch 들이 **실제 사용 환경에서 의도대로 동작하는지** 사용자가 수동으로 검증할 때 사용하는 테스트 매뉴얼입니다.

## 사전 조건

테스트 시작 전 반드시 확인:

```bash
# 1) binary 가 최신인지 확인
~/.local/bin/jcode --version
# 기대값: v0.12.115-dev (1c97ef70) 또는 더 최신

# 2) 현재 도는 jcode server 가 새 binary 인지 확인
ps -ef | grep "jcode .* serve" | grep -v grep
# 시작 시간 (etime) 이 install 시간보다 나중이어야 함
# 그렇지 않으면 /reload 또는 server kill + 재시작 필요

# 3) 새 binary 가 메모리에 올라간지 확인
for pid in $(pgrep -f "jcode .* serve"); do
    readlink /proc/$pid/exe
done
# /home/lazydino/.jcode/builds/versions/lazydino-1c97ef70/jcode 같이 나와야 함
```

서버 재시작 절차:

```bash
# 옵션 A: jcode 안에서 (가장 안전, session 유지 시도)
/reload

# 옵션 B: 외부에서 강제 (session 메모리 reload 가능, 단 .json snapshot 이 .journal 보다 오래됐으면 메시지 유실 위험 — M7 참고)
pkill -f "jcode .* serve"
# 다음 jcode 호출 시 새 server 가 새 binary 로 자동 시작
```

⚠️ **현재 알려진 위험** (2026-05-10): reload 시 `.json` snapshot 이 `.journal.jsonl` 보다 오래되어 있으면 메시지가 다 사라짐. 이게 M7 으로 추적 중. M7 끝나기 전엔 reload 신중하게.

---

## 테스트 케이스

### T-M5 — Alt+B early race fix 검증

**목적**: ToolStart 직후 빠른 Alt+B 가 안전하게 background detach 되는지

**시나리오 1: 빠른 Alt+B**
1. 새 jcode 세션 시작.
2. 짧은 prompt: `bash 명령어로 sleep 30 실행해줘`.
3. ToolStart 카드 (`bash` 가 시작됨) 가 화면에 뜨자마자 **즉시 (1 초 안에)** Alt+B.
4. 기대 결과:
   - "Moving tool to background..." status notice 가 뜨고 → 곧이어 background task 카드로 전환
   - `Tool 'bash' was moved to background by the user (task_id: ...)` 메시지가 history 에 추가됨
   - turn loop 가 다음 모델 응답으로 진행 (M6 가 deferred 라 upstream 동작 그대로)

**시나리오 2: 연속 Alt+B**
1. 한 turn 안에서 tool 두 개 호출되는 prompt: `pwd 실행하고 그 다음 sleep 20 실행해줘`.
2. 두 번째 (sleep) tool 의 ToolStart 직후 Alt+B.
3. 기대 결과: 첫 번째 tool 영향 없음, 두 번째 tool 만 background 로 detach.

**기존 회귀 (이 테스트가 catch 해야 함)**:
- Alt+B 가 무시됨 (status notice 만 뜨고 background 안 들어감)
- `background_tool_signal.reset()` 이 fire 를 wipe 해서 발생했었음. M5 patch 로 해결.

**fail 시 진단**:
```bash
grep "Tool '.*' moved to background" ~/.jcode/logs/jcode-$(date -u +%Y-%m-%d).log | tail -5
# 위 로그가 안 나오면 server 가 옛 binary 로 도는 것일 수 있음
```

---

### T-M1 — Background task delivery target routing 검증

**목적**: Alt+B 로 background detach 한 task 가 완료되었을 때 부모 TUI 가 알림 받는지 (이전에는 "No output captured" 만 떴음)

**시나리오: subagent + Alt+B + 완료 알림**
1. 새 jcode 세션 시작.
2. subagent 호출 prompt: `subagent 띄워서 sleep 30 && echo "subagent done" 실행하고 결과 알려줘`.
3. subagent ToolStart 가 뜨면 즉시 Alt+B.
4. **30 초 정도 기다림** (subagent 가 background 에서 자기 일 마치는 시간).
5. 기대 결과:
   - subagent 완료 시 부모 TUI 에 background task 완료 카드가 표시됨
   - 카드에 실제 output (`subagent done` 등) 이 보여야 함
   - "No output captured" 만 뜨면 fail

**검증 명령**:
```bash
# 완료 후 부모 세션의 journal 에 BackgroundTaskCompleted 가 도달했는지
grep "background_task" ~/.jcode/logs/jcode-$(date -u +%Y-%m-%d).log | tail -10

# 이전 fail 패턴: "Failed to notify attached clients for background task completion"
grep "Failed to notify attached clients" ~/.jcode/logs/jcode-$(date -u +%Y-%m-%d).log | tail -3
# patch 적용 후엔 이 WARN 이 줄어들거나 없어야 정상
```

**기존 회귀 (이 테스트가 catch 해야 함)**:
- subagent 가 끝나도 부모 TUI 카드가 "No output captured" 만 표시
- `fanout_session_event` 가 parent chain 추적 안 해서 발생.
- `run_background_task_message_in_live_session_if_idle` 의 headless drain false-live 판정도 같이 fix.

---

### T-M3 — Lifecycle hooks (`session.stop`, `response.completed`) 검증

**목적**: 새로 추가된 두 hook event 가 발행되는지

**준비**:
```bash
# 테스트용 hook 설정 추가 (project 또는 global)
cat >> ~/.jcode/config.toml <<'EOF'

[hooks]
enabled = true

[[hooks.commands]]
event = "response.completed"
command = "tee -a /tmp/jcode-response-completed.log"
blocking = false

[[hooks.commands]]
event = "session.stop"
command = "tee -a /tmp/jcode-session-stop.log"
blocking = false
EOF

# 로그 초기화
: > /tmp/jcode-response-completed.log
: > /tmp/jcode-session-stop.log
```

**시나리오 1: response.completed 발행**
1. 새 jcode 세션 시작 후 짧은 prompt 한 번 (예: `안녕하세요`).
2. 모델 응답 끝난 후:
   ```bash
   cat /tmp/jcode-response-completed.log
   # 한 줄 (또는 여러 줄, 한 turn 당 한 번) 의 JSON payload 가 있어야 함
   # session_id, message_id, stop_reason, tool_calls_count, output_chars 포함
   ```

**시나리오 2: retry 중간에 발행 안 됨**
- (이건 자동 재현 어려움, 단위 테스트로 보장됨: `lifecycle_hook_fires_matching_command` 등)

**시나리오 3: session.stop 발행**
1. 위 prompt 후 jcode 세션 종료 (`/exit` 또는 Ctrl+D).
2. 종료 후:
   ```bash
   cat /tmp/jcode-session-stop.log
   # 한 줄의 JSON payload, reason="user_close" 또는 비슷한 값
   ```

**시나리오 4: reload 시 session.stop 발행 안 됨 (중요)**
1. 세션 안에서 `/reload`.
2. reload 후:
   ```bash
   cat /tmp/jcode-session-stop.log
   # 새 줄이 추가되지 않아야 정상 (reload 는 session stop 이 아님)
   ```

**기존 상태**: 두 event 자체가 없었음. M3 patch 로 추가.

---

### T-Smoke — 일반 회귀 검증

**목적**: 우리 patch 들이 평소 동작에 회귀를 만들지 않았는지

1. 새 jcode 세션, 짧은 대화 (3~5 turn) 정상 진행.
2. 한 turn 에서 multi-tool 호출되는 prompt (예: `pwd 한 번, ls /tmp 한 번 실행하고 결과 정리해줘`).
3. subagent 호출 (Alt+B 안 누르고 정상 종료까지) → 결과 회수 정상.
4. session 종료 → 다음 jcode 호출 시 resume 정상.

회귀 fail 시:
- `git log custom/lazydino-harness ^origin/master` 에서 어떤 패치가 영향 줬는지 추적.
- LAZYDINO_MAINTENANCE.md 의 known-failure list 와 비교.

---

## 테스트 결과 기록 양식

각 테스트 후 아래 표 형식으로 결과 기록 (이 문서 하단에 누적).

| 일시 | binary | 테스트 | 결과 | 비고 |
|------|--------|--------|------|------|
| 2026-05-10 ?? | `1c97ef70` | T-M5 시나리오 1 | (미테스트) | reload 후 측정 예정 |
| 2026-05-10 ?? | `1c97ef70` | T-M5 시나리오 2 | (미테스트) | |
| 2026-05-10 ?? | `1c97ef70` | T-M1 | (미테스트) | |
| 2026-05-10 ?? | `1c97ef70` | T-M3 시나리오 1 | (미테스트) | |
| 2026-05-10 ?? | `1c97ef70` | T-M3 시나리오 3 | (미테스트) | |
| 2026-05-10 ?? | `1c97ef70` | T-M3 시나리오 4 | (미테스트) | |
| 2026-05-10 ?? | `1c97ef70` | T-Smoke | (미테스트) | |

---

## 테스트 결과 (누적 기록)

(여기에 실제 테스트 후 결과 추가)
