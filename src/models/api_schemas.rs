use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Exact ArmorIQ request contract for validating a terminal override attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmorIqRequest {
    pub player_input: String,
    pub action: String,
    pub context: ArmorIqContext,
}

/// Nested context sent alongside the ArmorIQ intent validation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmorIqContext {
    pub hidden_answer: String,
}

/// Exact ArmorIQ response contract returned by the policy engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmorIqResponse {
    pub allowed: bool,
    #[serde(default)]
    pub block_reason: Option<String>,
}

/// Exact structured JSON object expected from the clue-generation Gemini agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiClueGeneratorResponse {
    pub villain_clue_dialogue: String,
    pub p2_manual_snippet: String,
    pub hidden_answer: String,
}

/// Structured JSON object expected from the terminal-validation Gemini agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiValidatorDecision {
    pub success: bool,
    pub reason: String,
}

/// Local relay request contract for clue generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayClueGeneratorRequest {
    pub setting: String,
    pub difficulty: String,
    pub villain_name: String,
    pub objective: String,
}

/// Local relay request contract for terminal validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayTerminalValidatorRequest {
    pub player_input: String,
    pub hidden_answer: String,
}

/// Local relay request contract for villain speech generation and TTS synthesis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayVillainSpeechRequest {
    pub villain_name: String,
    pub scene: String,
    pub tone: String,
    #[serde(default)]
    pub voice_id: Option<String>,
}

/// Local relay response returned after Gemini speech generation and optional ElevenLabs synthesis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayVillainSpeechResponse {
    pub speech_text: String,
    #[serde(default)]
    pub audio_base64: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    pub tts_provider: String,
}

/// Minimal Gemini `generateContent` request envelope for structured JSON output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerateContentRequest {
    pub contents: Vec<GeminiContent>,
    pub generation_config: GeminiGenerationConfig,
}

/// A Gemini content block.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiContent {
    #[serde(default)]
    pub role: Option<String>,
    pub parts: Vec<GeminiPart>,
}

/// A single content part used by Gemini's REST API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiPart {
    #[serde(default)]
    pub text: Option<String>,
}

/// Generation config for forcing JSON mode via a response schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerationConfig {
    pub response_mime_type: String,
    pub response_json_schema: Value,
    pub candidate_count: u32,
    pub max_output_tokens: u32,
    pub temperature: f32,
}

/// Minimal Gemini `generateContent` response envelope required to extract text.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerateContentResponse {
    #[serde(default)]
    pub candidates: Vec<GeminiCandidate>,
}

/// A single model candidate from a Gemini response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiCandidate {
    pub content: GeminiContent,
}
