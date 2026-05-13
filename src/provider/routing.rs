pub(crate) fn should_eager_detect_copilot_tier() -> bool {
    std::env::var("JCODE_NON_INTERACTIVE").is_err()
}

pub(crate) fn is_transient_transport_error(error_str: &str) -> bool {
    let lower = error_str.to_ascii_lowercase();
    lower.contains("connection reset")
        || lower.contains("connection closed")
        || lower.contains("connection refused")
        || lower.contains("connection aborted")
        || lower.contains("broken pipe")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("operation timed out")
        || lower.contains("error decoding")
        || lower.contains("error reading")
        || lower.contains("unexpected eof")
        || lower.contains("tls handshake eof")
        || lower.contains("badrecordmac")
        || lower.contains("bad_record_mac")
        || lower.contains("fatal alert: badrecordmac")
        || lower.contains("fatal alert: bad_record_mac")
        || lower.contains("received fatal alert: badrecordmac")
        || lower.contains("received fatal alert: bad_record_mac")
        || lower.contains("decryption failed or bad record mac")
        || lower.contains("temporary failure in name resolution")
        || lower.contains("failed to lookup address information")
        || lower.contains("dns error")
        || lower.contains("name or service not known")
        || lower.contains("no route to host")
        || lower.contains("network is unreachable")
        || lower.contains("host is unreachable")
}

pub(crate) fn anthropic_oauth_route_availability(model: &str) -> (bool, String) {
    if model.ends_with("[1m]") && !crate::usage::has_extra_usage() {
        (false, "requires extra usage".to_string())
    } else if model.contains("opus") && !crate::auth::claude::is_max_subscription() {
        (false, "requires Max subscription".to_string())
    } else {
        (true, String::new())
    }
}

pub(crate) fn anthropic_api_key_route_availability(model: &str) -> (bool, String) {
    if model.ends_with("[1m]") && !crate::usage::has_extra_usage() {
        (false, "requires extra usage".to_string())
    } else {
        (true, String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_1m_routes_are_advertisable_with_extra_usage_hint_when_unknown() {
        for model in ["claude-opus-4-7[1m]", "claude-sonnet-4-6[1m]"] {
            let (oauth_available, oauth_detail) = anthropic_oauth_route_availability(model);
            let (api_key_available, api_key_detail) = anthropic_api_key_route_availability(model);

            assert!(!oauth_available, "{model} should be visible but disabled");
            assert_eq!(oauth_detail, "requires extra usage");
            assert!(!api_key_available, "{model} should be visible but disabled");
            assert_eq!(api_key_detail, "requires extra usage");
        }
    }

    #[test]
    fn anthropic_non_1m_api_key_routes_are_available_without_usage_fetch() {
        let (available, detail) = anthropic_api_key_route_availability("claude-opus-4-7");

        assert!(available);
        assert!(detail.is_empty());
    }
}
