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
  if (!serverHandle?.server) {
    return;
  }

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
  assert.ok(body.routes.includes("POST /api/armoriq/verify"));
  assert.ok(body.routes.includes("POST /api/gemini/terminal-validator"));
});

test("armoriq verify endpoint allows matching terminal input in mock mode", async () => {
  const response = await fetch(`${baseUrl}/api/armoriq/verify`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      player_input: "run containment override",
      action: "terminal_override",
      context: {
        hidden_answer: "containment override"
      }
    })
  });

  assert.equal(response.status, 200);
  const body = await response.json();
  assert.equal(body.allowed, true);
  assert.equal(body.block_reason, null);
});

test("armoriq verify endpoint blocks non-matching terminal input in mock mode", async () => {
  const response = await fetch(`${baseUrl}/api/armoriq/verify`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      player_input: "open hangar bay",
      action: "terminal_override",
      context: {
        hidden_answer: "containment override"
      }
    })
  });

  assert.equal(response.status, 200);
  const body = await response.json();
  assert.equal(body.allowed, false);
  assert.match(body.block_reason, /policy/i);
});

test("clue generator endpoint returns the required JSON contract", async () => {
  const response = await fetch(`${baseUrl}/api/gemini/clue-generator`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      setting: "a derelict orbital lab",
      difficulty: "hard",
      villainName: "The Entity",
      theme: "industrial cosmic horror"
    })
  });

  assert.equal(response.status, 200);
  const body = await response.json();
  assert.equal(typeof body.game_title, "string");
  assert.equal(typeof body.shared_manual_intro, "string");
  assert.equal(typeof body.round_1.player_1_ui.bootup_dialogue, "string");
  assert.ok(Array.isArray(body.round_1.player_1_ui.clue_sequence));
  assert.ok(Array.isArray(body.round_2.player_2_manual.flowchart));
  assert.equal(body.round_3.kill_phrase_3, body.round_3.validation_answer);
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
      scene: "the players have almost solved the puzzle",
      selected_cue_id: "v_r2_c1"
    })
  });

  assert.equal(response.status, 200);
  const body = await response.json();
  assert.ok(Array.isArray(body.speech_cues));
  assert.ok(body.speech_cues.length >= 3);
  assert.equal(body.selected_cue_id, "v_r2_c1");
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

test("armoriq verify rejects malformed requests", async () => {
  const response = await fetch(`${baseUrl}/api/armoriq/verify`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      player_input: "run containment override",
      context: {}
    })
  });

  assert.equal(response.status, 400);
  const body = await response.json();
  assert.match(body.error, /action|context\.hidden_answer/);
});
