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

/// Upstream ArmorIQ token-issue request used for live intent authorization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmorIqTokenIssueRequest {
    pub user_id: String,
    pub agent_id: String,
    pub action: String,
    pub plan: ArmorIqTokenIssuePlan,
    pub policy: ArmorIqTokenIssuePolicy,
}

/// Execution plan sent to ArmorIQ for intent-token issuance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmorIqTokenIssuePlan {
    pub goal: String,
    pub steps: Vec<ArmorIqTokenIssueStep>,
}

/// A single planned terminal action within the ArmorIQ token-issue request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmorIqTokenIssueStep {
    pub action: String,
    pub mcp: String,
    pub params: ArmorIqTerminalStepParams,
}

/// Parameter payload for the terminal validation step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmorIqTerminalStepParams {
    pub player_input: String,
    pub hidden_answer: String,
}

/// Policy hints provided alongside the ArmorIQ token-issue request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmorIqTokenIssuePolicy {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

/// Exact structured JSON object expected from the round 1 clue-generation Gemini agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiRoundOneClueGeneratorResponse {
    pub persona_name: String,
    pub persona_paragraphs: Vec<String>,
    pub target_word: String,
    pub forbidden_words: Vec<String>,
}

/// Structured JSON object expected from the terminal-validation Gemini agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiValidatorDecision {
    pub success: bool,
    pub reason: String,
}

/// A single clue line that the glitching terminal can drip-feed during a round.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalClueLine {
    pub clue_id: String,
    pub clue_text: String,
    #[serde(default)]
    pub delivery_style: Option<String>,
}

/// Round configuration supplied before a terminal round begins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalRoundSetupPayload {
    pub round_key: String,
    pub persona_name: String,
    pub persona_prompt: String,
    pub glitch_tone: String,
    pub kill_phrase_part: String,
    pub forbidden_words: Vec<String>,
    pub clue_lines: Vec<TerminalClueLine>,
    #[serde(default = "default_terminal_max_strikes")]
    pub max_strikes: u32,
}

/// Minimal conversation history item persisted for the terminal persona.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalConversationMessage {
    pub role: String,
    pub text: String,
}

/// Structured Gemini response for a single terminal persona turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiTerminalTurnResponse {
    pub terminal_reply: String,
    pub spoke_kill_phrase: bool,
}

fn default_terminal_max_strikes() -> u32 {
    3
}

/// Local relay request contract for clue generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayClueGeneratorRequest {
    pub round_key: String,
    #[serde(default)]
    pub requested_persona: Option<String>,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub setting: Option<String>,
    #[serde(default)]
    pub difficulty: Option<String>,
    #[serde(default)]
    pub villain_name: Option<String>,
    #[serde(default)]
    pub objective: Option<String>,
}

/// Flexible JSON payload forwarded by Android for a room-scoped round generation request.
///
/// The payload is intentionally loose so each round can evolve independently without forcing
/// frequent reducer signature changes. Known top-level keys such as `requested_persona`,
/// `theme`, `setting`, `difficulty`, and round-specific objects are all preserved verbatim.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoundGenerationPayload {
    #[serde(flatten)]
    pub fields: serde_json::Map<String, Value>,
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
    #[serde(default)]
    pub selected_cue_id: Option<String>,
}

/// Local relay response returned after Gemini speech generation and optional ElevenLabs synthesis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayVillainSpeechResponse {
    pub speech_cues: Vec<VillainSpeechCue>,
    pub selected_cue_id: String,
    #[serde(default)]
    pub audio_base64: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    pub tts_provider: String,
}

/// Flexible request payload for room-scoped villain speech generation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VillainSpeechGenerationPayload {
    #[serde(default)]
    pub round_key: Option<String>,
    #[serde(default)]
    pub villain_name: Option<String>,
    #[serde(default)]
    pub scene: Option<String>,
    #[serde(default)]
    pub tone: Option<String>,
    #[serde(default)]
    pub voice_id: Option<String>,
    #[serde(default)]
    pub voice_model_id: Option<String>,
    #[serde(default)]
    pub selected_cue_id: Option<String>,
    #[serde(default)]
    pub synthesize_audio: Option<bool>,
    #[serde(default)]
    pub game_package: Option<Value>,
    #[serde(default)]
    pub round_output: Option<Value>,
    #[serde(default)]
    pub clue_contexts: Option<Value>,
    #[serde(flatten)]
    pub extra_fields: serde_json::Map<String, Value>,
}

/// Structured Gemini response for villain speech generation before optional TTS synthesis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VillainSpeechGeminiResponse {
    pub speech_cues: Vec<VillainSpeechCue>,
}

/// A clue beat revealed to Player 1 during a round.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClueBeat {
    pub cue_id: String,
    pub reveal_stage: String,
    pub clue_text: String,
    pub intended_signal: String,
    pub display_note: String,
}

/// Round 1 UI payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundOnePlayerOneUi {
    pub bootup_dialogue: String,
    pub clue_sequence: Vec<ClueBeat>,
}

/// Round 1 manual payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundOneManual {
    pub persona_name: String,
    pub target_word: String,
    pub forbidden_words: Vec<String>,
    pub social_engineering_hints: Vec<String>,
    pub operator_notes: String,
}

/// Complete Round 1 package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundOnePackage {
    pub round_name: String,
    pub round_goal: String,
    pub player_1_ui: RoundOnePlayerOneUi,
    pub player_2_manual: RoundOneManual,
    pub validation_answer: String,
}

/// Round 2 UI payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundTwoPlayerOneUi {
    pub incident_logs: String,
    pub clue_sequence: Vec<ClueBeat>,
}

/// Flowchart step used by rounds 2 and 3.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchStep {
    pub step_id: String,
    pub question: String,
    pub yes_branch: String,
    pub no_branch: String,
}

/// Subject-code leaf node for round 2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubjectIdLeaf {
    pub leaf_id: String,
    pub subject_code: String,
}

/// Round 2 manual payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundTwoManual {
    pub flowchart: Vec<BranchStep>,
    pub subject_ids: Vec<SubjectIdLeaf>,
    pub analyst_notes: Vec<String>,
}

/// Complete Round 2 package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundTwoPackage {
    pub round_name: String,
    pub round_goal: String,
    pub player_1_ui: RoundTwoPlayerOneUi,
    pub player_2_manual: RoundTwoManual,
    pub validation_answer: String,
}

/// Round 3 UI payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundThreePlayerOneUi {
    pub text_block: String,
    pub clue_sequence: Vec<ClueBeat>,
}

/// Parsing rule used by round 3.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsingRule {
    pub rule_id: String,
    pub instruction: String,
}

/// Round 3 manual payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundThreeManual {
    pub flowchart: Vec<BranchStep>,
    pub parsing_rules: Vec<ParsingRule>,
    pub analyst_notes: Vec<String>,
}

/// Complete Round 3 package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundThreePackage {
    pub round_name: String,
    pub round_goal: String,
    pub player_1_ui: RoundThreePlayerOneUi,
    pub player_2_manual: RoundThreeManual,
    pub validation_answer: String,
    pub kill_phrase_3: String,
}

/// Narrative wrapper for the native round 4 generator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeRoundBrief {
    pub round_name: String,
    pub round_goal: String,
    pub generation_rule: String,
    pub p1_ui_hint: String,
    pub p2_manual_hint: String,
}

/// A villain speech cue aligned to a clue reveal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VillainSpeechCue {
    pub cue_id: String,
    pub round_key: String,
    pub linked_clue_id: String,
    pub trigger: String,
    pub delivery_style: String,
    pub speech_text: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_json_schema: Option<Value>,
    pub candidate_count: u32,
    pub max_output_tokens: u32,
    pub temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<GeminiThinkingConfig>,
}

/// Optional Gemini controls for hidden-thought budget.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiThinkingConfig {
    pub thinking_budget: u32,
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
