import { createSession, sendMessage } from "./chatSession.js";
import { executeWorkflow } from "./workflow.js";

const sessions = new Map();
const results = new Map();

export async function handleMessage(req, res) {
  const { userId, message } = req.body;

  // ✅ Respond immediately
  res.json({
    status: "accepted",
    message: "Processing request"
  });

  // 🔁 Async work after response
  (async () => {
    try {
      // Mark as processing
      results.set(userId, { status: "processing", startedAt: new Date() });

      let sessionId = sessions.get(userId);
      if (!sessionId) {
        sessionId = await createSession(userId);
        sessions.set(userId, sessionId);
      }

      const chatResponse = await sendMessage(sessionId, message);
      const workflowResult = await executeWorkflow(message);

      // Store results - only include workflowResult if it's not null
      const resultData = {
        status: "done",
        chatResponse,
        completedAt: new Date()
      };
      
      if (workflowResult) {
        resultData.workflowResult = workflowResult;
      }

      results.set(userId, resultData);
    } catch (err) {
      console.error(`Async processing error for user ${userId}:`, err);
      
      // Store error
      results.set(userId, {
        status: "error",
        error: err.message,
        failedAt: new Date()
      });
    }
  })();
}

export function getResults(req, res) {
  const { userId } = req.query;

  if (!userId) {
    return res.status(400).json({ error: "userId query parameter required" });
  }

  const result = results.get(userId);

  if (!result) {
    return res.json({ status: "not_found", message: "No request found for this user" });
  }

  res.json(result);
}
