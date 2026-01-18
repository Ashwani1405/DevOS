import express from "express";
import cors from "cors";
import dotenv from "dotenv";
import fetch from "node-fetch";
import { handleMessage, getResults } from "./controller.js";

dotenv.config();

const app = express();

app.use(cors());
app.use(express.json());

// 🔹 Chat / Workflow entry point
app.post("/chat", handleMessage);

// 🔹 Poll results
app.get("/results", getResults);

// 🔹 Text-to-Speech endpoint
app.post("/tts", async (req, res) => {
  try {
    const { text } = req.body;

    if (!text || !text.trim()) {
      return res.status(400).json({ error: "Text is required" });
    }

    const apiKey = process.env.ONDEMAND_API_KEY;
    if (!apiKey) {
      return res.json({
        useBrowserTTS: true,
        text,
        message: "Using browser text-to-speech (API key missing)"
      });
    }

    const response = await fetch(
      "https://api.on-demand.io/services/v1/public/service/execute/text_to_speech",
      {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          apikey: apiKey,
        },
        body: JSON.stringify({
          text: text.substring(0, 4096),
          voice: "nova",
        }),
      }
    );

    if (!response.ok) {
      const errorData = await response.text();
      console.error("TTS API error:", response.status, errorData);

      return res.json({
        useBrowserTTS: true,
        text,
        message: "Using browser text-to-speech (service unavailable)"
      });
    }

    const data = await response.json();
    res.json(data);

  } catch (err) {
    console.error("TTS Error:", err);
    res.json({
      useBrowserTTS: true,
      text: req.body?.text,
      message: "Using browser text-to-speech"
    });
  }
});

// 🔹 Health check
app.get("/health", (_, res) => {
  res.json({ status: "ok" });
});

const PORT = process.env.PORT || 3000;
app.listen(PORT, () => {
  console.log(`Backend running on http://localhost:${PORT}`);
});
