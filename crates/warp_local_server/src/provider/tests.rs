use super::*;
use warp_multi_agent_api as api;

/// A request whose settings carry a single custom provider+model, with
/// `model_config.base` set to `selected` (the picked model's `config_key`).
fn request_with_custom(
    base_url: &str,
    api_key: &str,
    slug: &str,
    config_key: &str,
    selected: &str,
) -> api::Request {
    api::Request {
        settings: Some(api::request::Settings {
            model_config: Some(api::request::settings::ModelConfig {
                base: selected.to_string(),
                ..Default::default()
            }),
            custom_model_providers: Some(api::request::settings::CustomModelProviders {
                providers: vec![
                    api::request::settings::custom_model_providers::CustomModelProvider {
                        base_url: base_url.to_string(),
                        api_key: api_key.to_string(),
                        models: vec![
                            api::request::settings::custom_model_providers::CustomModel {
                                slug: slug.to_string(),
                                config_key: config_key.to_string(),
                            },
                        ],
                    },
                ],
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn resolve_matches_config_key() {
    let req = request_with_custom(
        "https://api.minimax.io/v1",
        "sk-cp-test",
        "MiniMax-M3",
        "cfg-key-123",
        "cfg-key-123",
    );
    let provider = resolve(&req, None).expect("should resolve");
    assert_eq!(provider.base_url, "https://api.minimax.io/v1");
    assert_eq!(provider.api_key, "sk-cp-test");
    assert_eq!(provider.model, "MiniMax-M3");
}

#[test]
fn resolve_rejects_empty_api_key() {
    // The custom-match path returns before the env fallback, so this is
    // deterministic regardless of the process environment.
    let req = request_with_custom("https://api.minimax.io/v1", "", "MiniMax-M3", "k", "k");
    let err = resolve(&req, None).expect_err("empty key is rejected");
    assert_eq!(
        err,
        ConfigError::EmptyApiKey {
            base_url: "https://api.minimax.io/v1".to_string(),
            model: "MiniMax-M3".to_string(),
        }
    );
    assert!(err.message().contains("No API key"));
}

#[test]
fn resolve_rejects_empty_base_url() {
    let req = request_with_custom("", "sk-x", "MiniMax-M3", "k", "k");
    let err = resolve(&req, None).expect_err("empty url is rejected");
    assert_eq!(err, ConfigError::EmptyBaseUrl);
    assert!(err.message().contains("empty URL"));
}
