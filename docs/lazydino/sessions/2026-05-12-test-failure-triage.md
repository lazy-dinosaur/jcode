# Test failure triage — 2026-05-12

다른 세션이 round 24 (`a5829751`) 직후 `cargo test --lib` 광역 실패를
"환경 의존 / 이번 fix 와 무관" 으로 보고했음. 이 세션에서 그 보고를 검증.

## TL;DR

| 분류 | 항목 | 단독 실행 | 처리 |
|---|---|---|---|
| **A. 환경/격리 의존 (M29)** | `copilot_usage::*`, `auth::*`, `provider::openrouter::*`, `session::tests::*`, `side_panel::*`, `provider::activation::*`, `sidecar::*`, `tool::ambient::*` 등 ~37건 | ✅ **PASS** | M29 (기존 OPEN, 사용자 영향 0) 그대로 |
| **B. 진짜 회귀 (M36, 신규)** | `config::tests::global_config_cache_reloads_after_manual_file_edit`, `config::tests::cached_external_auth_trust_observes_manual_revocation` | ❌ **단독에서도 FAIL** | **이번 ��션 fix → `72df53fc`** |

다른 세션 분석은 **대부분 정확** — 환경 의존 FAIL 은 M29 가설 그대로
확인됨. 다만 위 B 두 건은 **단독에서도 FAIL** 이라 M29 가 cover 못 함.

## 검증 절차

```bash
# 1. 다른 세션 보고된 광역 실패 영역에서 한 건씩 단독 실행
cargo +nightly test --lib copilot_usage::tests           # 2/2 PASS  → M29
cargo +nightly test --lib auth::tests::full_and_fast_auth_status_match_for_shared_probe_fields  # 1/1 PASS → M29
cargo +nightly test --lib provider::openrouter::tests    # 42/42 PASS → M29
cargo +nightly test --lib config::tests::config_save_invalidates_global_config_cache  # 1/1 PASS → M29

# 2. config hot-reload 영역의 두 의심 테스트도 단독 실행
cargo +nightly test --lib config::tests::global_config_cache_reloads_after_manual_file_edit  # 0/1 FAIL ← 진짜 회귀
cargo +nightly test --lib config::tests::cached_external_auth_trust_observes_manual_revocation  # 0/1 FAIL ← 진짜 회귀
```

`--nocapture` 로 panic 보기:

```
thread 'config::tests::global_config_cache_reloads_after_manual_file_edit'
panicked at src/config_tests.rs:112:
  assertion failed: crate::config::config().display.centered
```

## Root cause (M36)

upstream commit `e8f17de6 Reload config cache on file changes` (Sat May 9
2026, jeremy@1jehuang) 가 hot-reload + 500ms debounce + 새 회귀 테스트
3 건을 함께 들여옴 (M21 ver.2 squash-rebase 로 우리 fork 에 들어옴).

`config()` 의 hot-reload 로직 (`src/config.rs:82~95`):

```rust
pub fn config() -> &'static Config {
    // ...
    if now.duration_since(guard.last_checked) >= CONFIG_RELOAD_DEBOUNCE {  // 500ms
        guard.last_checked = now;
        maybe_reload(&mut guard);
    }
    guard.current
}
```

테스트는:

1. `Config::invalidate_cache()` 호출 → `last_checked = Instant::now()`.
2. 첫 `config()` 호출 → debounce 안에 떨어짐, `maybe_reload` 스킵, 이미
   invalidate 가 새로 load 한 값이라 PASS.
3. `std::fs::write(&path, ...)` 로 manual edit.
4. **수 밀리초 안** 두번째 `config()` 호출 → 여전히 같은 500ms 윈도우 →
   `maybe_reload` 스킵 → 옛 값 반환 → assertion FAIL.

이는 production 의 user-edit 시나리오와는 다른 race 임 (user edit 은
보통 수 초 단위라 500ms 윈도우 안에 안 들어옴) — **upstream 이 테스트
작성을 잘못함**.

## Fix (이 세션)

production 의 명시적 reload path (`force_reload_config()` — slash command
또는 `/reload-config` 가 호출하는 helper) 를 테스트에서도 호출:

```rust
// 변경 전
std::fs::write(&path, "...").expect(...);
assert!(crate::config::config().display.centered);  // FAIL

// 변경 후
std::fs::write(&path, "...").expect(...);
assert!(crate::config::force_reload_config());      // debounce bypass
assert!(crate::config::config().display.centered);  // PASS
```

production 코드 변경 0. test-only fix.

## 검증 결과

```
$ cargo +nightly test --lib config::tests::global_config_cache_reloads_after_manual_file_edit -- --test-threads=1
test result: ok. 1 passed; 0 failed

$ cargo +nightly test --lib config::tests::cached_external_auth_trust_observes_manual_revocation -- --test-threads=1
test result: ok. 1 passed; 0 failed

$ cargo +nightly test --lib config::tests -- --test-threads=1
test result: ok. 53 passed; 0 failed; 0 ignored; 0 measured; 2777 filtered out
```

## Round 24 fix 자체 영향?

**0건**. round 24 의 두 commit (`95b370a3` compaction native replay token,
`a5829751` server round-24 debug + OpenAI defaults) 는 위 두 테스트의
config-hot-reload 코드 path 와 무관. 발견은 round 24 의 광역 테스트
실행이 trigger 했을 뿐 회귀 commit 자체는 `c714fcb8` (M21 ver.2) 시점.

## 사용자 영향

| 항목 | 영향 |
|---|---|
| Release binary 동작 | **0** — production hot-reload 자체는 정상 (M19 라이브 검증 + Round 21~23 사용 중) |
| CI hygiene | 두 테스트가 단독에서 FAIL → ✅ Fix 됨 |
| 광역 lib suite | 여전히 M29 패턴 (parallel 시 fail, isolation 시 pass) — 별개 OPEN |

## 후속 작업

- **M29** (test 격리/순서 의존성, 사용자 영향 0): 그대로 OPEN. priority Medium.
  광역 lib suite parallel FAIL 다수의 원인 — 별도 작업 필요.
- **M36**: 이 세션에서 ✅ DONE.

## 다음 세션 체크리스트

- [ ] release build 가 필요한가? — **불필요** (production code 변경 0).
- [ ] fork push? — 안전. `./scripts/fork-push.sh deploy/m9-m27-catchup`.
- [ ] 다음 OPEN 우선순위 진입 (STATUS 권장 순서: M30 검증 → M28 라이브 → M27 → ...).
