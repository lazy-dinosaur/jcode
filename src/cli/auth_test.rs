use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

const DEFAULT_AUTH_TEST_PROVIDER_PROMPT: &str =
    "Reply with exactly AUTH_TEST_OK and nothing else. Do not call tools.";
const DEFAULT_AUTH_TEST_TOOL_PROMPT: &str =
    "Please call the cwd tool exactly once. Then reply with exactly AUTH_TEST_OK and nothing else.";

include!("auth_test/types.rs");
include!("auth_test/run.rs");
include!("auth_test/probes.rs");
include!("auth_test/choice.rs");
