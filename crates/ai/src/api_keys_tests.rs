use std::time::{Duration, SystemTime};

use super::*;

fn make_manager(keys: ApiKeys) -> ApiKeyManager {
    make_manager_with_grok(keys, None)
}

fn make_manager_with_grok(keys: ApiKeys, grok_tokens: Option<GrokTokens>) -> ApiKeyManager {
    ApiKeyManager {
        keys,
        grok_tokens,
        #[cfg(not(target_family = "wasm"))]
        grok_refresh_allowed: false,
        #[cfg(not(target_family = "wasm"))]
        grok_refresh_in_flight: false,
        aws_credentials_state: AwsCredentialsState::Missing,
        aws_credentials_refresh_strategy: AwsCredentialsRefreshStrategy::default(),
        secure_storage_write_version: 0,
        grok_secure_storage_write_version: 0,
    }
}

fn grok_tokens(access_token: &str, expires_in: Option<u64>) -> GrokTokens {
    GrokTokens {
        access_token: access_token.into(),
        refresh_token: Some("refresh".into()),
        expires_at: expires_in.map(|secs| SystemTime::now() + Duration::from_secs(secs)),
        connected_at: None,
    }
}

fn endpoint(
    name: &str,
    url: &str,
    api_key: &str,
    models: &[(&str, Option<&str>)],
) -> CustomEndpoint {
    endpoint_with_keys(
        name,
        url,
        api_key,
        &models
            .iter()
            .enumerate()
            .map(|(i, (n, a))| (*n, *a, format!("cfg-{i}")))
            .collect::<Vec<_>>()
            .iter()
            .map(|(n, a, k)| (*n, *a, k.as_str()))
            .collect::<Vec<_>>(),
    )
}

fn endpoint_with_keys(
    name: &str,
    url: &str,
    api_key: &str,
    models: &[(&str, Option<&str>, &str)],
) -> CustomEndpoint {
    CustomEndpoint {
        name: name.into(),
        url: url.into(),
        api_key: api_key.into(),
        api_format: ApiFormat::default(),
        anthropic_bridge_url: String::new(),
        models: models
            .iter()
            .map(|(n, a, cfg)| CustomEndpointModel {
                name: (*n).into(),
                alias: a.map(|s| s.into()),
                config_key: (*cfg).into(),
            })
            .collect(),
    }
}

// ── serde round-trip ────────────────────────────────────────────

#[test]
fn serde_round_trip_empty() {
    let keys = ApiKeys::default();
    let json = serde_json::to_string(&keys).unwrap();
    let deser: ApiKeys = serde_json::from_str(&json).unwrap();
    assert_eq!(keys, deser);
}

#[test]
fn serde_round_trip_with_provider_keys() {
    let keys = ApiKeys {
        openai: Some("sk-openai".into()),
        anthropic: Some("sk-ant-abc".into()),
        google: Some("AIzaSy123".into()),
        open_router: Some("sk-or-xxx".into()),
        custom_endpoints: vec![],
    };
    let json = serde_json::to_string(&keys).unwrap();
    let deser: ApiKeys = serde_json::from_str(&json).unwrap();
    assert_eq!(keys, deser);
}

#[test]
fn serde_round_trip_with_custom_endpoints() {
    let keys = ApiKeys {
        openai: None,
        anthropic: None,
        google: None,
        open_router: None,
        custom_endpoints: vec![
            endpoint("ep1", "https://a.io/v1", "key1", &[("gpt-4", Some("fast"))]),
            endpoint(
                "ep2",
                "https://b.io/v1",
                "key2",
                &[("llama-70b", None), ("mixtral", Some("mix"))],
            ),
        ],
    };
    let json = serde_json::to_string(&keys).unwrap();
    let deser: ApiKeys = serde_json::from_str(&json).unwrap();
    assert_eq!(keys, deser);
}

#[test]
fn serde_ignores_unknown_fields() {
    let json = r#"{"openai":"sk-x","unknown_field":"value","custom_endpoints":[]}"#;
    let keys: ApiKeys = serde_json::from_str(json).unwrap();
    assert_eq!(keys.openai, Some("sk-x".into()));
    assert!(keys.custom_endpoints.is_empty());
}

#[test]
fn serde_defaults_api_format_for_legacy_endpoints() {
    // Endpoints persisted before the api_format/anthropic_bridge_url fields
    // existed must deserialize as OpenAI-format with no bridge.
    let json = r#"{"custom_endpoints":[{"name":"ep","url":"https://a.io/v1","api_key":"k","models":[{"name":"m","alias":null,"config_key":"cfg-0"}]}]}"#;
    let keys: ApiKeys = serde_json::from_str(json).unwrap();
    let ep = &keys.custom_endpoints[0];
    assert_eq!(ep.api_format, ApiFormat::OpenAiChatCompletions);
    assert!(ep.anthropic_bridge_url.is_empty());
}

// ── has_any_key ─────────────────────────────────────────────────

#[test]
fn has_any_key_false_when_empty() {
    assert!(!ApiKeys::default().has_any_key());
}

#[test]
fn has_any_key_true_for_openai_only() {
    let keys = ApiKeys {
        openai: Some("sk-x".into()),
        ..Default::default()
    };
    assert!(keys.has_any_key());
}

#[test]
fn has_any_key_true_for_custom_endpoints_only() {
    let keys = ApiKeys {
        custom_endpoints: vec![endpoint("ep", "https://a.io", "key", &[("m", None)])],
        ..Default::default()
    };
    assert!(keys.has_any_key());
}

#[test]
fn has_any_key_false_for_endpoint_with_empty_api_key() {
    let keys = ApiKeys {
        custom_endpoints: vec![endpoint("ep", "https://a.io", "", &[("m", None)])],
        ..Default::default()
    };
    assert!(!keys.has_any_key());
}

// ── has_custom_endpoints

#[test]
fn has_custom_endpoints_false_when_empty() {
    assert!(!ApiKeys::default().has_custom_endpoints());
}

#[test]
fn has_custom_endpoints_true_when_present() {
    let keys = ApiKeys {
        custom_endpoints: vec![endpoint("ep", "https://a.io", "k", &[("m", None)])],
        ..Default::default()
    };
    assert!(keys.has_custom_endpoints());
}

// ── custom_model_providers_for_request ──────────────────────────

#[test]
fn custom_model_providers_none_when_empty() {
    let mgr = make_manager(ApiKeys::default());
    assert!(mgr.custom_model_providers_for_request(true).is_none());
}

#[test]
fn custom_model_providers_none_when_byo_disabled() {
    let mgr = make_manager(ApiKeys {
        custom_endpoints: vec![endpoint("ep", "https://a.io", "k", &[("m", None)])],
        ..Default::default()
    });
    assert!(mgr.custom_model_providers_for_request(false).is_none());
}

#[test]
fn custom_model_providers_populates_single_endpoint() {
    let mgr = make_manager(ApiKeys {
        custom_endpoints: vec![endpoint_with_keys(
            "My EP",
            "https://custom.io/v1",
            "ep-key",
            &[("big-model", Some("alias"), "uuid-1")],
        )],
        ..Default::default()
    });
    let result = mgr.custom_model_providers_for_request(true).unwrap();
    assert_eq!(result.providers.len(), 1);
    let p = &result.providers[0];
    assert_eq!(p.base_url, "https://custom.io/v1");
    assert_eq!(p.api_key, "ep-key");
    assert_eq!(p.models.len(), 1);
    assert_eq!(p.models[0].slug, "big-model");
    assert_eq!(p.models[0].config_key, "uuid-1");
}

#[test]
fn multiple_endpoints_all_serialize() {
    let mgr = make_manager(ApiKeys {
        custom_endpoints: vec![
            endpoint_with_keys(
                "ep1",
                "https://a.io",
                "k1",
                &[("gpt-4", Some("fast"), "uuid-a")],
            ),
            endpoint_with_keys(
                "ep2",
                "https://b.io",
                "k2",
                &[
                    ("llama-70b", None, "uuid-b"),
                    ("mixtral", Some("mix"), "uuid-c"),
                ],
            ),
        ],
        ..Default::default()
    });
    let result = mgr.custom_model_providers_for_request(true).unwrap();
    assert_eq!(result.providers.len(), 2);
    assert_eq!(result.providers[0].base_url, "https://a.io");
    assert_eq!(result.providers[0].models[0].config_key, "uuid-a");
    assert_eq!(result.providers[1].base_url, "https://b.io");
    assert_eq!(result.providers[1].models.len(), 2);
    assert_eq!(result.providers[1].models[0].slug, "llama-70b");
    assert_eq!(result.providers[1].models[0].config_key, "uuid-b");
    assert_eq!(result.providers[1].models[1].config_key, "uuid-c");
}

#[test]
fn byok_disabled_returns_none_even_with_endpoints() {
    let mgr = make_manager(ApiKeys {
        custom_endpoints: vec![endpoint("ep", "https://a.io", "k", &[("m", None)])],
        ..Default::default()
    });
    assert!(mgr.custom_model_providers_for_request(false).is_none());
}

#[test]
fn empty_api_key_endpoints_are_skipped() {
    let mgr = make_manager(ApiKeys {
        custom_endpoints: vec![
            endpoint_with_keys("empty", "https://a.io", "", &[("m", None, "uuid-x")]),
            endpoint_with_keys("ok", "https://b.io", "k", &[("m", None, "uuid-y")]),
        ],
        ..Default::default()
    });
    let result = mgr.custom_model_providers_for_request(true).unwrap();
    assert_eq!(result.providers.len(), 1);
    assert_eq!(result.providers[0].base_url, "https://b.io");
}

#[test]
fn endpoints_with_only_empty_models_are_skipped() {
    let mgr = make_manager(ApiKeys {
        custom_endpoints: vec![endpoint_with_keys(
            "ep",
            "https://a.io",
            "k",
            &[("", None, "uuid-z")],
        )],
        ..Default::default()
    });
    assert!(mgr.custom_model_providers_for_request(true).is_none());
}

// ── display_label fallback ─────────────────────────────────────

#[test]
fn display_label_uses_alias_when_present() {
    let m = CustomEndpointModel {
        name: "raw-name".into(),
        alias: Some("My Alias".into()),
        config_key: "k".into(),
    };
    assert_eq!(m.display_label(), "My Alias");
}

#[test]
fn display_label_falls_back_to_name_when_alias_missing() {
    let m = CustomEndpointModel {
        name: "raw-name".into(),
        alias: None,
        config_key: "k".into(),
    };
    assert_eq!(m.display_label(), "raw-name");
}

#[test]
fn display_label_falls_back_to_name_when_alias_is_whitespace() {
    let m = CustomEndpointModel {
        name: "raw-name".into(),
        alias: Some("   ".into()),
        config_key: "k".into(),
    };
    assert_eq!(m.display_label(), "raw-name");
}

// ── api_keys_for_request ────────────────────────────────────────

#[test]
fn api_keys_for_request_none_when_empty() {
    let mgr = make_manager(ApiKeys::default());
    assert!(mgr.api_keys_for_request(true, false).is_none());
}

#[test]
fn api_keys_for_request_populates_provider_keys() {
    let mgr = make_manager(ApiKeys {
        openai: Some("sk-o".into()),
        anthropic: Some("sk-a".into()),
        ..Default::default()
    });
    let result = mgr.api_keys_for_request(true, false).unwrap();
    assert_eq!(result.openai, "sk-o");
    assert_eq!(result.anthropic, "sk-a");
    assert!(result.google.is_empty());
}

#[test]
fn api_keys_for_request_omits_keys_when_byo_disabled() {
    let mgr = make_manager(ApiKeys {
        openai: Some("sk-o".into()),
        ..Default::default()
    });
    // With BYO disabled and no other credentials, returns None.
    assert!(mgr.api_keys_for_request(false, false).is_none());
}

#[test]
fn api_keys_for_request_none_for_custom_endpoints_only() {
    let mgr = make_manager(ApiKeys {
        custom_endpoints: vec![endpoint("ep", "https://a.io", "k", &[("m", None)])],
        ..Default::default()
    });
    assert!(mgr.api_keys_for_request(true, false).is_none());
}

// ── grok oauth token ────────────────────────────────────────────

#[test]
fn grok_access_token_present_without_expiry() {
    let t = GrokTokens {
        access_token: "tok".into(),
        ..Default::default()
    };
    assert_eq!(t.access_token_for_request(), Some("tok"));
}

#[test]
fn grok_access_token_blank_is_none() {
    let t = GrokTokens {
        access_token: "   ".into(),
        ..Default::default()
    };
    assert_eq!(t.access_token_for_request(), None);
}

#[test]
fn grok_access_token_near_expiry_still_sent() {
    // Expired tokens are still sent; the server is the authority on validity.
    let t = grok_tokens("tok", Some(0));
    assert_eq!(t.access_token_for_request(), Some("tok"));
}

#[test]
fn grok_access_token_far_future_is_some() {
    let t = grok_tokens("tok", Some(3600));
    assert_eq!(t.access_token_for_request(), Some("tok"));
}

#[test]
fn grok_needs_refresh_within_lead_time() {
    assert!(grok_tokens("tok", Some(30)).needs_refresh(Duration::from_secs(300)));
    assert!(!grok_tokens("tok", Some(3600)).needs_refresh(Duration::from_secs(300)));
    // Expired tokens still need a refresh.
    assert!(grok_tokens("tok", Some(0)).needs_refresh(Duration::from_secs(300)));
    // Unknown expiry never reports as needing refresh.
    assert!(!grok_tokens("tok", None).needs_refresh(Duration::from_secs(300)));
}

#[test]
fn api_keys_for_request_includes_grok_token() {
    let mgr = make_manager_with_grok(
        ApiKeys::default(),
        Some(grok_tokens("grok-abc", Some(3600))),
    );
    let result = mgr.api_keys_for_request(true, false).unwrap();
    assert_eq!(result.grok_oauth_access_token, "grok-abc");
    assert!(result.anthropic.is_empty());
}

#[test]
fn api_keys_for_request_omits_grok_token_when_byo_disabled() {
    // The Grok subscription is user-provided auth, so it follows the BYO
    // policy gate: with BYO disabled and no other credentials, returns None.
    let mgr = make_manager_with_grok(
        ApiKeys::default(),
        Some(grok_tokens("grok-abc", Some(3600))),
    );
    assert!(mgr.api_keys_for_request(false, false).is_none());
}

#[test]
fn api_keys_for_request_includes_expired_grok_token() {
    // Expired tokens are still sent in requests; the server rejects truly
    // invalid ones and the background refresh replaces them.
    let mgr = make_manager_with_grok(ApiKeys::default(), Some(grok_tokens("grok-abc", Some(0))));
    let result = mgr.api_keys_for_request(true, false).unwrap();
    assert_eq!(result.grok_oauth_access_token, "grok-abc");
}

// ── request_base_url ────────────────────────────────────────────

#[test]
fn request_base_url_openai_passthrough() {
    let ep = endpoint("ep", "https://a.io/v1", "k", &[("m", None)]);
    assert_eq!(ep.request_base_url(), "https://a.io/v1");
}

#[test]
fn request_base_url_anthropic_without_bridge_falls_back_to_raw_url() {
    let mut ep = endpoint(
        "ep",
        "https://api.minimax.io/anthropic",
        "k",
        &[("m", None)],
    );
    ep.api_format = ApiFormat::AnthropicMessages;
    assert_eq!(ep.request_base_url(), "https://api.minimax.io/anthropic");
}

#[test]
fn request_base_url_anthropic_with_bridge_encodes_target() {
    use base64::Engine as _;

    let mut ep = endpoint(
        "ep",
        "https://api.minimax.io/anthropic",
        "k",
        &[("m", None)],
    );
    ep.api_format = ApiFormat::AnthropicMessages;
    ep.anthropic_bridge_url = "https://bridge.example.com/".into();

    let url = ep.request_base_url();
    let encoded = url
        .strip_prefix("https://bridge.example.com/a/")
        .expect("bridged URL should start with the bridge base and /a/");
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .unwrap();
    assert_eq!(decoded, b"https://api.minimax.io/anthropic");
}

#[test]
fn custom_model_providers_use_bridged_base_url() {
    let mut ep = endpoint(
        "ep",
        "https://api.minimax.io/anthropic",
        "k",
        &[("m", None)],
    );
    ep.api_format = ApiFormat::AnthropicMessages;
    ep.anthropic_bridge_url = "https://bridge.example.com".into();
    let expected = ep.request_base_url();

    let manager = make_manager(ApiKeys {
        custom_endpoints: vec![ep],
        ..Default::default()
    });
    let providers = manager.custom_model_providers_for_request(true).unwrap();
    assert_eq!(providers.providers[0].base_url, expected);
}

// ── CustomEndpointDraft: normalization + validation ─────────────

fn draft(api_format: ApiFormat, bridge: &str) -> CustomEndpointDraft {
    CustomEndpointDraft {
        name: "MiniMax".into(),
        url: "https://api.minimax.io/v1".into(),
        api_key: "sk-cp-test".into(),
        api_format,
        anthropic_bridge_url: bridge.into(),
        models: vec![("MiniMax-M3".into(), Some("MiniMax M3".into()), None)],
    }
}

#[test]
fn draft_clears_bridge_url_for_openai_format() {
    // A stale bridge URL left from Anthropic mode is dropped on conversion.
    let ep: CustomEndpoint = draft(
        ApiFormat::OpenAiChatCompletions,
        "https://bridge.example.com",
    )
    .into();
    assert!(ep.anthropic_bridge_url.is_empty());
}

#[test]
fn draft_keeps_bridge_url_for_anthropic_format() {
    let ep: CustomEndpoint =
        draft(ApiFormat::AnthropicMessages, "https://bridge.example.com").into();
    assert_eq!(ep.anthropic_bridge_url, "https://bridge.example.com");
}

#[test]
fn draft_assigns_config_key_when_missing() {
    let ep: CustomEndpoint = draft(ApiFormat::OpenAiChatCompletions, "").into();
    assert_eq!(ep.models.len(), 1);
    assert!(!ep.models[0].config_key.is_empty());
}

#[test]
fn draft_validate_ok_for_complete_draft() {
    assert!(draft(ApiFormat::OpenAiChatCompletions, "")
        .validate()
        .is_ok());
}

#[test]
fn draft_validate_rejects_missing_fields() {
    let mut d = draft(ApiFormat::OpenAiChatCompletions, "");
    d.name = "  ".into();
    assert!(d.validate().is_err());

    let mut d = draft(ApiFormat::OpenAiChatCompletions, "");
    d.url = String::new();
    assert!(d.validate().is_err());

    let mut d = draft(ApiFormat::OpenAiChatCompletions, "");
    d.api_key = String::new();
    assert!(d.validate().is_err());

    let mut d = draft(ApiFormat::OpenAiChatCompletions, "");
    d.models = vec![("   ".into(), None, None)];
    assert!(d.validate().is_err());
}
