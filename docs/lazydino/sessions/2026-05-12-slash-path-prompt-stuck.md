# 2026-05-12 — Slash-prefixed prompt stuck in Stadium remote client

## 증상

`stadium server + remote client` (예: Worm/GPT-5.5) 에서 `/` 로 시작하는
일반 prompt 를 보내면, status 가 `• sending… 6.0s …` 에서 무한 stuck.
파일 추가/수정 작업 같은 평범한 요청이 전혀 전달되지 않음.

재현 케이스:

- `"/tmp/file-conflict-test.txt"` 단독 (공백 없음)
- `"/tmp/file-conflict-test.txt 단"` (공백 + 첫 단어)
- 이미지를 첨부해 같은 의도로 요청하면 정상 동작 (서로 다른 코드 경로)

같은 client 가 local 모드일 때는 stuck 대신 "Unknown skill: /xxx" 에러가
뜨고 끝남 — 이는 local 의 `tui/app/input.rs` 가 unknown_skill_invocation
fallback 을 가지고 있기 때문.

## Root cause

`src/tui/app/remote/input_dispatch.rs::submit_prepared_remote_input` 의 97~104
줄:

```rust
if let Some(skill_name) = SkillRegistry::parse_invocation(&prepared.expanded) {
    remote.activate_skill(skill_name).await?;
    app.input.clear();
    app.cursor_pos = 0;
    app.pending_images.clear();
    app.set_status_notice(format!("Activating skill: /{}", skill_name));
    return Ok(());
}
```

`SkillRegistry::parse_invocation` 는 단순히 `trimmed.starts_with('/') &&
!trimmed.contains(' ')` 만 검사한다 (`src/skill.rs:279-286`). 즉 공백이 없는
모든 `/` prefix 입력 (`/tmp/foo.txt` 같은 OS 경로 포함) 을 skill name 후보로
간주하고 **존재 여부 검사 없이** 무조건 `Request::ActivateSkill` 을 서버로
보낸다.

서버 측 (`src/server/client_lifecycle.rs:1963` 의 `ActivateSkill` handler)
은 `Agent::activate_skill(name)` 을 호출하는데, 존재하지 않는 skill 이면
`Err` 가 나서 `ServerEvent::Error` 가 client 한테 다시 전달돼야 정상이지만,
client 가 `Error` 이벤트를 받아도 이 시점에 `is_processing=true` 가 별도
경로에서 세팅되어 있어 (또는 status_notice 만 뜨고 user bubble 은 이미
별곳에서 그려진 상태에서) 무한 sending 으로 보이게 된다.

핵심은 — slash-prefixed 토큰이 **항상** skill 로 해석되는 것이 잘못이다.
실제로 skill 이 존재할 때만 ActivateSkill 을 보내고, 그렇지 않으면 일반
prompt 로 fall through 해야 한다 (local 의 `tui/app/input.rs:1911-2062` 가
이미 그런 fallback 을 구현하고 있음).

## Fix

`submit_prepared_remote_input` 안에서 `parse_invocation` 매치 시
local 의 패턴을 그대로 적용:

1. `app.current_skills_snapshot().get(skill_name)` 으로 1차 확인
2. 미발견이면 `SkillRegistry::load_for_working_dir(working_dir)` 으로 reload
   재시도 + 새로 로드된 registry 를 `app.skills` 와 `app.registry.skills()`
   에 반영
3. 그래도 미발견이면 `activate_skill` 보내지 않고 fall-through →
   `begin_remote_send` 로 일반 prompt 전송

서버 측은 보통 `send_message` 로 도착한 텍스트의 leading `/` 를 별도로
가로채지 않으므로 (`rg -n "parse_invocation" src/server/` 결과 0), LLM 한테
원문 그대로 전달되어 사용자가 의도한 동작이 일어난다.

## 검증 계획

- 빌드 (lib + bins) — 다른 세션의 `cfg(test) headless` 패치
  (`src/server/comm_session.rs`) 와 충돌 없음
- 단위 회귀 테스트는 별도로 추가 권장 — `submit_prepared_remote_input`
  에 대한 fake `RemoteConnection` mock 이 현재 없어서 실용적으로는
  통합 테스트로 충당. 또는 `parse_invocation` 결과 + skill snapshot 결합
  로직만 추출해 pure-fn 단위 테스트 가능
- 라이브 검증: shared-server binary 갱신 후 새 remote client 띄워서
  `/tmp/zzz.txt 만들어줘` 보내고 stuck 없이 LLM 응답 도착 확인

## 한계

기존에 켜져 있는 remote client (active swarm worker 들) 는 새 client
binary 가 빌드되어도 즉시 fix 적용 안 됨. 사용자가 그 client 를 재시작
해야 함. 활성 서버 자체는 kill 하지 않음.

## 영역

`src/tui/app/remote/input_dispatch.rs` 단일 함수 패치, 약 35줄 추가.
