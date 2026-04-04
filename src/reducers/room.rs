use spacetimedb::{Identity, ProcedureContext, ReducerContext, SpacetimeType, Table};

use crate::tables::state::{
    game_room, game_secret, game_state, room_sequence, room_ticket, GameRoom, GameSecret,
    GameState, RoomSequence, RoomStatus, RoomTicket, TerminalStatus, ROOM_GAME_ID_OFFSET,
};

/// Creates a new multiplayer room and allocates a dedicated game-state row for it.
///
/// The caller becomes both the host and Player 1. The reducer returns the generated room id so
/// Android can immediately display or share it.
#[spacetimedb::reducer]
pub fn initiate_room(ctx: &ReducerContext, villain_name: Option<String>) -> Result<(), String> {
    let sequence = ctx.db.room_sequence().insert(RoomSequence {
        room_seq: 0,
        created_at: ctx.timestamp,
    });

    let room_id = format_room_id(sequence.room_seq);
    let game_id = ROOM_GAME_ID_OFFSET + sequence.room_seq;
    let villain = normalize_villain_name(villain_name);

    ctx.db.game_state().insert(GameState {
        game_id,
        player_one: Some(ctx.sender()),
        player_two: None,
        villain_name: villain,
        is_processing_terminal: false,
        active_terminal_request: None,
        terminal_status: TerminalStatus::Idle,
        last_terminal_result: None,
        last_terminal_message: Some("Room initialized and waiting for Player 2".to_string()),
        last_terminal_actor: Some(ctx.sender()),
        updated_at: ctx.timestamp,
    });

    ctx.db.game_secret().insert(GameSecret {
        game_id,
        hidden_answer: String::new(),
        updated_at: ctx.timestamp,
    });

    ctx.db.game_room().insert(GameRoom {
        room_id: room_id.clone(),
        game_id,
        host_identity: ctx.sender(),
        player_one: Some(ctx.sender()),
        player_two: None,
        status: RoomStatus::WaitingForPlayers,
        created_at: ctx.timestamp,
        updated_at: ctx.timestamp,
        terminated_at: None,
    });

    upsert_room_ticket(
        ctx,
        ctx.sender(),
        Some(room_id),
        Some(RoomStatus::WaitingForPlayers),
    );
    Ok(())
}

/// Joins an existing room as Player 2.
///
/// The reducer is idempotent for callers that are already inside the room, which simplifies client
/// reconnect flows.
#[spacetimedb::reducer]
pub fn join_room(ctx: &ReducerContext, room_id: String) -> Result<(), String> {
    let normalized_room_id = normalize_room_id(room_id)?;
    let mut room = load_room(ctx, &normalized_room_id)?;

    if room.status == RoomStatus::Terminated {
        return Err(format!("room {} has already been terminated", room.room_id));
    }

    if room.player_one == Some(ctx.sender()) || room.player_two == Some(ctx.sender()) {
        upsert_room_ticket(ctx, ctx.sender(), Some(room.room_id), Some(room.status));
        return Ok(());
    }

    if room.player_two.is_some() {
        return Err(format!("room {} is already full", room.room_id));
    }

    room.player_two = Some(ctx.sender());
    room.status = RoomStatus::Ready;
    room.updated_at = ctx.timestamp;
    ctx.db.game_room().room_id().update(room.clone());

    let mut game_state = load_game_state(ctx, room.game_id)?;
    game_state.player_one = room.player_one;
    game_state.player_two = room.player_two;
    game_state.last_terminal_message = Some("Player 2 joined the room".to_string());
    game_state.last_terminal_actor = Some(ctx.sender());
    game_state.updated_at = ctx.timestamp;
    ctx.db.game_state().game_id().update(game_state);

    upsert_room_ticket(
        ctx,
        ctx.sender(),
        Some(room.room_id),
        Some(RoomStatus::Ready),
    );
    Ok(())
}

/// Terminates a room while preserving its historical game data for later inspection.
///
/// Only the host may terminate the room. The room row remains available to clients so they can
/// understand that the session was closed instead of silently disappearing.
#[spacetimedb::reducer]
pub fn terminate_room(ctx: &ReducerContext, room_id: String) -> Result<(), String> {
    let normalized_room_id = normalize_room_id(room_id)?;
    let mut room = load_room(ctx, &normalized_room_id)?;

    if room.host_identity != ctx.sender() {
        return Err("only the room host may terminate the room".to_string());
    }

    if room.status == RoomStatus::Terminated {
        return Ok(());
    }

    room.status = RoomStatus::Terminated;
    room.updated_at = ctx.timestamp;
    room.terminated_at = Some(ctx.timestamp);
    ctx.db.game_room().room_id().update(room.clone());

    let mut game_state = load_game_state(ctx, room.game_id)?;
    game_state.is_processing_terminal = false;
    game_state.active_terminal_request = None;
    game_state.last_terminal_result = Some(false);
    game_state.last_terminal_message = Some("Room terminated by host".to_string());
    game_state.last_terminal_actor = Some(ctx.sender());
    game_state.terminal_status = TerminalStatus::Failed;
    game_state.updated_at = ctx.timestamp;
    ctx.db.game_state().game_id().update(game_state);

    upsert_room_ticket(
        ctx,
        ctx.sender(),
        Some(room.room_id),
        Some(RoomStatus::Terminated),
    );
    Ok(())
}

/// Testing-focused room termination path that skips host-only checks.
///
/// This reducer keeps the same room shutdown behavior as `terminate_room`, but it allows
/// any caller to terminate the room, which is useful for simple integration tests.
#[spacetimedb::reducer]
pub fn terminate_room_for_testing(ctx: &ReducerContext, room_id: String) -> Result<(), String> {
    let normalized_room_id = normalize_room_id(room_id)?;
    let mut room = load_room(ctx, &normalized_room_id)?;

    if room.status == RoomStatus::Terminated {
        return Ok(());
    }

    room.status = RoomStatus::Terminated;
    room.updated_at = ctx.timestamp;
    room.terminated_at = Some(ctx.timestamp);
    ctx.db.game_room().room_id().update(room.clone());

    let mut game_state = load_game_state(ctx, room.game_id)?;
    game_state.is_processing_terminal = false;
    game_state.active_terminal_request = None;
    game_state.last_terminal_result = Some(false);
    game_state.last_terminal_message = Some("Room terminated in testing mode".to_string());
    game_state.last_terminal_actor = Some(ctx.sender());
    game_state.terminal_status = TerminalStatus::Failed;
    game_state.updated_at = ctx.timestamp;
    ctx.db.game_state().game_id().update(game_state);

    upsert_room_ticket(
        ctx,
        ctx.sender(),
        Some(room.room_id),
        Some(RoomStatus::Terminated),
    );
    Ok(())
}

/// Result object returned by `get_my_room_info`.
#[derive(Debug, Clone, SpacetimeType)]
pub struct MyRoomInfo {
    pub room_id: Option<String>,
    pub room_status: Option<RoomStatus>,
}

/// Procedure helper that returns the caller's room mapping without SQL.
///
/// Postman can call this via `/call/get_my_room_info` with the caller token to get a stable
/// `room_id` and status directly from `room_ticket`.
#[spacetimedb::procedure]
pub fn get_my_room_info(ctx: &mut ProcedureContext) -> MyRoomInfo {
    let sender = ctx.sender();
    ctx.with_tx(|tx| {
        if let Some(ticket) = tx.db.room_ticket().owner_identity().find(sender) {
            return MyRoomInfo {
                room_id: ticket.room_id,
                room_status: ticket.room_status,
            };
        }

        MyRoomInfo {
            room_id: None,
            room_status: None,
        }
    })
}

/// Resolves a room id to its room row, returning a descriptive error when the room is unknown.
pub fn load_room(ctx: &ReducerContext, room_id: &str) -> Result<GameRoom, String> {
    ctx.db
        .game_room()
        .room_id()
        .find(room_id.to_string())
        .ok_or_else(|| format!("room {} does not exist", room_id))
}

/// Resolves the room-backed game id for subsequent game reducers.
pub fn resolve_room_game_id(ctx: &ReducerContext, room_id: &str) -> Result<u64, String> {
    let room = load_room(ctx, room_id)?;
    if room.status == RoomStatus::Terminated {
        return Err(format!("room {} has already been terminated", room.room_id));
    }
    Ok(room.game_id)
}

fn load_game_state(ctx: &ReducerContext, game_id: u64) -> Result<GameState, String> {
    ctx.db
        .game_state()
        .game_id()
        .find(game_id)
        .ok_or_else(|| format!("game state {} is missing for the room", game_id))
}

fn normalize_room_id(room_id: String) -> Result<String, String> {
    let normalized = room_id.trim().to_uppercase();
    if normalized.is_empty() {
        return Err("room_id must not be empty".to_string());
    }
    Ok(normalized)
}

fn normalize_villain_name(villain_name: Option<String>) -> String {
    villain_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "AI Villain".to_string())
}

fn format_room_id(room_seq: u64) -> String {
    let alphabet = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut value = room_seq;
    let mut code = [b'A'; 6];

    for slot in code.iter_mut().rev() {
        let index = (value % alphabet.len() as u64) as usize;
        *slot = alphabet[index];
        value /= alphabet.len() as u64;
    }

    String::from_utf8(code.to_vec()).unwrap_or_else(|_| format!("ROOM{:06}", room_seq))
}

fn upsert_room_ticket(
    ctx: &ReducerContext,
    owner_identity: Identity,
    room_id: Option<String>,
    room_status: Option<RoomStatus>,
) {
    let ticket = RoomTicket {
        owner_identity,
        room_id,
        room_status,
        updated_at: ctx.timestamp,
    };

    if ctx
        .db
        .room_ticket()
        .owner_identity()
        .find(owner_identity)
        .is_some()
    {
        ctx.db.room_ticket().owner_identity().update(ticket);
    } else {
        ctx.db.room_ticket().insert(ticket);
    }
}

#[allow(dead_code)]
fn _same_identity(left: Option<Identity>, right: Identity) -> bool {
    left == Some(right)
}

/// A convenience reducer for clients to "ping" their own room ticket.
///
/// Because SpaceTimeDB handles connections amorphously, finding a client's own
/// room ID can involve messy SQL parsing to match `spacetime-identity`. 
/// By calling this reducer, the backend simply refreshes the `updated_at` timestamp 
/// on the caller's room ticket. The client SDK's `room_ticket::on_update` callback 
/// will immediately fire with the exact ticket, yielding the `room_id` cleanly.
#[spacetimedb::reducer]
pub fn ping_room_ticket(ctx: &ReducerContext) -> Result<(), String> {
    if let Some(mut ticket) = ctx.db.room_ticket().owner_identity().find(ctx.sender()) {
        ticket.updated_at = ctx.timestamp;
        ctx.db.room_ticket().owner_identity().update(ticket);
    }
    Ok(())
}
