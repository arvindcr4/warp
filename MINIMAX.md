# MiniMax Token Plan & Anthropic-Compatible API Support

## Warp Max: no Warp account, all AI on your own keys

"Warp Max" runs the agent entirely on your own LLM keys with **no Warp login**
and **no calls to app.warp.dev**. This is a deeper change than BYOK: stock Warp
(even with BYOK) sends every agent request to Warp's closed-source server,
which makes the provider call for you and requires authentication. Warp Max
replaces that server with a local one.

### How it works

```
Warp Max client  ──POST /ai/multi-agent (protobuf Request)──►  warp-max-server (localhost:8765)
   (no login)    ◄──── SSE: ResponseEvent protobufs ──────────   (calls YOUR key directly)
```

- The client speaks Warp's multi-agent protobuf protocol unchanged. The
  agentic loop (tool calls → execute locally → re-request) is driven by the
  client exactly as before.
- `warp-max-server` (`crates/warp_local_server`) decodes each `Request`,
  reads the endpoint URL + API key the client ships inside
  `settings.custom_model_providers` (i.e. the custom endpoint you configured),
  calls that OpenAI-compatible Chat Completions endpoint, and streams the
  result back as `ResponseEvent`s. It is **stateless** — one provider turn per
  request — and stores no credentials.
- Supported tools (v1): `run_shell_command`, `read_files`, `apply_file_diffs`.
  More can be added in `crates/warp_local_server/src/tools.rs`.

### Client changes that make it account-free

- `app/src/bin/oss.rs` — server URL points at `http://localhost:8765`
  (override with `WARP_MAX_SERVER_URL`).
- `app/src/root_view.rs` — the Oss build boots straight to the terminal (no
  login/onboarding gate).
- `app/src/workspaces/user_workspaces.rs` — BYOK and custom inference are
  enabled without a Warp account.
- `crates/warp_server_client/src/auth/session.rs` — a missing-credentials
  state yields `NoAuth` instead of failing requests.

### Running it

```bash
# 1. Start the local agent backend
cargo run -p warp_local_server --bin warp-max-server      # listens on 127.0.0.1:8765

# 2. Launch Warp Max (already installed at /Applications/Warp Max.app), then:
#    Settings → AI → Custom inference → + Add custom model → MiniMax Token Plan
#    Paste your sk-cp... key, pick MiniMax-M3, and start chatting in Agent Mode.
```

The server uses the OpenAI-compatible endpoint (MiniMax `/v1`). No key is
stored by the server; it is read from each request and used only for that call.

---

# (Original) MiniMax presets & Anthropic-compatible custom endpoints

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
