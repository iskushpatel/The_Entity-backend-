# The Entity Backend

Rust + SpacetimeDB backend for an asymmetric multiplayer horror puzzle game with:

- room creation and join flow
- a live persona-driven terminal for Player 1
- async ArmorIQ intent validation
- Gemini-powered clue/manual generation
- Gemini + ElevenLabs villain speech generation
- a 3-minute room timer that starts when Player 2 joins

The backend is designed to run as a SpacetimeDB module compiled to WASM. A small optional local relay is included for local development and smoke testing.

## Live Deployment

- Dashboard: [https://spacetimedb.com/the-entity-ty5fs](https://spacetimedb.com/the-entity-ty5fs)
- HTTP base: [https://maincloud.spacetimedb.com/v1/database/the-entity-ty5fs](https://maincloud.spacetimedb.com/v1/database/the-entity-ty5fs)
- WebSocket subscribe: `wss://maincloud.spacetimedb.com/v1/database/the-entity-ty5fs/subscribe`

All reducer calls use:

```text
POST /v1/database/the-entity-ty5fs/call/<reducer_name>
```

## Core Features

- Room lifecycle
  - `initiate_room`
  - `join_room`
  - `terminate_room`
- Public room/game state for clients
  - `game_room`
  - `room_ticket`
  - `game_state`
- Terminal round setup
  - persona name
  - persona prompt
  - clue sequence
  - forbidden words
  - kill phrase fragment
- Terminal turn flow
  - local forbidden-word strike check
  - ArmorIQ validation
  - Gemini persona reply
  - round completion if the AI says the fragment
- Content generation
  - room-scoped clue/manual generation
  - room-scoped villain speech generation
- Match timer
  - starts automatically when Player 2 joins
  - 180000 ms duration
  - stored in public state for Android countdown rendering

## Project Structure

```text
src/
  api/
    content_http.rs
    http_wrappers.rs
  models/
    api_schemas.rs
  reducers/
    room.rs
    terminal.rs
    content.rs
  tables/
    state.rs
scripts/
  publish-maincloud.ps1
relay/
  server.js
  clients.js
  prompts.js
docs/
  armoriq-setup.md
  spacetimedb-gui-setup.md
```

## Tech Stack

- Rust 2021
- `spacetimedb = 2.1.0`
- `serde`
- `serde_json`
- optional Node relay for local development

## Local Development

### Prerequisites

- Rust + Cargo
- `wasm32-unknown-unknown` target
- SpacetimeDB CLI
- Node.js if you want to run the local relay

### Install toolchain

```powershell
iwr https://windows.spacetimedb.com -UseBasicParsing | iex
& "$env:USERPROFILE\.cargo\bin\rustup.exe" target add wasm32-unknown-unknown
```

### Build checks

```powershell
cd C:\Users\HP\Desktop\The_Entity-backend-
& "$env:USERPROFILE\.cargo\bin\cargo.exe" check
& "$env:USERPROFILE\.cargo\bin\cargo.exe" check --target wasm32-unknown-unknown
```

### Local relay

The relay is optional and mainly useful for local mock/live testing outside Maincloud.

```powershell
cd C:\Users\HP\Desktop\The_Entity-backend-
npm run relay:test
$env:MOCK_MODE="true"
npm run relay:start
```

## Environment Variables

Template file: [.env.example](C:\Users\HP\Desktop\The_Entity-backend-\.env.example)

Important keys:

- `GEMINI_API_KEY`
- `GEMINI_EXPANSION_API_KEY`
- `GEMINI_TERMINAL_API_KEY`
- `ARMORIQ_API_KEY`
- `ARMORIQ_TOKEN_ISSUE_URL`
- `ELEVENLABS_API_KEY`
- `ELEVENLABS_VOICE_ID`

Do not commit `.env`.

## Deploying To Maincloud

Script: [publish-maincloud.ps1](C:\Users\HP\Desktop\The_Entity-backend-\scripts\publish-maincloud.ps1)

### One-time login

```powershell
spacetime login
```

### Publish

```powershell
cd C:\Users\HP\Desktop\The_Entity-backend-
powershell -ExecutionPolicy Bypass -File scripts\publish-maincloud.ps1
```

### Publish without rerunning cargo checks

```powershell
cd C:\Users\HP\Desktop\The_Entity-backend-
powershell -ExecutionPolicy Bypass -File scripts\publish-maincloud.ps1 -SkipChecks
```

## Public Reducers

### Room

- `initiate_room(villain_name: Option<String>)`
  - creates a room and room-scoped `game_state`
- `join_room(room_id: String)`
  - adds Player 2
  - starts the 3-minute timer
- `terminate_room(room_id: String)`
  - host terminates room
- `ping_room_ticket()`
  - refreshes the caller’s `room_ticket`

### Terminal

- `configure_terminal_round_for_room(room_id: String, setup_payload_json: String)`
  - stores persona, clues, forbidden words, kill phrase fragment, strike limits
  - writes the opening terminal line into `game_state.last_terminal_reply`
- `submit_terminal_for_room(room_id: String, input: String)`
  - runs the terminal turn flow
- `set_hidden_answer_for_room(room_id: String, hidden_answer: String)`
  - admin reducer for the room secret
- `configure_integrations(...)`
  - stores ArmorIQ + Gemini backend config
- `configure_terminal_gemini(gemini_terminal_api_key: String, gemini_terminal_model: String)`
  - optional terminal-only Gemini override
- `configure_armoriq_upstream(...)`
  - optional explicit ArmorIQ token issuance config

### Content

- `generate_clue_manual_for_room(room_id: String, round_key: String, request_payload_json: String, response_schema_json: String)`
- `generate_villain_speech_for_room(room_id: String, request_payload_json: String)`
- `configure_voice_integrations(...)`

Internal callback reducers exist but should not be called by clients directly.

## Public Tables Clients Should Read

### `game_room`

Use for room metadata:

- `room_id`
- `game_id`
- `host_identity`
- `player_one`
- `player_two`
- `status`
- `timer_started_at_ms`
- `timer_deadline_at_ms`
- `timer_duration_ms`

### `room_ticket`

Use for “what room am I in?”:

- `owner_identity`
- `room_id`
- `room_status`

### `game_state`

This is the main client-facing runtime state.

Important terminal fields:

- `active_round_key`
- `active_persona_name`
- `terminal_status`
- `is_processing_terminal`
- `last_terminal_reply`
- `last_terminal_message`
- `terminal_strikes`
- `terminal_max_strikes`
- `is_terminal_dead`
- `completed_rounds`
- `revealed_clue_count`

Important timer fields:

- `timer_started_at_ms`
- `timer_deadline_at_ms`
- `timer_duration_ms`
- `timer_remaining_ms`
- `is_game_disqualified`
- `disqualified_at_ms`

### `round_content_artifact`

Use for generated clue/manual payloads.

### `villain_speech_artifact`

Use for villain speech text and optional synthesized audio metadata.

## Timer Behavior

- The match timer starts only when Player 2 joins.
- Timer duration is `180000` milliseconds.
- The backend stores both:
  - start time
  - deadline time
- Android should use `timer_deadline_at_ms` as the authoritative countdown target.
- When the timer expires before all rounds are cleared:
  - `game_state.is_game_disqualified = true`
  - `game_state.timer_remaining_ms = 0`
  - `game_state.disqualified_at_ms` is set
  - `game_room.status = terminated`

Recommended Android countdown:

```text
remainingMs = max(0, timer_deadline_at_ms - System.currentTimeMillis())
```

## Terminal Flow

### 1. Configure the round

Call `configure_terminal_round_for_room`.

Example reducer body:

```json
[
  "ROOM_ID_HERE",
  "{\"round_key\":\"round_1\",\"persona_name\":\"1920s Detective\",\"persona_prompt\":\"You are a weary detective who speaks with suspicion, detail, and controlled impatience.\",\"glitch_tone\":\"cold, tense, noir\",\"kill_phrase_part\":\"glass pilgrims\",\"forbidden_words\":[\"glass\",\"pilgrims\",\"mirror\",\"fragile\",\"cathedral\"],\"clue_lines\":[{\"clue_id\":\"r1_c1\",\"clue_text\":\"The witness swore the procession wore reflections like uniforms.\",\"delivery_style\":\"dry suspicion\"},{\"clue_id\":\"r1_c2\",\"clue_text\":\"The chapel ledger marks a broken pane and twelve sets of wet footprints.\",\"delivery_style\":\"measured accusation\"}],\"max_strikes\":3}"
]
```

What happens:

- round state is stored in `terminal_round_state`
- the boot line is written to `game_state.last_terminal_reply`
- no player input is required for the intro line

### 2. Submit a terminal turn

Call `submit_terminal_for_room`.

Example reducer body:

```json
[
  "ROOM_ID_HERE",
  "Tell me exactly what in the witness account keeps bothering you."
]
```

Turn behavior:

1. local forbidden-word check
2. ArmorIQ validation
3. Gemini persona reply
4. clue progression update
5. round completion check if the AI says the kill phrase fragment

### 3. Read terminal output

Do not expect the reducer response body to contain the spoken line.

Read `game_state.last_terminal_reply`.

Recommended Android read model:

```json
{
  "room_id": "AAAABC",
  "game_id": 10123,
  "round_key": "round_1",
  "persona_name": "1920s Detective",
  "status": "succeeded",
  "processing": false,
  "reply": {
    "speaker": "terminal",
    "text": "That witness gave me a neat story, but neat stories are often lies with polished shoes.",
    "summary": "1920s Detective resists. Next clue primed: r1_c2."
  },
  "progress": {
    "revealed_clue_count": 1,
    "completed_rounds": 0
  },
  "strikes": {
    "current": 0,
    "max": 3,
    "is_dead": false
  },
  "timer": {
    "timer_started_at_ms": 1775339000000,
    "timer_deadline_at_ms": 1775339180000,
    "timer_remaining_ms": 172430,
    "is_game_disqualified": false
  }
}
```

## Clue / Manual Generation

Use `generate_clue_manual_for_room`.

Example request:

```json
[
  "ROOM_ID_HERE",
  "round_1",
  "{\"requested_persona\":\"1920s Detective\"}",
  ""
]
```

The backend currently uses a staged generation flow for richer results:

1. Gemini skeleton extraction
2. Gemini expansion
3. merge into final artifact row

Read the result from `round_content_artifact.response_payload_json`.

## Villain Speech Generation

Use `generate_villain_speech_for_room`.

Example request:

```json
[
  "ROOM_ID_HERE",
  "{\"round_key\":\"round_1\",\"villain_name\":\"The Entity\",\"scene\":\"first clue reveal\",\"tone\":\"cold, superior, controlled\",\"synthesize_audio\":true,\"selected_cue_id\":\"r1_c1\",\"clue_contexts\":[{\"cue_id\":\"r1_c1\",\"clue_text\":\"The witness swore the procession wore reflections like uniforms.\"}],\"round_output\":{\"persona_name\":\"1920s Detective\"}}"
]
```

Read the result from:

- `villain_speech_artifact.speech_cues_json`
- `villain_speech_artifact.audio_base64`
- `villain_speech_artifact.mime_type`

## Postman Notes

- Reducer calls use `Content-Type: application/json`
- SQL reads use `Content-Type: text/plain`
- Reducer bodies are JSON arrays, not objects

Example SQL read:

```sql
select * from game_state
```

## Authentication Notes

There are two different token concepts:

- `spacetime-identity-token`
  - returned from gameplay reducer calls
  - use this for player-scoped calls
- owner token
  - obtained via `spacetime login show --token`
  - use this for admin reducers like `configure_integrations`

## Android Integration Notes

Recommended client flow:

1. call `initiate_room`
2. read `room_ticket` or `game_room` to get `room_id`
3. Player 2 calls `join_room`
4. read `game_room.game_id`
5. configure the terminal round
6. subscribe to or poll `game_state`
7. render:
   - `last_terminal_reply`
   - strike counters
   - timer fields
8. submit terminal turns with `submit_terminal_for_room`
9. read `round_content_artifact` and `villain_speech_artifact` for generation flows

For a smooth countdown, compute time locally from `timer_deadline_at_ms`.

## Useful Commands

### Cargo

```powershell
cargo check
cargo check --target wasm32-unknown-unknown
```

### Publish

```powershell
powershell -ExecutionPolicy Bypass -File scripts\publish-maincloud.ps1
```

### Relay

```powershell
npm run relay:test
npm run relay:start
```

## Existing Docs

- [armoriq-setup.md](C:\Users\HP\Desktop\The_Entity-backend-\docs\armoriq-setup.md)
- [spacetimedb-gui-setup.md](C:\Users\HP\Desktop\The_Entity-backend-\docs\spacetimedb-gui-setup.md)

## Notes

- The backend is optimized around SpacetimeDB’s synchronous reducer model plus scheduled callback flow.
- HTTP work is done through scheduled procedures and reducer callbacks, not async/await inside reducers.
- Timer state is public so Android can render it without reconstructing server logic.
- The terminal writes its spoken output into `game_state.last_terminal_reply`.
