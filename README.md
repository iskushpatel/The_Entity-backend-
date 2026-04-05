# The Entity Backend

SpacetimeDB backend for a room-based multiplayer puzzle game.

This project is implemented in Rust and compiled to WASM for the SpacetimeDB runtime. It currently provides:

- room creation and join flow
- public room and game state tables for clients
- a persona-driven terminal flow for Player 1
- ArmorIQ validation before Gemini terminal turns
- room-scoped clue/manual generation
- room-scoped villain speech generation
- an 8-minute room timer that starts when Player 2 joins

## Live Deployment

- Dashboard: [https://spacetimedb.com/the-entity-ty5fs](https://spacetimedb.com/the-entity-ty5fs)
- HTTP base: [https://maincloud.spacetimedb.com/v1/database/the-entity-ty5fs](https://maincloud.spacetimedb.com/v1/database/the-entity-ty5fs)
- WebSocket subscribe: `wss://maincloud.spacetimedb.com/v1/database/the-entity-ty5fs/subscribe`

Reducer calls use:

```text
POST /v1/database/the-entity-ty5fs/call/<reducer_name>
```

## What Is Implemented

### Room Flow

- `initiate_room`
- `join_room`
- `terminate_room`
- `ping_room_ticket`

When Player 2 joins:

- the room becomes ready
- the match timer starts
- timer fields are written into public state

### Terminal Flow

- `configure_terminal_round_for_room`
- `submit_terminal_for_room`
- `set_hidden_answer_for_room`

Behavior implemented in the terminal flow:

- persona setup from JSON payload
- immediate intro line after configuration
- forbidden-word strike tracking
- death after max strikes
- ArmorIQ validation before Gemini turn generation
- Gemini terminal response stored in public `game_state`
- timeout guard during terminal flow

### Content Generation

- `generate_clue_manual_for_room`
- `generate_villain_speech_for_room`
- `configure_voice_integrations`

These reducers write their results into:

- `round_content_artifact`
- `villain_speech_artifact`

### Timer

The timer is implemented as public state plus a scheduled timeout callback.

Current duration:

- `480000` milliseconds
- 8 minutes

Timeout state is exposed in `game_state` so Android can render countdown UI.

## Current Scope Notes

- The terminal setup accepts `round_1` through `round_4`.
- Round-scoped content generation exists, but the most complete prompt/schema path is Round 1.
- The backend includes an optional local relay for development, but the live Maincloud backend runs without needing that relay.

## Project Structure

```text
src/
  api/
    content_http.rs
    http_wrappers.rs
  models/
    api_schemas.rs
  reducers/
    content.rs
    room.rs
    terminal.rs
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
- SpacetimeDB `2.1.0`
- `serde`
- `serde_json`
- optional Node-based relay for local testing

## Local Development

### Prerequisites

- Rust + Cargo
- `wasm32-unknown-unknown` target
- SpacetimeDB CLI
- Node.js if you want to run the local relay

### Install tools

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

### Optional local relay

```powershell
cd C:\Users\HP\Desktop\The_Entity-backend-
npm run relay:test
$env:MOCK_MODE="true"
npm run relay:start
```

## Environment Variables

Template file: [.env.example](C:\Users\HP\Desktop\The_Entity-backend-\.env.example)

Important values used by this project:

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

### Login

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
- `join_room(room_id: String)`
- `terminate_room(room_id: String)`
- `ping_room_ticket()`

### Terminal

- `configure_terminal_round_for_room(room_id: String, setup_payload_json: String)`
- `submit_terminal_for_room(room_id: String, input: String)`
- `set_hidden_answer_for_room(room_id: String, hidden_answer: String)`
- `configure_integrations(...)`
- `configure_terminal_gemini(gemini_terminal_api_key: String, gemini_terminal_model: String)`
- `configure_armoriq_upstream(...)`

Legacy singleton reducers also exist:

- `configure_terminal_round`
- `submit_terminal`
- `set_hidden_answer`

### Content

- `generate_clue_manual_for_room(room_id: String, round_key: String, request_payload_json: String, response_schema_json: String)`
- `generate_villain_speech_for_room(room_id: String, request_payload_json: String)`
- `configure_voice_integrations(...)`

Internal callback reducers exist, but clients should not call them directly.

## Public Tables Clients Should Read

### `game_room`

Room metadata:

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

Used to discover the caller's latest room:

- `owner_identity`
- `room_id`
- `room_status`

### `game_state`

Main client-facing runtime state.

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

Generated clue/manual output:

- `status`
- `request_payload_json`
- `response_payload_json`
- `hidden_answer_candidate`
- `last_error`

### `villain_speech_artifact`

Generated villain speech output:

- `status`
- `speech_cues_json`
- `selected_cue_id`
- `selected_speech_text`
- `audio_base64`
- `mime_type`
- `last_error`

## Timer Behavior

- The timer starts when Player 2 joins.
- Duration is `480000` milliseconds.
- Android should treat `timer_deadline_at_ms` as the authoritative deadline.
- If time expires before the game is cleared:
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

Example body:

```json
[
  "ROOM_ID_HERE",
  "{\"round_key\":\"round_1\",\"persona_name\":\"1920s Detective\",\"persona_prompt\":\"You are a weary detective who speaks with suspicion, detail, and controlled impatience.\",\"glitch_tone\":\"cold, tense, noir\",\"kill_phrase_part\":\"glass pilgrims\",\"forbidden_words\":[\"glass\",\"pilgrims\",\"mirror\",\"fragile\",\"cathedral\"],\"clue_lines\":[{\"clue_id\":\"r1_c1\",\"clue_text\":\"The witness swore the procession wore reflections like uniforms.\",\"delivery_style\":\"dry suspicion\"},{\"clue_id\":\"r1_c2\",\"clue_text\":\"The chapel ledger marks a broken pane and twelve sets of wet footprints.\",\"delivery_style\":\"measured accusation\"}],\"max_strikes\":3}"
]
```

What this does:

- stores terminal round state
- stores clue sequence
- stores forbidden words
- stores the kill phrase fragment
- writes the opening terminal line to `game_state.last_terminal_reply`

### 2. Submit a player turn

Call `submit_terminal_for_room`.

Example body:

```json
[
  "ROOM_ID_HERE",
  "Tell me exactly what in the witness account keeps bothering you."
]
```

Turn path:

1. forbidden-word check
2. ArmorIQ validation
3. Gemini terminal turn
4. terminal reply stored in `game_state`

### 3. Read terminal output

Do not expect the reducer response body to contain the spoken line.

Read:

- `game_state.last_terminal_reply`
- `game_state.last_terminal_message`
- `game_state.terminal_status`

## Content Generation

### Clue / Manual

Use `generate_clue_manual_for_room`.

Example body:

```json
[
  "ROOM_ID_HERE",
  "round_1",
  "{\"requested_persona\":\"1920s Detective\"}",
  ""
]
```

Read the result from `round_content_artifact.response_payload_json`.

### Villain Speech

Use `generate_villain_speech_for_room`.

Example body:

```json
[
  "ROOM_ID_HERE",
  "{\"round_key\":\"round_1\",\"villain_name\":\"The Entity\",\"scene\":\"first clue reveal\",\"tone\":\"cold, superior, controlled\",\"synthesize_audio\":true,\"selected_cue_id\":\"r1_c1\",\"clue_contexts\":[{\"cue_id\":\"r1_c1\",\"clue_text\":\"The witness swore the procession wore reflections like uniforms.\"}],\"round_output\":{\"persona_name\":\"1920s Detective\"}}"
]
```

Read the result from `villain_speech_artifact`.

## Postman Notes

- Reducer calls use `Content-Type: application/json`
- SQL reads use `Content-Type: text/plain`
- Reducer bodies are JSON arrays

Example SQL read:

```sql
select * from game_state
```

## Authentication Notes

Two different token types are commonly used:

- `spacetime-identity-token`
  - returned from gameplay calls
  - used for player-scoped reducer calls
- owner token
  - obtained with `spacetime login show --token`
  - used for admin reducers such as `configure_integrations`

## Android Integration Notes

Recommended client flow:

1. call `initiate_room`
2. read `room_ticket` or `game_room`
3. Player 2 calls `join_room`
4. read `game_room.game_id`
5. configure terminal round
6. subscribe to or poll `game_state`
7. render:
   - `last_terminal_reply`
   - strike count
   - timer fields
8. submit terminal turns with `submit_terminal_for_room`
9. read generation artifacts when needed

For a smooth countdown, compute remaining time locally from `timer_deadline_at_ms`.

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

- The backend uses SpacetimeDB reducers plus scheduled callbacks for remote work.
- HTTP work is not done with async/await inside reducers.
- Timer state is public so Android can render it directly.
- Terminal speech is written into `game_state.last_terminal_reply`.
