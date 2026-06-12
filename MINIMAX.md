# MiniMax Token Plan & Anthropic-Compatible API Support

This fork adds first-class support for using a **MiniMax Token Plan**
subscription (and, more generally, any Anthropic-compatible Messages API) as a
custom inference endpoint for Warp's agent.

## What was added

1. **MiniMax Token Plan quick setup** — `Settings → AI → Custom inference →
   + Add custom model` now has "MiniMax Token Plan" / "MiniMax Token Plan
   (China)" preset buttons that prefill the endpoint name, URL, and the default
   Token Plan models (`MiniMax-M3`, `MiniMax-M2.7`, `MiniMax-M2.7-highspeed`).
   You only paste your subscription key (`sk-cp...`).

2. **API format selection per custom endpoint** — each custom endpoint can now
   be marked as `OpenAI Chat Completions` (default, what stock Warp supports)
   or `Anthropic Messages`. Flipping the format on a known MiniMax URL
   automatically swaps between `/v1` and `/anthropic`.

3. **`anthropic-bridge`** (`crates/anthropic_bridge`) — a small self-hostable
   service that exposes an OpenAI-compatible Chat Completions surface and
   forwards translated requests (including SSE streaming, tools, and
   interleaved thinking) to any Anthropic-compatible Messages endpoint. The
   bridge stores no credentials: Warp's backend forwards your API key on every
   request and the bridge relays it to the target.

## Why a bridge is needed for Anthropic-format endpoints

The Warp client does not call model providers directly: agent requests go to
Warp's backend, which makes the provider call using the endpoint URL + API key
the client ships with each request (`CustomModelProviders` in
`warp_multi_agent_api`). That backend only speaks the **OpenAI Chat
Completions** protocol to custom endpoints, and its wire format has no
protocol field — so a pure client-side change cannot make it speak Anthropic
Messages.

For an `Anthropic Messages` endpoint with a bridge URL configured, the client
instead registers `{bridge}/a/{base64url(endpoint URL)}` as the endpoint.
Warp's backend then POSTs OpenAI-format requests to the bridge, which
translates them to `{endpoint URL}/v1/messages` and translates the response
(or SSE stream) back.

**MiniMax note:** a Token Plan key works over *both* protocols
(`https://api.minimax.io/v1` is OpenAI-compatible), so MiniMax works with no
bridge at all using the default preset. Use the Anthropic format + bridge if
you want Anthropic-API-specific behavior (e.g. interleaved thinking blocks),
or for providers that only offer an Anthropic-compatible API.

## Setting up MiniMax (no bridge, recommended)

1. Subscribe to a MiniMax Coding/Token Plan and copy the subscription key
   (`sk-cp...`) from <https://platform.minimax.io> (international) or
   <https://platform.minimaxi.com> (China).
2. In Warp: `Settings → AI → Custom inference → + Add custom model`.
3. Click **MiniMax Token Plan** (or **MiniMax Token Plan (China)**).
4. Paste your key, click **Add endpoint**.
5. Pick a MiniMax model in the model picker. Requests are billed to your Token
   Plan, not Warp credits.

Note: custom inference requires being logged in to Warp and the
`CustomInferenceEndpoints` feature flag; keys are stored in the OS keychain.

## Running the bridge (Anthropic format)

The bridge must be reachable **from the internet over HTTPS** (Warp's backend
is the caller, not your machine). Run it behind a reverse proxy:

```bash
cargo build --release -p anthropic_bridge
./target/release/anthropic-bridge 127.0.0.1:8744
```

Caddy example (automatic TLS):

```
bridge.example.com {
    reverse_proxy 127.0.0.1:8744
}
```

Then in the endpoint modal choose `Anthropic Messages`, set the endpoint URL
to the Anthropic-compatible base (e.g. `https://api.minimax.io/anthropic`) and
the bridge URL to `https://bridge.example.com`. The bridge never logs or
stores keys; it only relays the `Authorization` header it receives.

Health check: `GET /healthz` → `ok`.

## Code map

- `crates/ai/src/api_keys.rs` — `ApiFormat`, `CustomEndpoint::{api_format,
  anthropic_bridge_url}`, `request_base_url()` (bridged URL construction).
- `crates/ai/src/minimax.rs` — preset URLs, default models, format switching.
- `app/src/settings_view/custom_inference_modal.rs` — preset buttons, API
  format toggle, bridge URL field.
- `crates/anthropic_bridge/` — the translation service
  (`src/translate.rs` has the protocol mapping; unit-tested).
