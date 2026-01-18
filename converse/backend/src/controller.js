import { createSession, sendMessage } from "./chatSession.js";
import { executeWorkflow } from "./workflow.js";

const sessions = new Map();
const results = new Map();

// Simple intent gate (deterministic)
function isActionIntent(message) {
  const keywords = [
    "buy",
    "trade",
    "swap",
    "audit",
    "check safety",
    "analyze token",
    "portfolio",
    "balance"
  ];
  return keywords.some(k => message.toLowerCase().includes(k));
}

export async function handleMessage(req, res) {
  const { userId, message } = req.body;

  if (!userId || !message) {
    return res.status(400).json({ error: "userId and message are required" });
  }

  try {
    // 🔹 ACTION INTENT → WORKFLOW
    if (isActionIntent(message)) {
      const executionId = await executeWorkflow(message);

      const resultData = {
        status: "accepted",
        executionId,
        startedAt: new Date()
      };

      results.set(userId, resultData);
      return res.json(resultData);
    }

    // 🔹 CHAT ONLY
    let sessionId = sessions.get(userId);
    if (!sessionId) {
      sessionId = await createSession(userId);
      sessions.set(userId, sessionId);
    }

    const chatResponse = await sendMessage(sessionId, message);

    const resultData = {
      status: "done",
      chatResponse,
      completedAt: new Date()
    };

    results.set(userId, resultData);
    return res.json(resultData);

  } catch (err) {
    console.error(`Error handling message for user ${userId}:`, err);

    const errorData = {
      status: "error",
      error: err.message,
      failedAt: new Date()
    };

    results.set(userId, errorData);
    return res.status(500).json(errorData);
  }
}

export function getResults(req, res) {
  const { userId } = req.query;

  if (!userId) {
    return res.status(400).json({ error: "userId query parameter required" });
  }

  const result = results.get(userId);

  if (!result) {
    return res.json({
      status: "not_found",
      message: "No result found for this user"
    });
  }

  res.json(result);
}
