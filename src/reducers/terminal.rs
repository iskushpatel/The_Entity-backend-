use spacetimedb::{log, Identity, ReducerContext, Table};

use crate::api::http_wrappers::{extract_gemini_text, queue_gemini_validator, verify_with_armoriq};
use crate::models::api_schemas::{ArmorIqResponse, GeminiValidatorDecision};
use crate::tables::state::{
    armoriq_callback_schedule, game_secret, game_state, gemini_validator_callback_schedule,
    terminal_request, ArmoriqCallbackSchedule, GameSecret, GameState,
    GeminiValidatorCallbackSchedule, TerminalRequest, TerminalRequestPhase, TerminalStatus,
    DEFAULT_GAME_ID,
};

/// Trigger reducer for Player 1 terminal submissions.
///
/// The reducer only mutates state and enqueues the external validation workflow; it never
/// performs network I/O directly, which keeps the transaction deterministic and replay-safe.
#[spacetimedb::reducer]
pub fn submit_terminal(ctx: &ReducerContext, input: String) -> Result<(), String> {
    let normalized_input = input.trim().to_string();
    if normalized_input.is_empty() {
        return Err("terminal input must not be empty".to_string());
    }

    let mut game_state = load_or_create_game_state(ctx);
    bind_or_authorize_player_one(ctx.sender(), &mut game_state)?;
    repair_stale_lock_if_needed(ctx, &mut game_state);

    if game_state.is_processing_terminal {
        return Err("terminal validation is already in progress".to_string());
    }

    let hidden_answer = ctx
        .db
        .game_secret()
        .game_id()
        .find(game_state.game_id)
        .map(|secret| secret.hidden_answer)
        .filter(|value: &String| !value.trim().is_empty())
        .ok_or_else(|| {
            format!(
                "hidden answer for game {} is not configured yet",
                game_state.game_id
            )
        })?;

    let request = ctx.db.terminal_request().insert(TerminalRequest {
        request_id: 0,
        game_id: game_state.game_id,
        player_identity: ctx.sender(),
        phase: TerminalRequestPhase::PendingArmorIq,
        player_input: normalized_input.clone(),
        hidden_answer_snapshot: hidden_answer.clone(),
        armoriq_allowed: None,
        armoriq_block_reason: None,
        armoriq_raw_response: None,
        gemini_raw_response: None,
        validator_success: None,
        validator_reason: None,
        created_at: ctx.timestamp,
        updated_at: ctx.timestamp,
    });

    game_state.is_processing_terminal = true;
    game_state.active_terminal_request = Some(request.request_id);
    game_state.terminal_status = TerminalStatus::PendingArmorIq;
    game_state.last_terminal_result = None;
    game_state.last_terminal_message = Some("ArmorIQ validation in progress".to_string());
    game_state.last_terminal_actor = Some(ctx.sender());
    game_state.updated_at = ctx.timestamp;
    ctx.db.game_state().game_id().update(game_state);

    verify_with_armoriq(ctx, request.request_id, normalized_input, hidden_answer)
}

/// Scheduled reducer that consumes the persisted ArmorIQ HTTP result.
#[spacetimedb::reducer]
pub fn _armoriq_callback(
    ctx: &ReducerContext,
    callback: ArmoriqCallbackSchedule,
) -> Result<(), String> {
    if !ctx.sender_auth().is_internal() {
        return Err("_armoriq_callback may only be invoked by the scheduler".to_string());
    }

    ctx.db
        .armoriq_callback_schedule()
        .scheduled_id()
        .delete(&callback.scheduled_id);

    let Some(mut request) = ctx.db.terminal_request().request_id().find(callback.request_id) else {
        clear_state_for_unknown_request(
            ctx,
            callback.request_id,
            "ArmorIQ callback arrived after the request row was removed".to_string(),
        );
        return Ok(());
    };

    let Some(mut game_state) = ctx.db.game_state().game_id().find(request.game_id) else {
        log::warn!(
            "Ignoring ArmorIQ callback for request {} because game {} no longer exists",
            request.request_id,
            request.game_id
        );
        return Ok(());
    };

    if let Some(transport_error) = callback.transport_error.clone() {
        request.armoriq_raw_response = Some(transport_error.clone());
        request.armoriq_allowed = Some(false);
        request.armoriq_block_reason = Some(transport_error.clone());
        request.phase = TerminalRequestPhase::Failed;
        request.validator_success = Some(false);
        request.validator_reason = Some(transport_error.clone());
        request.updated_at = ctx.timestamp;
        ctx.db.terminal_request().request_id().update(request.clone());

        fail_game_state(
            ctx,
            &mut game_state,
            request.request_id,
            request.player_identity,
            TerminalStatus::Failed,
            false,
            format!("ArmorIQ transport error: {transport_error}"),
        );
        return Ok(());
    }

    if callback.status_code != 200 {
        request.armoriq_raw_response = Some(callback.response_body.clone());
        request.armoriq_allowed = Some(false);
        request.armoriq_block_reason =
            Some(format!("ArmorIQ returned HTTP {}", callback.status_code));
        request.phase = TerminalRequestPhase::Failed;
        request.validator_success = Some(false);
        request.validator_reason = Some(format!(
            "ArmorIQ returned non-200 status {}",
            callback.status_code
        ));
        request.updated_at = ctx.timestamp;
        ctx.db.terminal_request().request_id().update(request.clone());

        fail_game_state(
            ctx,
            &mut game_state,
            request.request_id,
            request.player_identity,
            TerminalStatus::Failed,
            false,
            format!("ArmorIQ returned HTTP {}", callback.status_code),
        );
        return Ok(());
    }

    let parsed: ArmorIqResponse = match serde_json::from_str(&callback.response_body) {
        Ok(value) => value,
        Err(err) => {
            request.armoriq_raw_response = Some(callback.response_body.clone());
            request.armoriq_allowed = Some(false);
            request.armoriq_block_reason = Some(err.to_string());
            request.phase = TerminalRequestPhase::Failed;
            request.validator_success = Some(false);
            request.validator_reason = Some(format!("Invalid ArmorIQ JSON: {err}"));
            request.updated_at = ctx.timestamp;
            ctx.db.terminal_request().request_id().update(request.clone());

            fail_game_state(
                ctx,
                &mut game_state,
                request.request_id,
                request.player_identity,
                TerminalStatus::Failed,
                false,
                format!("Invalid ArmorIQ JSON: {err}"),
            );
            return Ok(());
        }
    };

    request.armoriq_raw_response = Some(callback.response_body.clone());
    request.armoriq_allowed = Some(parsed.allowed);
    request.armoriq_block_reason = parsed.block_reason.clone();
    request.updated_at = ctx.timestamp;

    if !parsed.allowed {
        request.phase = TerminalRequestPhase::Rejected;
        request.validator_success = Some(false);
        request.validator_reason = Some(
            parsed
                .block_reason
                .clone()
                .unwrap_or_else(|| "ArmorIQ blocked the terminal request".to_string()),
        );
        ctx.db.terminal_request().request_id().update(request.clone());

        fail_game_state(
            ctx,
            &mut game_state,
            request.request_id,
            request.player_identity,
            TerminalStatus::Rejected,
            false,
            parsed
                .block_reason
                .unwrap_or_else(|| "ArmorIQ blocked the terminal request".to_string()),
        );
        return Ok(());
    }

    request.phase = TerminalRequestPhase::PendingGeminiValidator;
    ctx.db.terminal_request().request_id().update(request.clone());

    game_state.is_processing_terminal = true;
    game_state.active_terminal_request = Some(request.request_id);
    game_state.terminal_status = TerminalStatus::PendingGeminiValidator;
    game_state.last_terminal_result = None;
    game_state.last_terminal_message = Some("Gemini validator in progress".to_string());
    game_state.last_terminal_actor = Some(request.player_identity);
    game_state.updated_at = ctx.timestamp;
    ctx.db.game_state().game_id().update(game_state.clone());

    if let Err(err) = queue_gemini_validator(ctx, request.request_id) {
        if let Some(mut latest_request) = ctx.db.terminal_request().request_id().find(request.request_id) {
            latest_request.phase = TerminalRequestPhase::Failed;
            latest_request.validator_success = Some(false);
            latest_request.validator_reason = Some(err.clone());
            latest_request.updated_at = ctx.timestamp;
            ctx.db.terminal_request().request_id().update(latest_request);
        }

        fail_game_state(
            ctx,
            &mut game_state,
            request.request_id,
            request.player_identity,
            TerminalStatus::Failed,
            false,
            err,
        );
    }

    Ok(())
}

/// Scheduled reducer that consumes the persisted Gemini validator HTTP result.
#[spacetimedb::reducer]
pub fn _gemini_validator_callback(
    ctx: &ReducerContext,
    callback: GeminiValidatorCallbackSchedule,
) -> Result<(), String> {
    if !ctx.sender_auth().is_internal() {
        return Err("_gemini_validator_callback may only be invoked by the scheduler".to_string());
    }

    ctx.db
        .gemini_validator_callback_schedule()
        .scheduled_id()
        .delete(&callback.scheduled_id);

    let Some(mut request) = ctx.db.terminal_request().request_id().find(callback.request_id) else {
        clear_state_for_unknown_request(
            ctx,
            callback.request_id,
            "Gemini validator callback arrived after the request row was removed".to_string(),
        );
        return Ok(());
    };

    let Some(mut game_state) = ctx.db.game_state().game_id().find(request.game_id) else {
        log::warn!(
            "Ignoring Gemini validator callback for request {} because game {} no longer exists",
            request.request_id,
            request.game_id
        );
        return Ok(());
    };

    if let Some(transport_error) = callback.transport_error.clone() {
        request.gemini_raw_response = Some(transport_error.clone());
        request.phase = TerminalRequestPhase::Failed;
        request.validator_success = Some(false);
        request.validator_reason = Some(transport_error.clone());
        request.updated_at = ctx.timestamp;
        ctx.db.terminal_request().request_id().update(request.clone());

        fail_game_state(
            ctx,
            &mut game_state,
            request.request_id,
            request.player_identity,
            TerminalStatus::Failed,
            false,
            format!("Gemini transport error: {transport_error}"),
        );
        return Ok(());
    }

    if callback.status_code != 200 {
        request.gemini_raw_response = Some(callback.response_body.clone());
        request.phase = TerminalRequestPhase::Failed;
        request.validator_success = Some(false);
        request.validator_reason =
            Some(format!("Gemini returned non-200 status {}", callback.status_code));
        request.updated_at = ctx.timestamp;
        ctx.db.terminal_request().request_id().update(request.clone());

        fail_game_state(
            ctx,
            &mut game_state,
            request.request_id,
            request.player_identity,
            TerminalStatus::Failed,
            false,
            format!("Gemini returned HTTP {}", callback.status_code),
        );
        return Ok(());
    }

    let decision: GeminiValidatorDecision =
        match serde_json::from_str(&callback.response_body).or_else(|_| {
            extract_gemini_text(&callback.response_body)
                .and_then(|text| serde_json::from_str(&text).map_err(|err| err.to_string()))
        }) {
            Ok(value) => value,
            Err(err) => {
                request.gemini_raw_response = Some(callback.response_body.clone());
                request.phase = TerminalRequestPhase::Failed;
                request.validator_success = Some(false);
                request.validator_reason = Some(format!("Invalid Gemini decision JSON: {err}"));
                request.updated_at = ctx.timestamp;
                ctx.db.terminal_request().request_id().update(request.clone());

                fail_game_state(
                    ctx,
                    &mut game_state,
                    request.request_id,
                    request.player_identity,
                    TerminalStatus::Failed,
                    false,
                    format!("Invalid Gemini decision JSON: {err}"),
                );
                return Ok(());
            }
        };

    request.gemini_raw_response = Some(callback.response_body.clone());
    request.validator_success = Some(decision.success);
    request.validator_reason = Some(decision.reason.clone());
    request.phase = if decision.success {
        TerminalRequestPhase::Succeeded
    } else {
        TerminalRequestPhase::Failed
    };
    request.updated_at = ctx.timestamp;
    ctx.db.terminal_request().request_id().update(request.clone());

    game_state.is_processing_terminal = false;
    if game_state.active_terminal_request == Some(request.request_id) {
        game_state.active_terminal_request = None;
    }
    game_state.terminal_status = if decision.success {
        TerminalStatus::Succeeded
    } else {
        TerminalStatus::Failed
    };
    game_state.last_terminal_result = Some(decision.success);
    game_state.last_terminal_message = Some(decision.reason);
    game_state.last_terminal_actor = Some(request.player_identity);
    game_state.updated_at = ctx.timestamp;
    ctx.db.game_state().game_id().update(game_state);

    Ok(())
}

fn load_or_create_game_state(ctx: &ReducerContext) -> GameState {
    if let Some(state) = ctx.db.game_state().game_id().find(DEFAULT_GAME_ID) {
        return state;
    }

    ctx.db.game_state().insert(GameState {
        game_id: DEFAULT_GAME_ID,
        player_one: Some(ctx.sender()),
        player_two: None,
        villain_name: "AI Villain".to_string(),
        is_processing_terminal: false,
        active_terminal_request: None,
        terminal_status: TerminalStatus::Idle,
        last_terminal_result: None,
        last_terminal_message: None,
        last_terminal_actor: None,
        updated_at: ctx.timestamp,
    })
}

fn bind_or_authorize_player_one(sender: Identity, state: &mut GameState) -> Result<(), String> {
    match state.player_one {
        Some(existing) if existing != sender => {
            Err("only Player 1 may submit terminal commands".to_string())
        }
        Some(_) => Ok(()),
        None => {
            state.player_one = Some(sender);
            Ok(())
        }
    }
}

fn repair_stale_lock_if_needed(ctx: &ReducerContext, state: &mut GameState) {
    let is_stale = state.is_processing_terminal
        && state
            .active_terminal_request
            .and_then(|request_id| ctx.db.terminal_request().request_id().find(request_id))
            .is_none();

    if is_stale {
        state.is_processing_terminal = false;
        state.active_terminal_request = None;
        state.terminal_status = TerminalStatus::Idle;
        state.last_terminal_message = Some(
            "Recovered from a stale terminal lock left by a missing request".to_string(),
        );
        state.updated_at = ctx.timestamp;
        ctx.db.game_state().game_id().update(state.clone());
    }
}

fn fail_game_state(
    ctx: &ReducerContext,
    state: &mut GameState,
    request_id: u64,
    actor: Identity,
    status: TerminalStatus,
    result: bool,
    message: String,
) {
    state.is_processing_terminal = false;
    if state.active_terminal_request == Some(request_id) {
        state.active_terminal_request = None;
    }
    state.terminal_status = status;
    state.last_terminal_result = Some(result);
    state.last_terminal_message = Some(message);
    state.last_terminal_actor = Some(actor);
    state.updated_at = ctx.timestamp;
    ctx.db.game_state().game_id().update(state.clone());
}

fn clear_state_for_unknown_request(ctx: &ReducerContext, request_id: u64, message: String) {
    for mut state in ctx.db.game_state().iter() {
        if state.active_terminal_request == Some(request_id) {
            state.is_processing_terminal = false;
            state.active_terminal_request = None;
            state.terminal_status = TerminalStatus::Failed;
            state.last_terminal_result = Some(false);
            state.last_terminal_message = Some(message.clone());
            state.updated_at = ctx.timestamp;
            ctx.db.game_state().game_id().update(state);
        }
    }
}

#[allow(dead_code)]
fn _store_hidden_answer(ctx: &ReducerContext, hidden_answer: String) {
    let row = GameSecret {
        game_id: DEFAULT_GAME_ID,
        hidden_answer,
        updated_at: ctx.timestamp,
    };

    if ctx.db.game_secret().game_id().find(DEFAULT_GAME_ID).is_some() {
        ctx.db.game_secret().game_id().update(row);
    } else {
        ctx.db.game_secret().insert(row);
    }
}
