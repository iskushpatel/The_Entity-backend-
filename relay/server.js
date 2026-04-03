const http = require("node:http");
const {
  buildClueGeneratorPrompt,
  buildTerminalValidatorPrompt,
  buildVillainSpeechPrompt,
  clueGeneratorSchema,
  terminalValidatorSchema,
  villainSpeechSchema
} = require("./prompts");
const {
  buildMockClueResponse,
  buildMockValidatorResponse,
  buildMockVillainResponse,
  callGeminiJson,
  synthesizeWithElevenLabs
} = require("./clients");

function createConfigFromEnv(overrides = {}) {
  return {
    host: overrides.host || process.env.RELAY_HOST || "127.0.0.1",
    port: Number(overrides.port || process.env.RELAY_PORT || 8787),
    mockMode: coerceBoolean(overrides.mockMode ?? process.env.MOCK_MODE ?? false),
    geminiApiKey: overrides.geminiApiKey ?? process.env.GEMINI_API_KEY ?? "",
    geminiClueModel: overrides.geminiClueModel || process.env.GEMINI_CLUE_MODEL || "gemini-2.5-flash",
    geminiValidatorModel:
      overrides.geminiValidatorModel || process.env.GEMINI_VALIDATOR_MODEL || "gemini-2.5-flash",
    geminiVillainModel:
      overrides.geminiVillainModel || process.env.GEMINI_VILLAIN_MODEL || "gemini-2.5-flash",
    elevenLabsApiKey: overrides.elevenLabsApiKey ?? process.env.ELEVENLABS_API_KEY ?? "",
    elevenLabsVoiceId: overrides.elevenLabsVoiceId ?? process.env.ELEVENLABS_VOICE_ID ?? "",
    elevenLabsModelId:
      overrides.elevenLabsModelId || process.env.ELEVENLABS_MODEL_ID || "eleven_multilingual_v2"
  };
}

function createServer(config = createConfigFromEnv()) {
  return http.createServer(async (req, res) => {
    try {
      if (req.method === "GET" && req.url === "/health") {
        return sendJson(res, 200, {
          ok: true,
          mock_mode: config.mockMode,
          routes: [
            "POST /api/gemini/clue-generator",
            "POST /api/gemini/terminal-validator",
            "POST /api/villain/speech"
          ]
        });
      }

      if (req.method === "POST" && req.url === "/api/gemini/clue-generator") {
        const body = await readJsonBody(req);
        const result = await callGeminiJson({
          apiKey: config.geminiApiKey,
          model: config.geminiClueModel,
          prompt: buildClueGeneratorPrompt(body),
          responseJsonSchema: clueGeneratorSchema,
          mockMode: config.mockMode,
          mockValue: buildMockClueResponse(body)
        });
        return sendJson(res, 200, result);
      }

      if (req.method === "POST" && req.url === "/api/gemini/terminal-validator") {
        const body = await readJsonBody(req);
        requireString(body.player_input, "player_input");
        requireString(body.hidden_answer, "hidden_answer");

        const result = await callGeminiJson({
          apiKey: config.geminiApiKey,
          model: config.geminiValidatorModel,
          prompt: buildTerminalValidatorPrompt(body),
          responseJsonSchema: terminalValidatorSchema,
          mockMode: config.mockMode,
          mockValue: buildMockValidatorResponse(body)
        });
        return sendJson(res, 200, result);
      }

      if (req.method === "POST" && req.url === "/api/villain/speech") {
        const body = await readJsonBody(req);
        const textResult = await callGeminiJson({
          apiKey: config.geminiApiKey,
          model: config.geminiVillainModel,
          prompt: buildVillainSpeechPrompt(body),
          responseJsonSchema: villainSpeechSchema,
          mockMode: config.mockMode,
          mockValue: buildMockVillainResponse(body)
        });

        const audio = await synthesizeWithElevenLabs({
          apiKey: config.elevenLabsApiKey,
          voiceId: body.voice_id || config.elevenLabsVoiceId,
          text: textResult.speech_text,
          modelId: body.voice_model_id || config.elevenLabsModelId,
          mockMode: config.mockMode
        });

        return sendJson(res, 200, {
          speech_text: textResult.speech_text,
          audio_base64: audio.audio_base64,
          mime_type: audio.mime_type,
          tts_provider: audio.provider
        });
      }

      return sendJson(res, 404, { error: "Route not found" });
    } catch (error) {
      return sendJson(res, error.statusCode || 500, {
        error: error.message || "Internal server error"
      });
    }
  });
}

function startServer(configOverrides = {}) {
  const config = createConfigFromEnv(configOverrides);
  const server = createServer(config);

  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(config.port, config.host, () => {
      resolve({ server, config });
    });
  });
}

function readJsonBody(req) {
  return new Promise((resolve, reject) => {
    let data = "";
    req.setEncoding("utf8");

    req.on("data", (chunk) => {
      data += chunk;
      if (data.length > 128 * 1024) {
        reject(withStatus(new Error("Request body is too large"), 413));
        req.destroy();
      }
    });

    req.on("end", () => {
      if (!data) {
        resolve({});
        return;
      }

      try {
        resolve(JSON.parse(data));
      } catch {
        reject(withStatus(new Error("Invalid JSON body"), 400));
      }
    });

    req.on("error", reject);
  });
}

function sendJson(res, statusCode, payload) {
  res.writeHead(statusCode, { "Content-Type": "application/json; charset=utf-8" });
  res.end(JSON.stringify(payload));
}

function requireString(value, fieldName) {
  if (typeof value !== "string" || !value.trim()) {
    throw withStatus(new Error(`${fieldName} must be a non-empty string`), 400);
  }
}

function withStatus(error, statusCode) {
  error.statusCode = statusCode;
  return error;
}

function coerceBoolean(value) {
  return value === true || value === "true" || value === "1";
}

if (require.main === module) {
  startServer()
    .then(({ config }) => {
      console.log(
        `Local relay listening on http://${config.host}:${config.port} (mock_mode=${config.mockMode})`
      );
    })
    .catch((error) => {
      console.error(error);
      process.exitCode = 1;
    });
}

module.exports = {
  createConfigFromEnv,
  createServer,
  startServer
};
