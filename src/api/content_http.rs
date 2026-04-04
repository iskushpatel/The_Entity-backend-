use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use spacetimedb::{log, ProcedureContext, ReducerContext, Table};

use crate::api::http_wrappers::extract_gemini_text;
use crate::models::api_schemas::{
    GeminiContent, GeminiGenerateContentRequest, GeminiGenerationConfig, GeminiPart,
    VillainSpeechGenerationPayload,
};
use crate::tables::state::{
    round_generation_callback_schedule, round_generation_request,
    round_generation_request_schedule, server_config, villain_speech_callback_schedule,
    villain_speech_request, villain_speech_request_schedule, villain_tts_callback_schedule,
    villain_tts_request_schedule, voice_config, RoundGenerationCallbackSchedule,
    RoundGenerationPhase, RoundGenerationRequest, RoundGenerationRequestSchedule, ServerConfig,
    VillainSpeechCallbackSchedule, VillainSpeechPhase, VillainSpeechRequest,
    VillainSpeechRequestSchedule, VillainTtsCallbackSchedule, VillainTtsRequestSchedule,
    VoiceConfig, ACTIVE_SERVER_CONFIG_KEY, ACTIVE_VOICE_CONFIG_KEY,
};

pub fn queue_round_generation_request(ctx: &ReducerContext, request_id: u64) -> Result<(), String> {
    ctx.db
        .round_generation_request_schedule()
        .insert(RoundGenerationRequestSchedule {
            scheduled_id: 0,
            scheduled_at: ctx.timestamp.into(),
            request_id,
        });
    Ok(())
}

pub fn queue_villain_speech_request(ctx: &ReducerContext, request_id: u64) -> Result<(), String> {
    ctx.db
        .villain_speech_request_schedule()
        .insert(VillainSpeechRequestSchedule {
            scheduled_id: 0,
            scheduled_at: ctx.timestamp.into(),
            request_id,
        });
    Ok(())
}

pub fn queue_villain_tts_request(ctx: &ReducerContext, request_id: u64) -> Result<(), String> {
    ctx.db
        .villain_tts_request_schedule()
        .insert(VillainTtsRequestSchedule {
            scheduled_id: 0,
            scheduled_at: ctx.timestamp.into(),
            request_id,
        });
    Ok(())
}

pub fn normalize_round_key(round_key: &str) -> Result<String, String> {
    let normalized = round_key.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "round_1" | "round_2" | "round_3" | "round_4" => Ok(normalized),
        _ => Err(format!(
            "round_key must be one of round_1, round_2, round_3, or round_4; received {}",
            round_key
        )),
    }
}

pub fn default_round_response_schema(round_key: &str) -> Result<Value, String> {
    match normalize_round_key(round_key)?.as_str() {
        "round_1" => Ok(json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "persona_name": { "type": "string" },
                "persona_paragraphs": {
                    "type": "array",
                    "minItems": 2,
                    "maxItems": 3,
                    "items": { "type": "string" }
                },
                "target_word": { "type": "string" },
                "forbidden_words": {
                    "type": "array",
                    "minItems": 5,
                    "maxItems": 5,
                    "items": { "type": "string" }
                },
                "clues": {
                    "type": "array",
                    "minItems": 2,
                    "maxItems": 2,
                    "items": { "type": "object", "additionalProperties": true }
                },
                "manual": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "codex_entries": { "type": "array", "minItems": 1, "maxItems": 1, "items": { "type": "object", "additionalProperties": true } },
                        "timeline_fragments": { "type": "array", "minItems": 1, "maxItems": 1, "items": { "type": "object", "additionalProperties": true } },
                        "cipher_legend": { "type": "array", "minItems": 1, "maxItems": 1, "items": { "type": "object", "additionalProperties": true } },
                        "protocol_matrix": { "type": "array", "minItems": 1, "maxItems": 1, "items": { "type": "object", "additionalProperties": true } },
                        "false_leads": { "type": "array", "minItems": 1, "maxItems": 1, "items": { "type": "object", "additionalProperties": true } }
                    },
                    "required": ["codex_entries", "timeline_fragments", "cipher_legend", "protocol_matrix", "false_leads"]
                },
                "decoder_walkthrough": {
                    "type": "array",
                    "minItems": 1,
                    "items": { "type": "object", "additionalProperties": true }
                },
                "solution": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "final_identity_guess": { "type": "string" },
                        "final_target_word_inference": { "type": "string" }
                    },
                    "required": ["final_identity_guess", "final_target_word_inference"]
                }
            },
            "required": [
                "persona_name",
                "persona_paragraphs",
                "target_word",
                "forbidden_words",
                "clues",
                "manual",
                "decoder_walkthrough",
                "solution"
            ]
        })),
        "round_2" | "round_3" | "round_4" => Ok(json!({
            "type": "object",
            "additionalProperties": true
        })),
        _ => unreachable!(),
    }
}

#[spacetimedb::procedure]
pub fn process_round_content_request(
    ctx: &mut ProcedureContext,
    job: RoundGenerationRequestSchedule,
) {
    if ctx.sender() != ctx.identity() {
        log::warn!("Rejected direct invocation of process_round_content_request");
        return;
    }

    ctx.with_tx(|tx| {
        tx.db
            .round_generation_request_schedule()
            .scheduled_id()
            .delete(&job.scheduled_id);
    });

    let prepared = ctx.try_with_tx(|tx| prepare_round_content_http_request(tx, job.request_id));
    let (request_id, http_request) = match prepared {
        Ok(value) => value,
        Err(err) => {
            enqueue_round_content_callback(ctx, job.request_id, 0, String::new(), Some(err));
            return;
        }
    };

    match ctx.http.send(http_request) {
        Ok(response) => {
            let (parts, body) = response.into_parts();
            enqueue_round_content_callback(
                ctx,
                request_id,
                parts.status.as_u16(),
                body.into_string_lossy(),
                None,
            );
        }
        Err(err) => {
            enqueue_round_content_callback(
                ctx,
                request_id,
                0,
                String::new(),
                Some(err.to_string()),
            );
        }
    }
}

#[spacetimedb::procedure]
pub fn process_villain_speech_request(
    ctx: &mut ProcedureContext,
    job: VillainSpeechRequestSchedule,
) {
    if ctx.sender() != ctx.identity() {
        log::warn!("Rejected direct invocation of process_villain_speech_request");
        return;
    }

    ctx.with_tx(|tx| {
        tx.db
            .villain_speech_request_schedule()
            .scheduled_id()
            .delete(&job.scheduled_id);
    });

    let prepared = ctx.try_with_tx(|tx| prepare_villain_speech_http_request(tx, job.request_id));
    let (request_id, http_request) = match prepared {
        Ok(value) => value,
        Err(err) => {
            enqueue_villain_speech_callback(ctx, job.request_id, 0, String::new(), Some(err));
            return;
        }
    };

    match ctx.http.send(http_request) {
        Ok(response) => {
            let (parts, body) = response.into_parts();
            enqueue_villain_speech_callback(
                ctx,
                request_id,
                parts.status.as_u16(),
                body.into_string_lossy(),
                None,
            );
        }
        Err(err) => {
            enqueue_villain_speech_callback(
                ctx,
                request_id,
                0,
                String::new(),
                Some(err.to_string()),
            );
        }
    }
}

#[spacetimedb::procedure]
pub fn process_villain_tts_request(ctx: &mut ProcedureContext, job: VillainTtsRequestSchedule) {
    if ctx.sender() != ctx.identity() {
        log::warn!("Rejected direct invocation of process_villain_tts_request");
        return;
    }

    ctx.with_tx(|tx| {
        tx.db
            .villain_tts_request_schedule()
            .scheduled_id()
            .delete(&job.scheduled_id);
    });

    let prepared = ctx.try_with_tx(|tx| prepare_villain_tts_http_request(tx, job.request_id));
    let (request_id, http_request) = match prepared {
        Ok(value) => value,
        Err(err) => {
            enqueue_villain_tts_callback(ctx, job.request_id, 0, String::new(), None, Some(err));
            return;
        }
    };

    match ctx.http.send(http_request) {
        Ok(response) => {
            let (parts, body) = response.into_parts();
            let mime_type = parts
                .headers
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .map(|value| value.to_string());
            let body_base64 = BASE64_STANDARD.encode(body.into_bytes());
            enqueue_villain_tts_callback(
                ctx,
                request_id,
                parts.status.as_u16(),
                body_base64,
                mime_type,
                None,
            );
        }
        Err(err) => {
            enqueue_villain_tts_callback(
                ctx,
                request_id,
                0,
                String::new(),
                None,
                Some(err.to_string()),
            );
        }
    }
}

fn prepare_round_content_http_request(
    tx: &spacetimedb::TxContext,
    request_id: u64,
) -> Result<(u64, spacetimedb::http::Request<String>), String> {
    let request = tx
        .db
        .round_generation_request()
        .request_id()
        .find(request_id)
        .ok_or_else(|| format!("round generation request {} no longer exists", request_id))?;

    let config = load_server_config(tx)?;
    let http_request = build_round_content_http_request(&config, &request)?;
    Ok((request.request_id, http_request))
}

fn prepare_villain_speech_http_request(
    tx: &spacetimedb::TxContext,
    request_id: u64,
) -> Result<(u64, spacetimedb::http::Request<String>), String> {
    let request = tx
        .db
        .villain_speech_request()
        .request_id()
        .find(request_id)
        .ok_or_else(|| format!("villain speech request {} no longer exists", request_id))?;

    let config = load_server_config(tx)?;
    let http_request = build_villain_speech_http_request(&config, &request)?;
    Ok((request.request_id, http_request))
}

fn prepare_villain_tts_http_request(
    tx: &spacetimedb::TxContext,
    request_id: u64,
) -> Result<(u64, spacetimedb::http::Request<String>), String> {
    let request = tx
        .db
        .villain_speech_request()
        .request_id()
        .find(request_id)
        .ok_or_else(|| format!("villain speech request {} no longer exists", request_id))?;

    if request.phase != VillainSpeechPhase::PendingTts {
        return Err(format!(
            "villain speech request {} is not ready for TTS",
            request_id
        ));
    }

    let config = load_voice_config(tx)?;
    let http_request = build_villain_tts_http_request(&config, &request)?;
    Ok((request.request_id, http_request))
}

fn load_server_config(tx: &spacetimedb::TxContext) -> Result<ServerConfig, String> {
    tx.db
        .server_config()
        .config_key()
        .find(ACTIVE_SERVER_CONFIG_KEY)
        .ok_or_else(|| {
            format!(
                "ServerConfig row {} is missing; configure Gemini integrations before generating content",
                ACTIVE_SERVER_CONFIG_KEY
            )
        })
}

fn load_voice_config(tx: &spacetimedb::TxContext) -> Result<VoiceConfig, String> {
    tx.db
        .voice_config()
        .config_key()
        .find(ACTIVE_VOICE_CONFIG_KEY)
        .ok_or_else(|| {
            format!(
                "VoiceConfig row {} is missing; configure ElevenLabs before synthesizing villain audio",
                ACTIVE_VOICE_CONFIG_KEY
            )
        })
}

fn build_round_content_http_request(
    config: &ServerConfig,
    request: &RoundGenerationRequest,
) -> Result<spacetimedb::http::Request<String>, String> {
    require_non_empty(
        "ServerConfig.gemini_api_base_url",
        &config.gemini_api_base_url,
    )?;
    require_non_empty("ServerConfig.gemini_api_key", &config.gemini_api_key)?;
    require_non_empty(
        "ServerConfig.gemini_clue_generator_model",
        &config.gemini_clue_generator_model,
    )?;

    let prompt = build_round_generation_prompt(request)?;
    let max_tokens = round_generation_max_output_tokens(request);

    build_gemini_request(
        &config.gemini_api_base_url,
        &config.gemini_api_key,
        &config.gemini_clue_generator_model,
        prompt,
        None,
        round_generation_temperature(request),
        max_tokens,
    )
}

fn build_villain_speech_http_request(
    config: &ServerConfig,
    request: &VillainSpeechRequest,
) -> Result<spacetimedb::http::Request<String>, String> {
    require_non_empty(
        "ServerConfig.gemini_api_base_url",
        &config.gemini_api_base_url,
    )?;
    require_non_empty("ServerConfig.gemini_api_key", &config.gemini_api_key)?;
    require_non_empty(
        "ServerConfig.gemini_villain_model",
        &config.gemini_villain_model,
    )?;

    let payload: VillainSpeechGenerationPayload =
        serde_json::from_str(&request.request_payload_json)
            .map_err(|err| format!("invalid villain speech payload JSON: {err}"))?;

    let prompt = build_villain_speech_prompt(&payload, &request.request_payload_json);

    build_gemini_request(
        &config.gemini_api_base_url,
        &config.gemini_api_key,
        &config.gemini_villain_model,
        prompt,
        Some(villain_speech_response_schema()),
        0.6,
        2048,
    )
}

fn build_villain_tts_http_request(
    config: &VoiceConfig,
    request: &VillainSpeechRequest,
) -> Result<spacetimedb::http::Request<String>, String> {
    let payload: VillainSpeechGenerationPayload =
        serde_json::from_str(&request.request_payload_json)
            .map_err(|err| format!("invalid villain speech payload JSON: {err}"))?;

    require_non_empty(
        "VoiceConfig.elevenlabs_api_base_url",
        &config.elevenlabs_api_base_url,
    )?;
    require_non_empty("VoiceConfig.elevenlabs_api_key", &config.elevenlabs_api_key)?;

    let voice_id = payload
        .voice_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| config.elevenlabs_default_voice_id.clone());
    require_non_empty("Voice voice_id", &voice_id)?;

    let model_id = payload
        .voice_model_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| config.elevenlabs_default_model_id.clone());
    require_non_empty("Voice model_id", &model_id)?;

    let text = request
        .selected_speech_text
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "selected villain speech text is missing for TTS".to_string())?;

    let url = format!(
        "{}/text-to-speech/{}",
        config.elevenlabs_api_base_url.trim_end_matches('/'),
        voice_id
    );
    let body = serde_json::to_string(&json!({
        "text": text,
        "model_id": model_id,
    }))
    .map_err(|err| format!("failed to serialize ElevenLabs payload: {err}"))?;

    spacetimedb::http::Request::builder()
        .method("POST")
        .uri(url)
        .header("Content-Type", "application/json")
        .header("Accept", "audio/mpeg")
        .header("xi-api-key", config.elevenlabs_api_key.as_str())
        .body(body)
        .map_err(|err| format!("failed to build villain TTS request: {err}"))
}

fn build_gemini_request(
    base_url: &str,
    api_key: &str,
    model: &str,
    prompt: String,
    response_schema: Option<Value>,
    temperature: f32,
    max_output_tokens: u32,
) -> Result<spacetimedb::http::Request<String>, String> {
    let payload = GeminiGenerateContentRequest {
        contents: vec![GeminiContent {
            role: Some("user".to_string()),
            parts: vec![GeminiPart { text: Some(prompt) }],
        }],
        generation_config: GeminiGenerationConfig {
            response_mime_type: "application/json".to_string(),
            response_json_schema: response_schema,
            candidate_count: 1,
            max_output_tokens,
            temperature,
            thinking_config: None,
        },
    };

    let url = format!(
        "{}/models/{}:generateContent?key={}",
        base_url.trim_end_matches('/'),
        model,
        api_key,
    );

    let body = serde_json::to_string(&payload)
        .map_err(|err| format!("failed to serialize Gemini request payload: {err}"))?;

    spacetimedb::http::Request::builder()
        .method("POST")
        .uri(url)
        .header("Content-Type", "application/json")
        .body(body)
        .map_err(|err| format!("failed to build Gemini HTTP request: {err}"))
}

fn round_generation_temperature(request: &RoundGenerationRequest) -> f32 {
    match request.round_key.as_str() {
        "round_1" => match request.phase {
            RoundGenerationPhase::PendingGeminiSkeleton => 0.95,
            RoundGenerationPhase::PendingGeminiExpansion => 0.65,
            _ => 0.65,
        },
        "round_2" => 0.8,
        "round_3" => 0.75,
        "round_4" => 0.55,
        _ => 0.7,
    }
}

fn round_generation_max_output_tokens(request: &RoundGenerationRequest) -> u32 {
    match request.round_key.as_str() {
        "round_1" => match request.phase {
            RoundGenerationPhase::PendingGeminiSkeleton => 2000,
            RoundGenerationPhase::PendingGeminiExpansion => 3500,
            _ => 3500,
        },
        "round_2" => 2200,
        "round_3" => 2200,
        "round_4" => 1600,
        _ => 1800,
    }
}

fn build_round_generation_prompt(request: &RoundGenerationRequest) -> Result<String, String> {
    let payload_value: Value = serde_json::from_str(&request.request_payload_json)
        .map_err(|err| format!("invalid request_payload_json: {err}"))?;

    match normalize_round_key(&request.round_key)?.as_str() {
        "round_1" => match request.phase {
            RoundGenerationPhase::PendingGeminiSkeleton => build_round_one_skeleton_prompt(&payload_value),
            RoundGenerationPhase::PendingGeminiExpansion => build_round_one_expansion_prompt(&payload_value, request.skeleton_payload_json.as_deref()),
            _ => Err("Invalid phase for round 1 generation".to_string()),
        },
        "round_2" => Ok([
            "You are an expert game designer creating Round 2 content for an asymmetrical deduction game.",
            "Generate a complete clue and manual package for the requested Round 2 structure.",
            "Round 2 should center on corrupted incident logs, post-mortem evidence, branching logic, and a hidden answer that can only be solved by combining the Player 1 material with the Player 2 manual.",
            "The clue material must be atmospheric but internally consistent.",
            "The manual must be explicit, usable under pressure, and sufficient to derive the validation answer from the clue material.",
            "Return JSON only and follow the response schema exactly.",
            "",
            "Round parameter JSON:",
            &serde_json::to_string_pretty(&payload_value).unwrap_or_else(|_| request.request_payload_json.to_string()),
        ]
        .join("\n")),
        "round_3" => Ok([
            "You are an expert game designer creating Round 3 content for an asymmetrical deduction game.",
            "Generate a thematic cipher round where Player 1 receives a dense structural text block and Player 2 receives a manual containing parsing rules, branching logic, or procedural extraction rules.",
            "The final answer must be solvable from the physical structure of the Player 1 text, not from vague interpretation alone.",
            "The manual must be rigorous enough for two players to collaborate under time pressure.",
            "Return JSON only and follow the response schema exactly.",
            "",
            "Round parameter JSON:",
            &serde_json::to_string_pretty(&payload_value).unwrap_or_else(|_| request.request_payload_json.to_string()),
        ]
        .join("\n")),
        "round_4" => Ok([
            "You are an expert game designer creating Round 4 content for an asymmetrical deduction game.",
            "Generate a high-pressure final calibration round built around short, hostile, mechanically precise clue-and-manual logic.",
            "If the requested structure resembles a native or deterministic final round, produce the rule package, player-facing logic, and manual mapping rather than implementation notes.",
            "The result must be concise, tense, and easy to operationalize in code and UI.",
            "Return JSON only and follow the response schema exactly.",
            "",
            "Round parameter JSON:",
            &serde_json::to_string_pretty(&payload_value).unwrap_or_else(|_| request.request_payload_json.to_string()),
        ]
        .join("\n")),
        _ => unreachable!(),
    }
}

fn get_requested_persona(payload_value: &Value) -> Result<&str, String> {
    payload_value
        .get("requested_persona")
        .or_else(|| payload_value.get("requestedPersona"))
        .or_else(|| payload_value.get("persona_name"))
        .or_else(|| payload_value.get("personaName"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "round_1 generation requires requested_persona in request_payload_json".to_string()
        })
}

fn build_round_one_skeleton_prompt(payload_value: &Value) -> Result<String, String> {
    let requested_persona = get_requested_persona(payload_value)?;

    Ok([
        "Role & Objective:",
        "You are an expert game designer creating an asymmetrical deduction game. Generate a Skeleton for the requested character persona.",
        "",
        "Requested Persona:",
        requested_persona,
        "",
        "Task Details:",
        "",
        "persona_name: Repeat the exact requested persona.",
        "",
        "target_word: Select a highly specific noun associated with this persona's lore or methods.",
        "",
        "forbidden_words: List exactly 5 words that are the most obvious clues or synonyms for the target_word.",
        "",
        "clue_concepts: Generate exactly 4 short concepts/ideas for clues. Do not write the full clues.",
        "",
        "manual_structure: Outline the structure of a large manual with many sections (codex_entries, timeline_fragments, cipher_legend, protocol_matrix, false_leads). Give a brief 1 sentence description of the overall theme and how distractors will function to hide the true clues.",
        "",
        "decoder_walkthrough_skeleton: Provide a logical flow mapping the 4 clues to sections in the manual that would allow deducing the target word.",
        "",
        "solution: Provide final_identity_guess and final_target_word_inference.",
        "",
        "Return JSON strictly following these keys: persona_name, target_word, forbidden_words, clue_concepts, manual_structure, decoder_walkthrough_skeleton, solution.",
    ]
    .join("\n"))
}

fn build_round_one_expansion_prompt(payload_value: &Value, skeleton_json: Option<&str>) -> Result<String, String> {
    let requested_persona = get_requested_persona(payload_value)?;
    let skeleton = skeleton_json.ok_or_else(|| "Skeleton JSON missing for expansion phase".to_string())?;

    Ok([
        "Role & Objective:",
        "You are an expert game designer. Below is a Skeleton for a deduction game round. Expand this skeleton into the final full version.",
        "",
        "Requested Persona:",
        requested_persona,
        "",
        "Skeleton Context:",
        skeleton,
        "",
        "Task Details:",
        "",
        "persona_name: Keep from skeleton.",
        "",
        "target_word: Keep from skeleton.",
        "",
        "forbidden_words: Keep from skeleton.",
        "",
        "persona_paragraphs: Write 2 to 3 paragraphs written in the distinct voice of this persona. The primary goal of these paragraphs is to act as a riddle so Player 1 can deduce WHO is speaking.",
        "",
        "clues: Generate exactly 4 clues based on clue_concepts in skeleton. Each clue must include clue_id, clue_type, clue_text, required_manual_refs, expected_inference, and difficulty.",
        "",
        "manual: Expand the manual_structure into detailed content with these sections. It must be a large document with many details where the real answers are hidden among many distractors. Output must be a JSON array of strings or formatted text for each section:",
        "- codex_entries: generate a single massive, highly detailed narrative report spanning 4-5 paragraphs filled with dense lore, distractors, and the real answer.",
        "- timeline_fragments: generate 1 precise timeline record.",
        "- cipher_legend: generate detailed rules with 1 complex cipher.",
        "- protocol_matrix: generate 1 detailed operational rule.",
        "- false_leads: generate 1 highly plausible red herring paragraph.",
        "",
        "decoder_walkthrough: Provide exact deduction steps mapping each clue_id to manual_refs_used.",
        "",
        "solution: Keep from skeleton.",
        "",
        "ABSOLUTE CONSTRAINTS:",
        "",
        "DO NOT use the persona_name (or any direct aliases) in the paragraphs.",
        "",
        "DO NOT use the target_word anywhere in the paragraphs.",
        "",
        "DO NOT use ANY of the 5 forbidden_words anywhere in the paragraphs.",
        "",
        "The final output MUST be pure JSON matching the standard final clue and manual structure for Round 1.",
        "",
        "Make the persona's identity guessable through their tone, philosophy, and subtle lore hints.",
        "The clues must be non-trivial and at least half must require combining two or more manual sections.",
        "Keep each persona paragraph around 80 words.",
        "The manual sections should contain many paragraphs of details to act as distractors where the real clues are hidden.",
        "Output JSON key order should be: persona_name, persona_paragraphs, target_word, forbidden_words, clues, manual, decoder_walkthrough, solution.",
        "",
        "Output rules:",
        "1. Return JSON only.",
        "2. Follow schema exactly.",
        "3. persona_name must exactly match the requested persona string.",
        "4. target_word must be a single noun.",
        "5. forbidden_words must contain exactly 5 distinct words.",
        "6. persona_paragraphs must contain 2 or 3 full paragraphs in the persona voice.",
        "",
        "Additional payload JSON:",
        &serde_json::to_string_pretty(payload_value).unwrap_or_else(|_| "{}".to_string()),
    ]
    .join("\n"))
}

fn build_villain_speech_prompt(
    payload: &VillainSpeechGenerationPayload,
    raw_payload_json: &str,
) -> String {
    let villain_name = payload
        .villain_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("The Entity");
    let scene = payload
        .scene
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("the villain speaks as clues appear on screen");
    let tone = payload
        .tone
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("cold, superior, predatory, and theatrical");

    [
        "You are the villain dialogue writer for an asymmetrical horror puzzle game.",
        "Return JSON only.",
        "Generate a set of spoken cues that should play alongside clue reveals using the supplied payload and clue context.",
        "",
        "Speech rules:",
        "1. Produce one speech cue for each supplied clue beat whenever clue ids are available.",
        "2. Preserve supplied clue ids exactly in linked_clue_id whenever possible.",
        "3. Each speech_text must be 2 to 4 sentences, voiced, performable, and suitable for TTS.",
        "4. The villain may taunt, misdirect, threaten, narrate, or frame the clue, but must not directly reveal the validation answer, hidden answer, or kill phrase.",
        "5. delivery_style must be a short performance note.",
        "6. trigger must briefly explain when the line should play in the UI.",
        "7. Escalate menace across rounds when the provided context suggests progression.",
        "",
        &format!("Villain name: {}", villain_name),
        &format!("Scene: {}", scene),
        &format!("Tone: {}", tone),
        "",
        "Payload and clue context JSON:",
        raw_payload_json,
    ]
    .join("\n")
}

fn villain_speech_response_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "speech_cues": {
                "type": "array",
                "minItems": 1,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "cue_id": { "type": "string" },
                        "round_key": { "type": "string" },
                        "linked_clue_id": { "type": "string" },
                        "trigger": { "type": "string" },
                        "delivery_style": { "type": "string" },
                        "speech_text": { "type": "string" }
                    },
                    "required": ["cue_id", "round_key", "linked_clue_id", "trigger", "delivery_style", "speech_text"]
                }
            }
        },
        "required": ["speech_cues"]
    })
}

fn enqueue_round_content_callback(
    ctx: &mut ProcedureContext,
    request_id: u64,
    status_code: u16,
    response_body: String,
    transport_error: Option<String>,
) {
    ctx.with_tx(|tx| {
        tx.db
            .round_generation_callback_schedule()
            .insert(RoundGenerationCallbackSchedule {
                scheduled_id: 0,
                scheduled_at: tx.timestamp.into(),
                request_id,
                status_code,
                response_body: response_body.clone(),
                transport_error: transport_error.clone(),
            });
    });
}

fn enqueue_villain_speech_callback(
    ctx: &mut ProcedureContext,
    request_id: u64,
    status_code: u16,
    response_body: String,
    transport_error: Option<String>,
) {
    ctx.with_tx(|tx| {
        tx.db
            .villain_speech_callback_schedule()
            .insert(VillainSpeechCallbackSchedule {
                scheduled_id: 0,
                scheduled_at: tx.timestamp.into(),
                request_id,
                status_code,
                response_body: response_body.clone(),
                transport_error: transport_error.clone(),
            });
    });
}

fn enqueue_villain_tts_callback(
    ctx: &mut ProcedureContext,
    request_id: u64,
    status_code: u16,
    response_body_base64: String,
    mime_type: Option<String>,
    transport_error: Option<String>,
) {
    ctx.with_tx(|tx| {
        tx.db
            .villain_tts_callback_schedule()
            .insert(VillainTtsCallbackSchedule {
                scheduled_id: 0,
                scheduled_at: tx.timestamp.into(),
                request_id,
                status_code,
                response_body_base64: response_body_base64.clone(),
                mime_type: mime_type.clone(),
                transport_error: transport_error.clone(),
            });
    });
}

pub fn parse_round_generation_result(body: &str) -> Result<Value, String> {
    let parsed: Value = serde_json::from_str(body).map_err(|err| err.to_string())?;

    // If Gemini returned an envelope, extract and parse the text candidate.
    if parsed.get("candidates").is_some() {
        let finish_reason = parsed
            .get("candidates")
            .and_then(Value::as_array)
            .and_then(|candidates| candidates.first())
            .and_then(|candidate| candidate.get("finishReason"))
            .and_then(Value::as_str)
            .unwrap_or("UNKNOWN");

        let text = extract_gemini_text(body)?;
        let candidate_value: Result<Value, _> = serde_json::from_str(&text);

        // Gemini can mark a response as MAX_TOKENS even when the JSON object is complete.
        // Accept complete candidate JSON regardless of finish reason and only fail when parse fails.
        if let Ok(value) = candidate_value {
            return Ok(value);
        }

        if finish_reason != "STOP" {
            return Err(format!(
                "Gemini candidate incomplete (finishReason={finish_reason}); retry generation with a smaller payload"
            ));
        }

        return serde_json::from_str(&text).map_err(|err| {
            format!(
                "failed to parse extracted Gemini candidate JSON: {err}; candidate_len={}",
                text.len()
            )
        });
    }

    Ok(parsed)
}

fn require_non_empty(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    Ok(())
}
