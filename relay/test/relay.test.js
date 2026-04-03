const test = require("node:test");
const assert = require("node:assert/strict");
const { startServer } = require("../server");

let serverHandle;
let baseUrl;

test.before(async () => {
  serverHandle = await startServer({
    host: "127.0.0.1",
    port: 0,
    mockMode: true
  });

  const address = serverHandle.server.address();
  baseUrl = `http://${address.address}:${address.port}`;
});

test.after(async () => {
  await new Promise((resolve, reject) => {
    serverHandle.server.close((error) => (error ? reject(error) : resolve()));
  });
});

test("health endpoint reports available routes", async () => {
  const response = await fetch(`${baseUrl}/health`);
  assert.equal(response.status, 200);

  const body = await response.json();
  assert.equal(body.ok, true);
  assert.equal(body.mock_mode, true);
  assert.ok(body.routes.includes("POST /api/gemini/terminal-validator"));
});

test("clue generator endpoint returns the required JSON contract", async () => {
  const response = await fetch(`${baseUrl}/api/gemini/clue-generator`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      setting: "a derelict orbital lab",
      difficulty: "hard",
      villainName: "The Entity"
    })
  });

  assert.equal(response.status, 200);
  const body = await response.json();
  assert.equal(typeof body.villain_clue_dialogue, "string");
  assert.equal(typeof body.p2_manual_snippet, "string");
  assert.equal(typeof body.hidden_answer, "string");
});

test("terminal validator endpoint accepts matching input in mock mode", async () => {
  const response = await fetch(`${baseUrl}/api/gemini/terminal-validator`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      player_input: "run containment override",
      hidden_answer: "containment override"
    })
  });

  assert.equal(response.status, 200);
  const body = await response.json();
  assert.equal(body.success, true);
});

test("villain speech endpoint returns text plus synthesized audio payload", async () => {
  const response = await fetch(`${baseUrl}/api/villain/speech`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      villainName: "The Entity",
      scene: "the players have almost solved the puzzle"
    })
  });

  assert.equal(response.status, 200);
  const body = await response.json();
  assert.equal(typeof body.speech_text, "string");
  assert.equal(body.tts_provider, "mock");
  assert.equal(typeof body.audio_base64, "string");
});

test("terminal validator rejects malformed requests", async () => {
  const response = await fetch(`${baseUrl}/api/gemini/terminal-validator`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      player_input: ""
    })
  });

  assert.equal(response.status, 400);
  const body = await response.json();
  assert.match(body.error, /hidden_answer|player_input/);
});
