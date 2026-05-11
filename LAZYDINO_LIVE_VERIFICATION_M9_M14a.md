# Live Verification Plan — M9, M10, M12, M13, M14, M14a

**Build SHA**: `82b7c81f` (deploy/m9-m10 branch)
**Build location**: `~/.jcode/builds/versions/lazydino-82b7c81f/jcode`
**Symlinks**: `~/.local/bin/jcode`, `~/.jcode/builds/stable/jcode`, `~/.jcode/builds/current/jcode` 모두 같은 binary md5 `e1253115b0978aa863453c665b3303a4`
**작성일**: 2026-05-10 14:57 UTC

---

## ⚠️ Prerequisite: 활성 server 재시작

활성 jcode server (PID 1770980 또는 비슷) 가 옛 binary 를 메모리에 들고 있음. 새 fix 효과 보려면 사용자가 **TUI 에서 `/restart`** 한 번 실행 필요.

`/restart` 가 실패하거나 효과 없으면:
```bash
# fallback (위험: 현재 세션 끊김 가능)
ps aux | grep "jcode serve" | grep -v grep | awk '{print $2}' | head -1
# 그 PID 에 대해 kill -TERM (graceful) 또는 사용자가 jcode 직접 재실행
```

확인:
```bash
ls -l /proc/$(pgrep -f "jcode serve" | head -1)/exe
# 출력이 ~/.jcode/builds/versions/lazydino-82b7c81f/jcode 인지 확인
```

---

## ✅ M9 — Hook 이중 발동 fix

**증상 (fix 전)**: `~/` 또는 `~/.jcode` ancestor 에서 `jcode` 실행 시 모든 lifecycle/tool hook 이 **2회 fire** (글로벌 + project-local 같은 파일을 두 번 읽음).

**검증 방법**:

1. 사전 준비 — `~/.jcode/config.toml` 에 hook 추가 (없으면 추가, 있으면 그대로):
   ```toml
   [hooks]
   enabled = true

   [[hooks.commands]]
   event = "tool.execute.before"
   tool = "bash"
   command = "echo \"M9-fire $(date +%H%M%S%N)\" >> /tmp/m9-hook-log"
   blocking = false
   timeout_ms = 5000
   ```

   ⚠️ **주의**: 필드명은 `tool` (not `matcher`), 값은 `"bash"` 소문자 (not `"Bash"`).
   소스 진리: `crates/jcode-config-types/src/lib.rs:481` 의 `tool: Option<String>` 필드 + `src/tool/bash.rs:535` 의 `fn name() -> "bash"`.

   ⚠️ **Config 변경 후 반드시 `/restart`**: `Config::load()` 가 `OnceLock` 으로 한 번만 로드됨 (`src/config.rs:23`). Server 살아있는 동안 config 변경 reflect 안 됨.

2. 로그 초기화:
   ```bash
   rm -f /tmp/m9-hook-log
   ```

3. **`~` 에서 jcode 실행** (이게 트리거 조건):
   ```bash
   cd ~ && jcode
   ```

4. TUI 에서 아무 bash 도구 한 번 실행되게 메시지 보내기, 예:
   > "Run `ls /tmp` and tell me what you see"

5. AI 가 bash 도구 호출하는 turn 1회 후 hook log 확인:
   ```bash
   cat /tmp/m9-hook-log
   wc -l /tmp/m9-hook-log
   ```

**합격 기준**:
- `wc -l` 결과가 **1** (fix 전: 2)
- 라인이 정확히 한 번만 기록되어야 함

**불합격 시 보고할 정보**:
- `cat /tmp/m9-hook-log` 전체 출력
- `jcode --version` 또는 `ls -l /proc/$(pgrep -f "jcode serve")/exe` 로 활성 binary path 확인
- `cd ~ && jcode debug config | grep -A3 hooks` (hook 이 몇 번 등록되는지 직접 출력)

---

## ✅ M10 — Non-blocking hook race fix

**증상 (fix 전)**: 단발성 CLI (`jcode run`) 에서 `blocking=false` lifecycle/tool hook 이 race 로 누락 (process 가 hook 끝나기 전에 종료).

**검증 방법**:

1. 사전 준비 — non-blocking hook 추가 (위 M9 hook 옆에):
   ```toml
   [[hooks.commands]]
   event = "session.stop"
   command = "sleep 0.5 && echo \"M10-stop-fired $(date +%H%M%S%N)\" >> /tmp/m10-hook-log"
   blocking = false
   timeout_ms = 5000
   ```

2. 로그 초기화:
   ```bash
   rm -f /tmp/m10-hook-log
   ```

3. **단발성 CLI 실행**:
   ```bash
   echo "say hi briefly" | jcode run
   ```

4. 종료 후 즉시 (1초 내):
   ```bash
   cat /tmp/m10-hook-log
   ```

**합격 기준**:
- `M10-stop-fired ...` 라인이 정확히 **1줄** 기록됨 (fix 전: 0줄, race 로 lost)
- `jcode run` 종료 후 약 0.5~5초 사이에 라인 등장 (sleep 0.5 + flush timeout 5s)

**불합격 시 보고할 정보**:
- `cat /tmp/m10-hook-log` (비어있는지 확인)
- `journalctl --user | tail -20` 또는 jcode log 에서 "non-blocking lifecycle hook" 관련 메시지

---

## ✅ M12 + M13 — Anthropic OAuth 도구 schema alignment

**증상 (fix 전)**: AI 가 `ToolSearch` 호출하면 `Unknown tool: ToolSearch` 에러. `ScheduleWakeup` 호출하면 `missing field 'task'` 에러.

**검증 방법**:

1. **Anthropic OAuth provider 로 세션 시작** (claude pro/max 계정 OAuth)

2. AI 한테 codesearch + schedule 사용 유도 메시지:
   > "Search this codebase for `paths_resolve_to_same_file` using your codesearch tool, then schedule a wakeup in 1 minute with task='check m9 hook log'"

3. AI 응답에서 두 도구 호출이 **���러 없이** 성공하는지 확인

**합격 기준**:
- `ToolSearch` (또는 `codesearch`) 호출 결과가 정상 (파일 위치 결과 반환됨)
- `ScheduleWakeup` 호출 결과가 정상 (스케줄 등록 확인 메시지)
- 둘 다 `Unknown tool` 또는 `missing field` 에러 없음

**불합격 시 보고할 정보**:
- AI 응답 전문
- jcode log 에서 "ToolSearch" 또는 "ScheduleWakeup" 관련 에러 메시지

---

## ✅ M14 + M14a — Compaction failure cooldown

**증상 (fix 전)**: 
- M14: `/compact` 실패하면 매 turn 마다 proactive auto-compaction 재발화 (cooldown 무력화)
- M14a: emergency compaction 22회 무한루프 (504k→20k→500k 반복)

**검증 방법 (자연 발생 대기)**: 
이건 인위적 재현이 어려움. 다음 jcode 사용 중 자연스럽게 관찰:

1. 평소처럼 jcode 사용
2. context 가 많이 쌓였을 때 (예: 100k+ tokens) `/compact` 실행
3. compaction 실패 메시지가 나오면 확인:
   - **fix 전 동작**: 그 후 매 turn 마다 자동 compaction 시도 재발생
   - **fix 후 동작**: 3회 연속 실패 후 자동 compaction 자체가 disable 됨, "compaction disabled after 3 consecutive failures" 비슷한 로그

**합격 기준 (관찰)**:
- compaction 실패가 발생해도 **매 turn 마다 재시도하지 않음**
- emergency compaction (504k 같은 거대 context 시) 도 22회 폭주 없이 3회에서 멈춤

**불합격 시 보고할 정보**:
- jcode log 에서 "compaction" 키워드 grep 결과
- 발생 시점의 context size + turn 횟수

---

## 📋 검증 결과 보고 양식

검증이 끝나면 아래 양식으로 lazydino main session 에 알려주세요:

```markdown
## Live verification 2026-05-10 (binary 82b7c81f)

- [ ] M9 hook 이중 발동 — `wc -l /tmp/m9-hook-log` 결과: __
- [ ] M10 non-blocking hook race — `cat /tmp/m10-hook-log` 결과: __
- [ ] M12 ToolSearch — 에러 없이 동작? Y/N
- [ ] M13 ScheduleWakeup — 에러 없이 동작? Y/N
- [ ] M14 compaction cooldown — 매 turn 재시도 없음? Y/N (자연 관찰)
- [ ] M14a emergency compaction — 22회 폭주 없음? Y/N (자연 관찰)

(불합격 항목이 있으면 위 "불합격 시 보고할 정보" 수집)
```
