const BASE_URL = "https://maincloud.spacetimedb.com/v1/database/the-entity-ty5fs";

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function decodeOptional(value) {
  if (Array.isArray(value)) {
    return value[0] === 0 ? value[1] : null;
  }
  return value ?? null;
}

async function sql(query, token) {
  const res = await fetch(`${BASE_URL}/sql`, {
    method: "POST",
    headers: {
      "Content-Type": "text/plain",
      Authorization: `Bearer ${token}`,
    },
    body: query,
  });

  const text = await res.text();
  if (!res.ok) {
    throw new Error(`SQL ${res.status}: ${text}`);
  }
  return JSON.parse(text);
}

async function run() {
  const initRes = await fetch(`${BASE_URL}/call/initiate_room`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify([{ some: "The Entity" }]),
  });

  const hostToken = initRes.headers.get("spacetime-identity-token");
  if (!initRes.ok || !hostToken) {
    const body = await initRes.text();
    throw new Error(`initiate_room failed: ${initRes.status} ${body}`);
  }

  let roomId = null;
  for (let i = 0; i < 8; i += 1) {
    await sleep(1000);
    const ticketData = await sql("select * from room_ticket", hostToken);
    const rows = ticketData?.[0]?.rows ?? [];
    const row = rows[rows.length - 1];
    if (!row) continue;
    roomId = decodeOptional(row[1]);
    if (roomId) break;
  }

  if (!roomId) {
    throw new Error("failed to resolve room_id from room_ticket");
  }

  const joinRes = await fetch(`${BASE_URL}/call/join_room`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify([roomId]),
  });

  if (!joinRes.ok) {
    const body = await joinRes.text();
    throw new Error(`join_room failed: ${joinRes.status} ${body}`);
  }

  const genRes = await fetch(`${BASE_URL}/call/generate_clue_manual_for_room`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${hostToken}`,
    },
    body: JSON.stringify([
      roomId,
      "round_1",
      JSON.stringify({ requested_persona: "1920s Detective" }),
      "",
    ]),
  });

  const genBody = await genRes.text();
  console.log("generate_clue_manual_for_room status:", genRes.status);
  console.log("generate_clue_manual_for_room body:", genBody || "<empty>");
  console.log("room_id:", roomId);

  for (let attempt = 1; attempt <= 16; attempt += 1) {
    await sleep(2500);
    const data = await sql(
      `select artifact_key, room_id, round_key, status, response_payload_json, last_error from round_content_artifact where room_id = '${roomId}'`,
      hostToken
    );

    const row = data?.[0]?.rows?.[0];
    if (!row) {
      console.log(`poll ${attempt}: no artifact row yet`);
      continue;
    }

    const statusVal = Array.isArray(row[3]) ? row[3][0] : row[3];
    const responsePayload = decodeOptional(row[4]);
    const lastError = decodeOptional(row[5]);

    console.log(`poll ${attempt}: status=${statusVal}`);
    if (responsePayload) {
      console.log("\n=== clue/manual output (response_payload_json) ===");
      try {
        const parsed = JSON.parse(responsePayload);
        console.log(JSON.stringify(parsed, null, 2));
      } catch {
        console.log(responsePayload);
      }
      return;
    }

    if (lastError) {
      console.log("\n=== generation error ===");
      console.log(lastError);
      return;
    }
  }

  console.log("Timed out waiting for round_content_artifact response_payload_json");
}

run().catch((err) => {
  console.error("Script failed:", err.message);
  process.exit(1);
});
