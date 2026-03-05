# 🧪 Ironclaw Factory: Twilio Integration & Testing Guide

Your Ironclaw Factory is now equipped with a **Twilio Media Stream Bridge**. This allows any phone call to be natively routed to a Vertex AI Gemini Live agent.

## 1. Configure Twilio Webhook

To route calls to your agents, set your Twilio Phone Number's **Incoming Voice** webhook to:

| Endpoint | URL |
| :--- | :--- |
| **Jenny (Sales)** | `https://ironclaw-factory-252305301168.us-central1.run.app/twiml?agent_id=template_neuro_sales_01` |
| **Mark (ROI Closer)** | `https://ironclaw-factory-252305301168.us-central1.run.app/twiml?agent_id=mark_roi_closer_01` |
| **Aria (Reception)** | `https://ironclaw-factory-252305301168.us-central1.run.app/twiml?agent_id=aria_receptionist_01` |
| **Jules (Architect)** | `https://ironclaw-factory-252305301168.us-central1.run.app/twiml?agent_id=jules_architect_01` |

> [!NOTE]
> Ensure the **HTTP Method** is set to `POST` (Ironclaw supports both GET and POST).

## 2. Testing via Web (Direct Link)

You can now also talk to the agents directly from your website or a browser. I've built a **Web Interface** for you.

1. **Start the Web Client**:
    - Open a new terminal.
    - Run `cd web-client && npm run dev`.
2. **Open in Browser**:
    - Visit the local URL provided (usually `http://localhost:5173`).
3. **Interact**:
    - Click the **Teal Orb** in the center to establish a "Neural Link".
    - Grant microphone permissions.
    - You are now talking to **Jenny** directly with high-fidelity audio!

## 3. Testing via Twilio (Phone)

You can verify the factory status and connectivity via these links:

- **Registry Status**: [Check loaded profiles](https://ironclaw-factory-252305301168.us-central1.run.app/status)
- **Health Check**: [Service Liveness](https://ironclaw-factory-252305301168.us-central1.run.app/healthz)

## 3. How the Media Stream Works

When the call starts:

1. **Twilio** hits `/twiml`, which returns a `<Connect><Stream>` instruction.
2. **Twilio** opens a WebSocket to `/media-stream`.
3. **Ironclaw** instantly spawns a **Vertex AI Gemini Live** session.
4. **Audio Bridge**: Ironclaw converts Twilio's **8kHz mu-law** audio to Vertex's **16kHz PCM16** (and vice versa) in real-time with <20ms latency.
5. **Agent Live**: You are now speaking directly to the AI.

## 4. Troubleshooting

If you don't hear audio:

- Check the **Cloud Run Logs** in the GCP Console. Look for `Twilio stream started` or `Vertex AI connected`.
- Verify that your Gemini API key/ADC has access to the `us-central1` region.
- Ensure the agent ID in the URL matches a loaded YAML profile.
