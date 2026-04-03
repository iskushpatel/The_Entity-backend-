# ArmorIQ Integration Setup

This repo keeps the game module in Rust and uses ArmorIQ as an external HTTP policy service.

## Architecture

1. `submit_terminal` in the Rust module stores a terminal request.
2. Rust schedules an outbound ArmorIQ verification call.
3. ArmorIQ responds with `{ "allowed": boolean, "block_reason": string | null }`.
4. If allowed, Rust schedules Gemini terminal validation.
5. If blocked, Rust rejects the request without invoking Gemini.

The relevant code lives in:

- `src/reducers/terminal.rs`
- `src/api/http_wrappers.rs`
- `relay/server.js`

## Local Development

Start the relay in mock mode:

```powershell
$env:MOCK_MODE="true"
npm run relay:start
```

Then configure the SpacetimeDB module to use the local relay for both ArmorIQ and Gemini terminal validation:

- reducer: `configure_local_dev_integrations(relay_base_url, armoriq_api_key)`
- example values:
  - `relay_base_url`: `http://127.0.0.1:8787`
  - `armoriq_api_key`: empty string is fine for local mock mode

After that, seed the hidden answer:

- reducer: `set_hidden_answer(hidden_answer)`

Then terminal submissions will follow this path:

- Rust reducer -> ArmorIQ external API call -> Rust callback -> Gemini validator hop

## Production Configuration

Use the full reducer:

- `configure_integrations(...)`

Fill these values:

- `armoriq_verify_url`
- `armoriq_api_key_header`
- `armoriq_api_key`
- `local_llm_relay_base_url` if you want Gemini validation to go through the local relay
- `gemini_api_base_url` and `gemini_api_key` if Rust should call Gemini directly
- `gemini_validator_model`
- `gemini_clue_generator_model`
- `gemini_villain_model`

## Relay Environment

The relay reads:

- `ARMORIQ_VERIFY_URL`
- `ARMORIQ_API_KEY_HEADER`
- `ARMORIQ_API_KEY`
- `GEMINI_API_KEY`
- `GEMINI_*_MODEL`
- `ELEVENLABS_*`

See `.env.example` for the current placeholders.

## Live Testing Design

For live testing, keep the two ArmorIQ URLs separate:

- `ARMORIQ_VERIFY_URL`
  Rust module calls this.
  In this repo it should usually stay `http://127.0.0.1:8787/api/armoriq/verify`.

- `ARMORIQ_UPSTREAM_VERIFY_URL`
  The relay calls this next.
  This must be the real ArmorIQ service endpoint or your hosted ArmorIQ adapter.
  It must not point back to the local relay.

Suggested live `.env` shape:

```env
RELAY_HOST=127.0.0.1
RELAY_PORT=8787
MOCK_MODE=false

GEMINI_API_KEY=your_real_gemini_key
GEMINI_CLUE_MODEL=gemini-2.5-flash
GEMINI_VALIDATOR_MODEL=gemini-2.5-flash
GEMINI_VILLAIN_MODEL=gemini-2.5-flash

ARMORIQ_VERIFY_URL=http://127.0.0.1:8787/api/armoriq/verify
ARMORIQ_UPSTREAM_VERIFY_URL=https://your-real-armoriq-endpoint
ARMORIQ_TOKEN_ISSUE_URL=https://your-real-armoriq-token-endpoint
ARMORIQ_API_KEY_HEADER=x-api-key
ARMORIQ_API_KEY=your_real_armoriq_key
ARMORIQ_USER_ID=your_user_id
ARMORIQ_AGENT_ID=your_agent_id
```

The relay now auto-loads `.env` and `.env.local`.

Live smoke tests:

- `npm run relay:smoke:gemini`
- `npm run relay:smoke:armoriq`
- `npm run relay:smoke:live`
