use super::*;

#[test]
fn minimax_url_covers_all_region_format_pairs() {
    assert_eq!(
        minimax_url(MinimaxRegion::Global, ApiFormat::OpenAiChatCompletions),
        "https://api.minimax.io/v1"
    );
    assert_eq!(
        minimax_url(MinimaxRegion::Global, ApiFormat::AnthropicMessages),
        "https://api.minimax.io/anthropic"
    );
    assert_eq!(
        minimax_url(MinimaxRegion::China, ApiFormat::OpenAiChatCompletions),
        "https://api.minimaxi.com/v1"
    );
    assert_eq!(
        minimax_url(MinimaxRegion::China, ApiFormat::AnthropicMessages),
        "https://api.minimaxi.com/anthropic"
    );
}

#[test]
fn switch_format_maps_known_urls() {
    assert_eq!(
        switch_minimax_url_format("https://api.minimax.io/v1", ApiFormat::AnthropicMessages),
        Some("https://api.minimax.io/anthropic")
    );
    assert_eq!(
        switch_minimax_url_format(
            "https://api.minimaxi.com/anthropic",
            ApiFormat::OpenAiChatCompletions
        ),
        Some("https://api.minimaxi.com/v1")
    );
}

#[test]
fn switch_format_tolerates_whitespace_and_trailing_slash() {
    assert_eq!(
        switch_minimax_url_format(" https://api.minimax.io/v1/ ", ApiFormat::AnthropicMessages),
        Some("https://api.minimax.io/anthropic")
    );
}

#[test]
fn switch_format_ignores_unknown_urls() {
    assert_eq!(
        switch_minimax_url_format("https://api.example.com/v1", ApiFormat::AnthropicMessages),
        None
    );
    assert_eq!(
        switch_minimax_url_format("", ApiFormat::AnthropicMessages),
        None
    );
}
