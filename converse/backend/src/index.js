import express from "express";
import cors from "cors";
import dotenv from "dotenv";
import { handleMessage, getResults } from "./controller.js";

dotenv.config();

const app = express();
app.use(cors());
app.use(express.json());

app.post("/chat", handleMessage);
app.get("/results", getResults);

app.listen(3000, () => {
  console.log("Backend running on http://localhost:3000");
});
