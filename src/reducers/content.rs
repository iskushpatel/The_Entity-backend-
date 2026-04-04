use serde_json::{json, Value};
use spacetimedb::{log, Identity, ReducerContext, Table};

use crate::api::content_http::{
    default_round_response_schema, normalize_round_key, parse_round_generation_result,
    queue_round_generation_request, queue_villain_speech_request, queue_villain_tts_request,
};
use crate::api::http_wrappers::extract_gemini_text;
use crate::models::api_schemas::{
    VillainSpeechCue, VillainSpeechGeminiResponse, VillainSpeechGenerationPayload,
};
use crate::reducers::room::load_room;
use crate::tables::state::{
    module_owner, round_content_artifact, round_generation_callback_schedule,
    round_generation_request, villain_speech_artifact, villain_speech_callback_schedule,
    villain_speech_request, villain_tts_callback_schedule, voice_config, GenerationStatus,
    RoomStatus, RoundContentArtifact, RoundGenerationCallbackSchedule, RoundGenerationPhase,
    RoundGenerationRequest, VillainSpeechArtifact, VillainSpeechCallbackSchedule,
    VillainSpeechPhase, VillainSpeechRequest, VillainTtsCallbackSchedule, VoiceConfig,
    ACTIVE_VOICE_CONFIG_KEY, MODULE_OWNER_KEY,
};

/// Triggers a room-scoped clue/manual generation request for the requested round.
///
/// `request_payload_json` is a flexible JSON object supplied by the Android client and should
/// contain the round-specific prompt inputs. `response_schema_json` can be left empty to use the
/// backend's default schema for the round, or supplied explicitly to enforce a custom contract.
#[spacetimedb::reducer]
pub fn generate_clue_manual_for_room(
    ctx: &ReducerContext,
    room_id: String,
    round_key: String,
    request_payload_json: String,
    response_schema_json: String,
) -> Result<(), String> {
    let normalized_room_id = normalize_room_id(room_id)?;
    let room = load_room(ctx, &normalized_room_id)?;
    ensure_room_is_active(&room)?;
    ensure_generation_room_access(&room)?;

    let normalized_round_key = normalize_round_key(&round_key)?;
    let normalized_payload_json =
        normalize_json_object("request_payload_json", request_payload_json)?;
    let normalized_schema_json =
        normalize_round_schema_json(&normalized_round_key, response_schema_json)?;

    let artifact_key = format_round_artifact_key(&room.room_id, &normalized_round_key);
    repair_round_artifact_if_stale(ctx, &artifact_key);

    if let Some(existing) = ctx
        .db
        .round_content_artifact()
        .artifact_key()
        .find(artifact_key.clone())
    {
        if matches!(
            existing.status,
            GenerationStatus::PendingGemini | GenerationStatus::PendingTts
        ) {
            return Err(format!(
                "round content generation is already in progress for room {} {}",
                room.room_id, normalized_round_key
            ));
        }
    }

    let request = ctx
        .db
        .round_generation_request()
        .insert(RoundGenerationRequest {
            request_id: 0,
            artifact_key: artifact_key.clone(),
            room_id: room.room_id.clone(),
            game_id: room.game_id,
            round_key: normalized_round_key.clone(),
            player_identity: ctx.sender(),
            request_payload_json: normalized_payload_json.clone(),
            response_schema_json: normalized_schema_json,
            phase: RoundGenerationPhase::PendingGeminiSkeleton,
            skeleton_payload_json: None,
            response_payload_json: None,
            hidden_answer_candidate: None,
            error_message: None,
            created_at: ctx.timestamp,
            updated_at: ctx.timestamp,
            retries: Some(0),
        });

    upsert_round_content_artifact(
        ctx,
        RoundContentArtifact {
            artifact_key: artifact_key.clone(),
            room_id: room.room_id,
            game_id: room.game_id,
            round_key: normalized_round_key,
            status: GenerationStatus::PendingGemini,
            request_payload_json: normalized_payload_json,
            response_payload_json: None,
            hidden_answer_candidate: None,
            active_request_id: Some(request.request_id),
            last_error: None,
            updated_at: ctx.timestamp,
        },
    );

    queue_round_generation_request(ctx, request.request_id)
}

/// Triggers a room-scoped villain speech generation request based on a flexible JSON payload.
///
/// The payload may include round context, clue context, tone, scene, selected cue preference,
/// and optional TTS settings such as `synthesize_audio`, `voice_id`, and `voice_model_id`.
#[spacetimedb::reducer]
pub fn generate_villain_speech_for_room(
    ctx: &ReducerContext,
    room_id: String,
    request_payload_json: String,
) -> Result<(), String> {
    let normalized_room_id = normalize_room_id(room_id)?;
    let room = load_room(ctx, &normalized_room_id)?;
    ensure_room_is_active(&room)?;
    ensure_generation_room_access(&room)?;

    let payload_json = normalize_json_object("request_payload_json", request_payload_json)?;
    let payload_value: Value = serde_json::from_str(&payload_json)
        .map_err(|err| format!("invalid request_payload_json: {err}"))?;
    let round_key = extract_optional_round_key(&payload_value)?;
    let artifact_key = format_villain_artifact_key(&room.room_id, round_key.as_deref());

    repair_villain_artifact_if_stale(ctx, &artifact_key);

    if let Some(existing) = ctx
        .db
        .villain_speech_artifact()
        .artifact_key()
        .find(artifact_key.clone())
    {
        if matches!(
            existing.status,
            GenerationStatus::PendingGemini | GenerationStatus::PendingTts
        ) {
            return Err(format!(
                "villain speech generation is already in progress for room {} scope {}",
                room.room_id, artifact_key
            ));
        }
    }

    let request = ctx
        .db
        .villain_speech_request()
        .insert(VillainSpeechRequest {
            request_id: 0,
            artifact_key: artifact_key.clone(),
            room_id: room.room_id.clone(),
            game_id: room.game_id,
            player_identity: ctx.sender(),
            round_key: round_key.clone(),
            request_payload_json: payload_json.clone(),
            phase: VillainSpeechPhase::PendingGemini,
            speech_cues_json: None,
            selected_cue_id: None,
            selected_speech_text: None,
            audio_base64: None,
            mime_type: None,
            tts_provider: None,
            error_message: None,
            created_at: ctx.timestamp,
            updated_at: ctx.timestamp,
            retries: Some(0),
        });

    upsert_villain_speech_artifact(
        ctx,
        VillainSpeechArtifact {
            artifact_key,
            room_id: room.room_id,
            game_id: room.game_id,
            round_key,
            status: GenerationStatus::PendingGemini,
            request_payload_json: payload_json,
            speech_cues_json: None,
            selected_cue_id: None,
            selected_speech_text: None,
            audio_base64: None,
            mime_type: None,
            tts_provider: None,
            active_request_id: Some(request.request_id),
            last_error: None,
            updated_at: ctx.timestamp,
        },
    );

    queue_villain_speech_request(ctx, request.request_id)
}

/// Stores the optional ElevenLabs configuration used by villain speech generation.
#[spacetimedb::reducer]
pub fn configure_voice_integrations(
    ctx: &ReducerContext,
    elevenlabs_api_base_url: String,
    elevenlabs_api_key: String,
    elevenlabs_default_voice_id: String,
    elevenlabs_default_model_id: String,
) -> Result<(), String> {
    ensure_module_owner(ctx)?;

    let config = VoiceConfig {
        config_key: ACTIVE_VOICE_CONFIG_KEY,
        elevenlabs_api_base_url: require_trimmed(
            "elevenlabs_api_base_url",
            elevenlabs_api_base_url,
        )?,
        elevenlabs_api_key: require_trimmed("elevenlabs_api_key", elevenlabs_api_key)?,
        elevenlabs_default_voice_id: require_trimmed(
            "elevenlabs_default_voice_id",
            elevenlabs_default_voice_id,
        )?,
        elevenlabs_default_model_id: require_trimmed(
            "elevenlabs_default_model_id",
            elevenlabs_default_model_id,
        )?,
    };

    if ctx
        .db
        .voice_config()
        .config_key()
        .find(ACTIVE_VOICE_CONFIG_KEY)
        .is_some()
    {
        ctx.db.voice_config().config_key().update(config);
    } else {
        ctx.db.voice_config().insert(config);
    }

    Ok(())
}

/// Scheduled reducer that finalizes the room-scoped clue/manual generation hop.
#[spacetimedb::reducer]
pub fn _round_content_callback(
    ctx: &ReducerContext,
    callback: RoundGenerationCallbackSchedule,
) -> Result<(), String> {
    if !ctx.sender_auth().is_internal() {
        return Err("_round_content_callback may only be invoked by the scheduler".to_string());
    }

    ctx.db
        .round_generation_callback_schedule()
        .scheduled_id()
        .delete(&callback.scheduled_id);

    let Some(mut request) = ctx
        .db
        .round_generation_request()
        .request_id()
        .find(callback.request_id)
    else {
        clear_round_artifact_for_unknown_request(
            ctx,
            callback.request_id,
            "round content callback arrived after the request row was removed".to_string(),
        );
        return Ok(());
    };

    let Some(mut artifact) = ctx
        .db
        .round_content_artifact()
        .artifact_key()
        .find(request.artifact_key.clone())
    else {
        log::warn!(
            "Ignoring round content callback for request {} because artifact {} no longer exists",
            request.request_id,
            request.artifact_key
        );
        return Ok(());
    };

    if let Some(transport_error) = callback.transport_error.clone() {
        request.phase = RoundGenerationPhase::Failed;
        request.error_message = Some(transport_error.clone());
        request.updated_at = ctx.timestamp;
        ctx.db
            .round_generation_request()
            .request_id()
            .update(request.clone());

        fail_round_artifact(ctx, &mut artifact, request.request_id, transport_error);
        return Ok(());
    }

    let current_retries = request.retries.unwrap_or(0);
    if callback.status_code == 429 && current_retries < 3 {
        request.retries = Some(current_retries + 1);
        request.updated_at = ctx.timestamp;
        ctx.db
            .round_generation_request()
            .request_id()
            .update(request.clone());

        // Queue another identical request. It will be picked up immediately
        // but retry attempts provide a small safety net against transient 429s.
        queue_round_generation_request(ctx, request.request_id).ok();
        return Ok(());
    }

    if callback.status_code != 200 {
        let error_message =
            summarize_gemini_http_error(callback.status_code, &callback.response_body);
        request.phase = RoundGenerationPhase::Failed;
        request.error_message = Some(error_message.clone());
        request.updated_at = ctx.timestamp;
        ctx.db
            .round_generation_request()
            .request_id()
            .update(request.clone());

        fail_round_artifact(ctx, &mut artifact, request.request_id, error_message);
        return Ok(());
    }

    let response_value = match parse_round_generation_result(&callback.response_body) {
        Ok(value) => value,
        Err(err) => {
            let error_message = format!("Invalid round content JSON: {err}");
            request.phase = RoundGenerationPhase::Failed;
            request.error_message = Some(error_message.clone());
            request.updated_at = ctx.timestamp;
            ctx.db
                .round_generation_request()
                .request_id()
                .update(request.clone());

            fail_round_artifact(ctx, &mut artifact, request.request_id, error_message);
            return Ok(());
        }
    };

    let response_payload_json = serde_json::to_string(&response_value)
        .map_err(|err| format!("failed to serialize round content response: {err}"))?;

    // Handle two-stage expansion state machine for Round 1
    if request.round_key == "round_1" && request.phase == RoundGenerationPhase::PendingGeminiSkeleton {
        request.phase = RoundGenerationPhase::PendingGeminiExpansion;
        request.skeleton_payload_json = Some(response_payload_json.clone());
        request.retries = Some(0);
        request.error_message = None;
        request.updated_at = ctx.timestamp;
        ctx.db
            .round_generation_request()
            .request_id()
            .update(request.clone());

        // Re-queue the request for the expansion phase
        queue_round_generation_request(ctx, request.request_id).ok();
        return Ok(());
    }

    let final_response_value = if request.round_key == "round_1"
        && request.phase == RoundGenerationPhase::PendingGeminiExpansion
    {
        let skeleton_json = request.skeleton_payload_json.as_deref().ok_or_else(|| {
            format!(
                "round 1 expansion finished without a stored skeleton for request {}",
                request.request_id
            )
        })?;
        compose_round_one_final_payload(skeleton_json, response_value)?
    } else {
        response_value
    };

    let hidden_answer_candidate = extract_hidden_answer_candidate(&final_response_value);

    let final_response_payload_json = serde_json::to_string(&final_response_value)
        .map_err(|err| format!("failed to serialize final round content response: {err}"))?;

    request.phase = RoundGenerationPhase::Succeeded;
    request.response_payload_json = Some(final_response_payload_json.clone());
    request.hidden_answer_candidate = hidden_answer_candidate.clone();
    request.error_message = None;
    request.updated_at = ctx.timestamp;
    ctx.db
        .round_generation_request()
        .request_id()
        .update(request.clone());

    artifact.status = GenerationStatus::Succeeded;
    artifact.response_payload_json = Some(final_response_payload_json);
    artifact.hidden_answer_candidate = hidden_answer_candidate;
    if artifact.active_request_id == Some(request.request_id) {
        artifact.active_request_id = None;
    }
    artifact.last_error = None;
    artifact.updated_at = ctx.timestamp;
    ctx.db
        .round_content_artifact()
        .artifact_key()
        .update(artifact);

    Ok(())
}

/// Scheduled reducer that finalizes the villain speech Gemini hop and optionally queues TTS.
#[spacetimedb::reducer]
pub fn _villain_speech_callback(
    ctx: &ReducerContext,
    callback: VillainSpeechCallbackSchedule,
) -> Result<(), String> {
    if !ctx.sender_auth().is_internal() {
        return Err("_villain_speech_callback may only be invoked by the scheduler".to_string());
    }

    ctx.db
        .villain_speech_callback_schedule()
        .scheduled_id()
        .delete(&callback.scheduled_id);

    let Some(mut request) = ctx
        .db
        .villain_speech_request()
        .request_id()
        .find(callback.request_id)
    else {
        clear_villain_artifact_for_unknown_request(
            ctx,
            callback.request_id,
            "villain speech callback arrived after the request row was removed".to_string(),
        );
        return Ok(());
    };

    let Some(mut artifact) = ctx
        .db
        .villain_speech_artifact()
        .artifact_key()
        .find(request.artifact_key.clone())
    else {
        log::warn!(
            "Ignoring villain speech callback for request {} because artifact {} no longer exists",
            request.request_id,
            request.artifact_key
        );
        return Ok(());
    };

    if let Some(transport_error) = callback.transport_error.clone() {
        request.phase = VillainSpeechPhase::Failed;
        request.error_message = Some(transport_error.clone());
        request.updated_at = ctx.timestamp;
        ctx.db
            .villain_speech_request()
            .request_id()
            .update(request.clone());

        fail_villain_artifact(ctx, &mut artifact, request.request_id, transport_error);
        return Ok(());
    }

    let current_retries = request.retries.unwrap_or(0);
    if callback.status_code == 429 && current_retries < 3 {
        request.retries = Some(current_retries + 1);
        request.phase = VillainSpeechPhase::PendingGemini;
        request.updated_at = ctx.timestamp;
        ctx.db
            .villain_speech_request()
            .request_id()
            .update(request.clone());

        queue_villain_speech_request(ctx, request.request_id).ok();
        return Ok(());
    }

    if callback.status_code != 200 {
        let error_message = format!("Gemini returned HTTP {}", callback.status_code);
        request.phase = VillainSpeechPhase::Failed;
        request.error_message = Some(error_message.clone());
        request.updated_at = ctx.timestamp;
        ctx.db
            .villain_speech_request()
            .request_id()
            .update(request.clone());

        fail_villain_artifact(ctx, &mut artifact, request.request_id, error_message);
        return Ok(());
    }

    let parsed: VillainSpeechGeminiResponse = match serde_json::from_str(&callback.response_body)
        .or_else(|_| {
            extract_gemini_text(&callback.response_body)
                .and_then(|text| serde_json::from_str(&text).map_err(|err| err.to_string()))
        }) {
        Ok(value) => value,
        Err(err) => {
            let error_message = format!("Invalid villain speech JSON: {err}");
            request.phase = VillainSpeechPhase::Failed;
            request.error_message = Some(error_message.clone());
            request.updated_at = ctx.timestamp;
            ctx.db
                .villain_speech_request()
                .request_id()
                .update(request.clone());

            fail_villain_artifact(ctx, &mut artifact, request.request_id, error_message);
            return Ok(());
        }
    };

    if parsed.speech_cues.is_empty() {
        let error_message = "villain speech generation returned no speech cues".to_string();
        request.phase = VillainSpeechPhase::Failed;
        request.error_message = Some(error_message.clone());
        request.updated_at = ctx.timestamp;
        ctx.db
            .villain_speech_request()
            .request_id()
            .update(request.clone());

        fail_villain_artifact(ctx, &mut artifact, request.request_id, error_message);
        return Ok(());
    }

    let payload: VillainSpeechGenerationPayload =
        serde_json::from_str(&request.request_payload_json)
            .map_err(|err| format!("invalid villain speech payload JSON: {err}"))?;

    let (selected_cue_id, selected_speech_text) =
        select_speech_cue(&parsed.speech_cues, payload.selected_cue_id.as_deref())?;
    let speech_cues_json = serde_json::to_string(&parsed.speech_cues)
        .map_err(|err| format!("failed to serialize villain speech cues: {err}"))?;

    request.speech_cues_json = Some(speech_cues_json.clone());
    request.selected_cue_id = Some(selected_cue_id.clone());
    request.selected_speech_text = Some(selected_speech_text.clone());
    request.audio_base64 = None;
    request.mime_type = None;
    request.error_message = None;

    if payload.synthesize_audio.unwrap_or(false) && voice_config_ready(ctx) {
        request.phase = VillainSpeechPhase::PendingTts;
        request.retries = Some(0);
        request.updated_at = ctx.timestamp;
        ctx.db
            .villain_speech_request()
            .request_id()
            .update(request.clone());

        artifact.status = GenerationStatus::PendingTts;
        artifact.speech_cues_json = Some(speech_cues_json);
        artifact.selected_cue_id = Some(selected_cue_id);
        artifact.selected_speech_text = Some(selected_speech_text);
        artifact.audio_base64 = None;
        artifact.mime_type = None;
        artifact.tts_provider = None;
        artifact.last_error = None;
        artifact.updated_at = ctx.timestamp;
        ctx.db
            .villain_speech_artifact()
            .artifact_key()
            .update(artifact.clone());

        if let Err(err) = queue_villain_tts_request(ctx, request.request_id) {
            if let Some(mut latest_request) = ctx
                .db
                .villain_speech_request()
                .request_id()
                .find(request.request_id)
            {
                latest_request.phase = VillainSpeechPhase::Failed;
                latest_request.error_message = Some(err.clone());
                latest_request.updated_at = ctx.timestamp;
                ctx.db
                    .villain_speech_request()
                    .request_id()
                    .update(latest_request);
            }

            fail_villain_artifact(ctx, &mut artifact, request.request_id, err);
        }
        return Ok(());
    }

    request.phase = VillainSpeechPhase::Succeeded;
    request.tts_provider = Some(if payload.synthesize_audio.unwrap_or(false) {
        "disabled".to_string()
    } else {
        "none".to_string()
    });
    request.updated_at = ctx.timestamp;
    ctx.db
        .villain_speech_request()
        .request_id()
        .update(request.clone());

    artifact.status = GenerationStatus::Succeeded;
    artifact.speech_cues_json = Some(speech_cues_json);
    artifact.selected_cue_id = Some(selected_cue_id);
    artifact.selected_speech_text = Some(selected_speech_text);
    artifact.audio_base64 = None;
    artifact.mime_type = None;
    artifact.tts_provider = request.tts_provider;
    if artifact.active_request_id == Some(request.request_id) {
        artifact.active_request_id = None;
    }
    artifact.last_error = None;
    artifact.updated_at = ctx.timestamp;
    ctx.db
        .villain_speech_artifact()
        .artifact_key()
        .update(artifact);

    Ok(())
}

/// Scheduled reducer that finalizes the optional villain speech TTS hop.
#[spacetimedb::reducer]
pub fn _villain_tts_callback(
    ctx: &ReducerContext,
    callback: VillainTtsCallbackSchedule,
) -> Result<(), String> {
    if !ctx.sender_auth().is_internal() {
        return Err("_villain_tts_callback may only be invoked by the scheduler".to_string());
    }

    ctx.db
        .villain_tts_callback_schedule()
        .scheduled_id()
        .delete(&callback.scheduled_id);

    let Some(mut request) = ctx
        .db
        .villain_speech_request()
        .request_id()
        .find(callback.request_id)
    else {
        clear_villain_artifact_for_unknown_request(
            ctx,
            callback.request_id,
            "villain TTS callback arrived after the request row was removed".to_string(),
        );
        return Ok(());
    };

    let Some(mut artifact) = ctx
        .db
        .villain_speech_artifact()
        .artifact_key()
        .find(request.artifact_key.clone())
    else {
        log::warn!(
            "Ignoring villain TTS callback for request {} because artifact {} no longer exists",
            request.request_id,
            request.artifact_key
        );
        return Ok(());
    };

    if let Some(transport_error) = callback.transport_error.clone() {
        request.phase = VillainSpeechPhase::Failed;
        request.error_message = Some(transport_error.clone());
        request.updated_at = ctx.timestamp;
        ctx.db
            .villain_speech_request()
            .request_id()
            .update(request.clone());

        fail_villain_artifact(ctx, &mut artifact, request.request_id, transport_error);
        return Ok(());
    }
    let current_retries = request.retries.unwrap_or(0);
    if callback.status_code == 429 && current_retries < 3 {
        request.retries = Some(current_retries + 1);
        request.phase = VillainSpeechPhase::PendingTts;
        request.updated_at = ctx.timestamp;
        ctx.db
            .villain_speech_request()
            .request_id()
            .update(request.clone());

        queue_villain_tts_request(ctx, request.request_id).ok();
        return Ok(());
    }
    if callback.status_code != 200 {
        let error_message = format!("ElevenLabs returned HTTP {}", callback.status_code);
        request.phase = VillainSpeechPhase::Failed;
        request.error_message = Some(error_message.clone());
        request.updated_at = ctx.timestamp;
        ctx.db
            .villain_speech_request()
            .request_id()
            .update(request.clone());

        fail_villain_artifact(ctx, &mut artifact, request.request_id, error_message);
        return Ok(());
    }

    request.phase = VillainSpeechPhase::Succeeded;
    request.audio_base64 = Some(callback.response_body_base64.clone());
    request.mime_type = callback.mime_type.clone();
    request.tts_provider = Some("elevenlabs".to_string());
    request.error_message = None;
    request.updated_at = ctx.timestamp;
    ctx.db
        .villain_speech_request()
        .request_id()
        .update(request.clone());

    artifact.status = GenerationStatus::Succeeded;
    artifact.audio_base64 = Some(callback.response_body_base64);
    artifact.mime_type = callback.mime_type;
    artifact.tts_provider = Some("elevenlabs".to_string());
    if artifact.active_request_id == Some(request.request_id) {
        artifact.active_request_id = None;
    }
    artifact.last_error = None;
    artifact.updated_at = ctx.timestamp;
    ctx.db
        .villain_speech_artifact()
        .artifact_key()
        .update(artifact);

    Ok(())
}

fn ensure_room_is_active(room: &crate::tables::state::GameRoom) -> Result<(), String> {
    if room.status == RoomStatus::Terminated {
        Err(format!("room {} has already been terminated", room.room_id))
    } else {
        Ok(())
    }
}

fn ensure_generation_room_access(room: &crate::tables::state::GameRoom) -> Result<(), String> {
    if room.status == RoomStatus::Terminated {
        return Err(format!("room {} has already been terminated", room.room_id));
    }

    // Maincloud anonymous HTTP setup flows can present a sender identity that does not line up
    // with the identity persisted into room-scoped rows at creation time. For content generation,
    // the room id itself is the capability boundary, so we keep access tied to room existence.
    Ok(())
}

fn ensure_host_or_player_one(
    sender: Identity,
    room: &crate::tables::state::GameRoom,
) -> Result<(), String> {
    if room.host_identity == sender || room.player_one == Some(sender) {
        return Ok(());
    }
    Err("only the room host or Player 1 may generate clue/manual content".to_string())
}

fn ensure_room_participant(
    sender: Identity,
    room: &crate::tables::state::GameRoom,
) -> Result<(), String> {
    if room.host_identity == sender
        || room.player_one == Some(sender)
        || room.player_two == Some(sender)
    {
        return Ok(());
    }
    Err("only a room participant may generate villain speech".to_string())
}

fn normalize_room_id(room_id: String) -> Result<String, String> {
    let normalized = room_id.trim().to_uppercase();
    if normalized.is_empty() {
        return Err("room_id must not be empty".to_string());
    }
    Ok(normalized)
}

fn normalize_json_object(label: &str, raw_json: String) -> Result<String, String> {
    let trimmed = raw_json.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} must not be empty"));
    }

    let value: Value = serde_json::from_str(trimmed)
        .map_err(|err| format!("{label} must be valid JSON: {err}"))?;
    if !value.is_object() {
        return Err(format!("{label} must be a JSON object"));
    }

    serde_json::to_string(&value).map_err(|err| format!("failed to normalize {label}: {err}"))
}

fn normalize_round_schema_json(round_key: &str, raw_schema_json: String) -> Result<String, String> {
    if raw_schema_json.trim().is_empty() {
        let schema = default_round_response_schema(round_key)?;
        return serde_json::to_string(&schema)
            .map_err(|err| format!("failed to serialize default round schema: {err}"));
    }

    let schema: Value = serde_json::from_str(raw_schema_json.trim())
        .map_err(|err| format!("response_schema_json must be valid JSON: {err}"))?;
    serde_json::to_string(&schema)
        .map_err(|err| format!("failed to normalize response_schema_json: {err}"))
}

fn extract_optional_round_key(payload_value: &Value) -> Result<Option<String>, String> {
    match payload_value.get("round_key") {
        Some(Value::String(round_key)) => Ok(Some(normalize_round_key(round_key)?)),
        Some(_) => Err("request_payload_json.round_key must be a string when provided".to_string()),
        None => Ok(None),
    }
}

fn format_round_artifact_key(room_id: &str, round_key: &str) -> String {
    format!("{}::{}", room_id, round_key)
}

fn format_villain_artifact_key(room_id: &str, round_key: Option<&str>) -> String {
    format!("{}::{}", room_id, round_key.unwrap_or("general"))
}

fn repair_round_artifact_if_stale(ctx: &ReducerContext, artifact_key: &str) {
    let Some(mut artifact) = ctx
        .db
        .round_content_artifact()
        .artifact_key()
        .find(artifact_key.to_string())
    else {
        return;
    };

    let is_stale = matches!(
        artifact.status,
        GenerationStatus::PendingGemini | GenerationStatus::PendingTts
    ) && artifact
        .active_request_id
        .and_then(|request_id| {
            ctx.db
                .round_generation_request()
                .request_id()
                .find(request_id)
        })
        .is_none();

    if is_stale {
        artifact.status = GenerationStatus::Failed;
        artifact.active_request_id = None;
        artifact.last_error =
            Some("Recovered from a stale round content lock left by a missing request".to_string());
        artifact.updated_at = ctx.timestamp;
        ctx.db
            .round_content_artifact()
            .artifact_key()
            .update(artifact);
    }
}

fn repair_villain_artifact_if_stale(ctx: &ReducerContext, artifact_key: &str) {
    let Some(mut artifact) = ctx
        .db
        .villain_speech_artifact()
        .artifact_key()
        .find(artifact_key.to_string())
    else {
        return;
    };

    let is_stale = matches!(
        artifact.status,
        GenerationStatus::PendingGemini | GenerationStatus::PendingTts
    ) && artifact
        .active_request_id
        .and_then(|request_id| {
            ctx.db
                .villain_speech_request()
                .request_id()
                .find(request_id)
        })
        .is_none();

    if is_stale {
        artifact.status = GenerationStatus::Failed;
        artifact.active_request_id = None;
        artifact.last_error = Some(
            "Recovered from a stale villain speech lock left by a missing request".to_string(),
        );
        artifact.updated_at = ctx.timestamp;
        ctx.db
            .villain_speech_artifact()
            .artifact_key()
            .update(artifact);
    }
}

fn upsert_round_content_artifact(ctx: &ReducerContext, artifact: RoundContentArtifact) {
    if ctx
        .db
        .round_content_artifact()
        .artifact_key()
        .find(artifact.artifact_key.clone())
        .is_some()
    {
        ctx.db
            .round_content_artifact()
            .artifact_key()
            .update(artifact);
    } else {
        ctx.db.round_content_artifact().insert(artifact);
    }
}

fn upsert_villain_speech_artifact(ctx: &ReducerContext, artifact: VillainSpeechArtifact) {
    if ctx
        .db
        .villain_speech_artifact()
        .artifact_key()
        .find(artifact.artifact_key.clone())
        .is_some()
    {
        ctx.db
            .villain_speech_artifact()
            .artifact_key()
            .update(artifact);
    } else {
        ctx.db.villain_speech_artifact().insert(artifact);
    }
}

fn fail_round_artifact(
    ctx: &ReducerContext,
    artifact: &mut RoundContentArtifact,
    request_id: u64,
    message: String,
) {
    artifact.status = GenerationStatus::Failed;
    if artifact.active_request_id == Some(request_id) {
        artifact.active_request_id = None;
    }
    artifact.last_error = Some(message);
    artifact.updated_at = ctx.timestamp;
    ctx.db
        .round_content_artifact()
        .artifact_key()
        .update(artifact.clone());
}

fn fail_villain_artifact(
    ctx: &ReducerContext,
    artifact: &mut VillainSpeechArtifact,
    request_id: u64,
    message: String,
) {
    artifact.status = GenerationStatus::Failed;
    if artifact.active_request_id == Some(request_id) {
        artifact.active_request_id = None;
    }
    artifact.last_error = Some(message);
    artifact.updated_at = ctx.timestamp;
    ctx.db
        .villain_speech_artifact()
        .artifact_key()
        .update(artifact.clone());
}

fn clear_round_artifact_for_unknown_request(
    ctx: &ReducerContext,
    request_id: u64,
    message: String,
) {
    for mut artifact in ctx.db.round_content_artifact().iter() {
        if artifact.active_request_id == Some(request_id) {
            artifact.status = GenerationStatus::Failed;
            artifact.active_request_id = None;
            artifact.last_error = Some(message.clone());
            artifact.updated_at = ctx.timestamp;
            ctx.db
                .round_content_artifact()
                .artifact_key()
                .update(artifact);
        }
    }
}

fn clear_villain_artifact_for_unknown_request(
    ctx: &ReducerContext,
    request_id: u64,
    message: String,
) {
    for mut artifact in ctx.db.villain_speech_artifact().iter() {
        if artifact.active_request_id == Some(request_id) {
            artifact.status = GenerationStatus::Failed;
            artifact.active_request_id = None;
            artifact.last_error = Some(message.clone());
            artifact.updated_at = ctx.timestamp;
            ctx.db
                .villain_speech_artifact()
                .artifact_key()
                .update(artifact);
        }
    }
}

fn extract_hidden_answer_candidate(response_value: &Value) -> Option<String> {
    for key in [
        "hidden_answer",
        "validation_answer",
        "kill_phrase_3",
        "target_word",
        "validation_code",
        "validation_cipher",
    ] {
        if let Some(value) = response_value.get(key).and_then(Value::as_str) {
            let normalized = value.trim();
            if !normalized.is_empty() {
                return Some(normalized.to_string());
            }
        }
    }

    None
}

fn select_speech_cue(
    cues: &[VillainSpeechCue],
    requested_cue_id: Option<&str>,
) -> Result<(String, String), String> {
    if let Some(requested_cue_id) = requested_cue_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Some(cue) = cues.iter().find(|cue| cue.cue_id == requested_cue_id) {
            return Ok((cue.cue_id.clone(), cue.speech_text.clone()));
        }
    }

    let cue = cues
        .first()
        .ok_or_else(|| "villain speech response did not include any cues".to_string())?;
    Ok((cue.cue_id.clone(), cue.speech_text.clone()))
}

fn voice_config_ready(ctx: &ReducerContext) -> bool {
    ctx.db
        .voice_config()
        .config_key()
        .find(ACTIVE_VOICE_CONFIG_KEY)
        .map(|config| {
            !config.elevenlabs_api_base_url.trim().is_empty()
                && !config.elevenlabs_api_key.trim().is_empty()
                && !config.elevenlabs_default_voice_id.trim().is_empty()
                && !config.elevenlabs_default_model_id.trim().is_empty()
        })
        .unwrap_or(false)
}

fn ensure_module_owner(ctx: &ReducerContext) -> Result<(), String> {
    let owner = ctx
        .db
        .module_owner()
        .owner_key()
        .find(MODULE_OWNER_KEY)
        .ok_or_else(|| {
            "module owner is not initialized; republish with clear to run init reducer".to_string()
        })?;

    if owner.owner_identity != ctx.sender() {
        return Err("only the module owner may call this reducer".to_string());
    }

    Ok(())
}

fn require_trimmed(label: &str, value: String) -> Result<String, String> {
    let normalized = value.trim().to_string();
    if normalized.is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    Ok(normalized)
}

fn summarize_gemini_http_error(status_code: u16, response_body: &str) -> String {
    let snippet = serde_json::from_str::<Value>(response_body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(Value::as_object)
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|message| !message.is_empty())
                .map(ToString::to_string)
        })
        .or_else(|| {
            let compact = response_body.split_whitespace().collect::<Vec<_>>().join(" ");
            let trimmed = compact.trim();
            if trimmed.is_empty() {
                None
            } else if trimmed.chars().count() > 280 {
                Some(format!("{}...", trimmed.chars().take(280).collect::<String>()))
            } else {
                Some(trimmed.to_string())
            }
        });

    match snippet {
        Some(message) => format!("Gemini returned HTTP {status_code}: {message}"),
        None => format!("Gemini returned HTTP {status_code}"),
    }
}

fn compose_round_one_final_payload(
    skeleton_json: &str,
    expansion_value: Value,
) -> Result<Value, String> {
    let skeleton_value: Value = serde_json::from_str(skeleton_json)
        .map_err(|err| format!("invalid stored round 1 skeleton JSON: {err}"))?;
    let skeleton_object = skeleton_value
        .as_object()
        .ok_or_else(|| "round 1 skeleton payload must be a JSON object".to_string())?;
    let expansion_object = expansion_value
        .as_object()
        .ok_or_else(|| "round 1 expansion payload must be a JSON object".to_string())?;

    let manual = expansion_object
        .get("manual")
        .cloned()
        .ok_or_else(|| "round 1 expansion payload is missing manual".to_string())?;
    let decoder_walkthrough = expansion_object
        .get("decoder_walkthrough")
        .cloned()
        .ok_or_else(|| "round 1 expansion payload is missing decoder_walkthrough".to_string())?;

    Ok(json!({
        "persona_name": skeleton_object
            .get("persona_name")
            .cloned()
            .ok_or_else(|| "round 1 skeleton is missing persona_name".to_string())?,
        "persona_paragraphs": skeleton_object
            .get("persona_paragraphs")
            .cloned()
            .ok_or_else(|| "round 1 skeleton is missing persona_paragraphs".to_string())?,
        "target_word": skeleton_object
            .get("target_word")
            .cloned()
            .ok_or_else(|| "round 1 skeleton is missing target_word".to_string())?,
        "forbidden_words": skeleton_object
            .get("forbidden_words")
            .cloned()
            .ok_or_else(|| "round 1 skeleton is missing forbidden_words".to_string())?,
        "clues": skeleton_object
            .get("clues")
            .cloned()
            .ok_or_else(|| "round 1 skeleton is missing clues".to_string())?,
        "manual": manual,
        "decoder_walkthrough": decoder_walkthrough,
        "solution": skeleton_object
            .get("solution")
            .cloned()
            .ok_or_else(|| "round 1 skeleton is missing solution".to_string())?
    }))
}
