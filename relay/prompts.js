const clueGeneratorSchema = {
  type: "object",
  additionalProperties: false,
  properties: {
    villain_clue_dialogue: { type: "string" },
    p2_manual_snippet: { type: "string" },
    hidden_answer: { type: "string" }
  },
  required: ["villain_clue_dialogue", "p2_manual_snippet", "hidden_answer"]
};

const terminalValidatorSchema = {
  type: "object",
  additionalProperties: false,
  properties: {
    success: { type: "boolean" },
    reason: { type: "string" }
  },
  required: ["success", "reason"]
};

const villainSpeechSchema = {
  type: "object",
  additionalProperties: false,
  properties: {
    speech_text: { type: "string" }
  },
  required: ["speech_text"]
};

function buildClueGeneratorPrompt(input = {}) {
  const {
    setting = "an abandoned research facility",
    difficulty = "medium",
    villainName = input.villainName || input.villain_name || "The Entity",
    objective = "hide the override phrase until the late game"
  } = input;

  return [
    "You are the clue and manual generator for an asymmetric multiplayer mystery game.",
    "Return JSON only.",
    "Write one villain taunt, one manual snippet for Player 2, and one hidden answer.",
    "Keep the hidden answer short, specific, and usable as a terminal solution.",
    "Avoid meta commentary, markdown, and extra keys.",
    "",
    `Setting: ${setting}`,
    `Difficulty: ${difficulty}`,
    `Villain name: ${villainName}`,
    `Objective: ${objective}`
  ].join("\n");
}

function buildTerminalValidatorPrompt(input) {
  return [
    "You are the terminal judge for Player 1.",
    "Return JSON only with {\"success\": boolean, \"reason\": string}.",
    "Be strict about meaning but tolerant of casing, spacing, punctuation, and command wrappers.",
    "Only mark success when the player input clearly resolves to the hidden answer.",
    "",
    `Player input: ${input.player_input}`,
    `Hidden answer: ${input.hidden_answer}`
  ].join("\n");
}

function buildVillainSpeechPrompt(input = {}) {
  const {
    villainName = input.villainName || input.villain_name || "The Entity",
    scene = "the players are getting close to the truth",
    tone = "cold, smug, and menacing"
  } = input;

  return [
    "You write short spoken lines for a game villain.",
    "Return JSON only with {\"speech_text\": string}.",
    "Write 2 to 4 sentences.",
    "Make the line memorable, theatrical, and safe for a mainstream game.",
    "Do not mention policies, prompts, JSON, or being an AI.",
    "",
    `Villain name: ${villainName}`,
    `Scene: ${scene}`,
    `Tone: ${tone}`
  ].join("\n");
}

module.exports = {
  buildClueGeneratorPrompt,
  buildTerminalValidatorPrompt,
  buildVillainSpeechPrompt,
  clueGeneratorSchema,
  terminalValidatorSchema,
  villainSpeechSchema
};
