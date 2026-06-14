//! Preset definitions for connecting a MiniMax "Token Plan" subscription as a
//! custom inference endpoint.
//!
//! A MiniMax Token Plan exposes the same subscription key (`sk-cp...`) over
//! two wire protocols:
//! - OpenAI-compatible Chat Completions at `https://api.minimax.io/v1`
//! - Anthropic-compatible Messages at `https://api.minimax.io/anthropic`
//!
//! (China-region plans use `api.minimaxi.com` instead of `api.minimax.io`.)
//!
//! Warp's backend speaks OpenAI Chat Completions to custom endpoints, so the
//! OpenAI-compatible URLs work directly; the Anthropic-compatible URLs require
//! routing through the `anthropic-bridge` translation service.

use crate::api_keys::ApiFormat;

pub const MINIMAX_ENDPOINT_NAME: &str = "MiniMax";

pub const MINIMAX_GLOBAL_OPENAI_URL: &str = "https://api.minimax.io/v1";
pub const MINIMAX_GLOBAL_ANTHROPIC_URL: &str = "https://api.minimax.io/anthropic";
pub const MINIMAX_CHINA_OPENAI_URL: &str = "https://api.minimaxi.com/v1";
pub const MINIMAX_CHINA_ANTHROPIC_URL: &str = "https://api.minimaxi.com/anthropic";

/// Default models offered on a MiniMax Token Plan, as (model slug, picker alias).
pub const MINIMAX_DEFAULT_MODELS: &[(&str, &str)] = &[
    ("MiniMax-M3", "MiniMax M3"),
    ("MiniMax-M2.7", "MiniMax M2.7"),
    ("MiniMax-M2.7-highspeed", "MiniMax M2.7 Highspeed"),
];

/// The MiniMax service region. Token Plan keys are region-specific: a plan
/// purchased on the international platform only works against `api.minimax.io`
/// and a China plan only against `api.minimaxi.com`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinimaxRegion {
    Global,
    China,
}

/// The MiniMax base URL for the given region and wire protocol.
pub fn minimax_url(region: MinimaxRegion, api_format: ApiFormat) -> &'static str {
    match (region, api_format) {
        (MinimaxRegion::Global, ApiFormat::OpenAiChatCompletions) => MINIMAX_GLOBAL_OPENAI_URL,
        (MinimaxRegion::Global, ApiFormat::AnthropicMessages) => MINIMAX_GLOBAL_ANTHROPIC_URL,
        (MinimaxRegion::China, ApiFormat::OpenAiChatCompletions) => MINIMAX_CHINA_OPENAI_URL,
        (MinimaxRegion::China, ApiFormat::AnthropicMessages) => MINIMAX_CHINA_ANTHROPIC_URL,
    }
}

/// Everything needed to prefill a custom-endpoint form for a MiniMax Token
/// Plan: the endpoint name, the base URL for the chosen wire format, and the
/// default models. One call gives the UI the whole preset, so it no longer
/// reaches for [`MINIMAX_ENDPOINT_NAME`], [`minimax_url`], and
/// [`MINIMAX_DEFAULT_MODELS`] separately.
pub struct MinimaxPreset {
    pub name: &'static str,
    pub url: &'static str,
    pub models: &'static [(&'static str, &'static str)],
}

/// Builds the MiniMax Token Plan preset for a region and wire protocol.
pub fn minimax_preset(region: MinimaxRegion, api_format: ApiFormat) -> MinimaxPreset {
    MinimaxPreset {
        name: MINIMAX_ENDPOINT_NAME,
        url: minimax_url(region, api_format),
        models: MINIMAX_DEFAULT_MODELS,
    }
}

/// If `url` is a known MiniMax endpoint, returns the equivalent URL in the
/// requested API format so the UI can keep the URL field in sync when the
/// user flips formats. Returns `None` for non-MiniMax URLs (including ones
/// the user has edited), which the caller should leave untouched.
pub fn switch_minimax_url_format(url: &str, api_format: ApiFormat) -> Option<&'static str> {
    let region = match url.trim().trim_end_matches('/') {
        MINIMAX_GLOBAL_OPENAI_URL | MINIMAX_GLOBAL_ANTHROPIC_URL => MinimaxRegion::Global,
        MINIMAX_CHINA_OPENAI_URL | MINIMAX_CHINA_ANTHROPIC_URL => MinimaxRegion::China,
        _ => return None,
    };
    Some(minimax_url(region, api_format))
}

#[cfg(test)]
#[path = "minimax_tests.rs"]
mod tests;
