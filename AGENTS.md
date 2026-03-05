# AGENTS.md: Bio DynamX Factory Protocols

## Team Roster

### @rust-engineer

**Focus:** Ironclaw core engine — Tokio-based async worker nodes, Vertex AI WebSocket
streams via `tokio-tungstenite`, the `notify` directory watcher for hot-reloading agent
profiles, and the concurrent session manager with `Arc<RwLock<>>` shared state.

**Standards:**

- All public APIs must use `Result<T, IronclawError>` — no unwraps in production paths.
- WebSocket connections must implement the silent keep-alive heartbeat pattern (160 samples
  of PCM16 silence every 2s during long tool calls).
- Session lifecycle: `Spawning → Connected → Setup → Live → Draining → Closed`.

### @prompt-engineer

**Focus:** YAML template configurations for agent personas. Embed the Old Brain survival
triggers, Limbic resonance, and Neocortex justification into every `neuro_system_prompt`.

**Standards:**

- Each profile MUST address the Triune Brain hierarchy in order: Reptilian → Limbic → Neocortex.
- Follow Miller's Law — keep spoken sentences under 7±2 concepts.
- Use SPIN (Situation, Problem, Implication, Need-payoff) frameworks in sales scripts.
- All claims must be backed by real data or labeled as examples (Ethical Standard #15).

### @devops-agent

**Focus:** Containerization via multi-stage Docker builds, deployment to GCP Cloud Run
with mounted Cloud Storage volumes for the `profiles/` directory, and CI/CD via Cloud Build.

**Standards:**

- Container images must use the `scratch` or `distroless` final stage for minimal attack surface.
- The `profiles/` volume mount must be read-only from the container's perspective.
- Health check endpoint at `/healthz` must return 200 with agent registry count.
- Structured JSON logging via `tracing-subscriber` for Cloud Logging integration.

---

## Bio DynamX Vertex AI Standards

### Model Selection (Live API)

| Use Case | Model ID | Notes |
| :--- | :--- | :--- |
| **Voice Agents (Production)** | `gemini-live-2.5-flash-native-audio` | Stable native audio. **Never** use `-preview` or `-09-2025` variants. |
| **Text/Chat (non-voice)** | `gemini-2.5-flash` | Standard text generation. Not compatible with Live API WebSocket. |
| **Fallback (if stable is down)** | `gemini-live-2.5-flash-native-audio` | Last-resort Live API model. Confirmed working on `v1beta1` endpoint. |

> [!CAUTION]
> The following model IDs are **rejected** by the Live API WebSocket (`BidiGenerateContent`):
> `gemini-2.0-flash`, `gemini-2.0-flash-001`, `gemini-2.0-flash-exp`, `gemini-2.5-flash`,
> `gemini-2.5-flash-native-audio-preview`. Do **not** use these for voice sessions.

### WebSocket Endpoint

```
wss://{LOCATION}-aiplatform.googleapis.com/ws/google.cloud.aiplatform.v1beta1.LlmBidiService/BidiGenerateContent
```

- Authentication: `?access_token={ADC_TOKEN}` appended to the URL.
- Location: `us-central1` (production).
- Project: `bio-dynamx` (via `GOOGLE_CLOUD_PROJECT` env var).

### Setup Message Schema (camelCase Required)

The Live API WebSocket expects **camelCase** JSON field names. The Rust structs use
`#[serde(rename_all = "camelCase")]` to enforce this.

```json
{
  "setup": {
    "model": "projects/bio-dynamx/locations/us-central1/publishers/google/models/gemini-live-2.5-flash-native-audio",
    "generationConfig": {
      "responseModalities": ["AUDIO"],
      "speechConfig": {
        "voiceConfig": {
          "prebuiltVoiceConfig": {
            "voiceName": "Aoede"
          }
        }
      }
    },
    "systemInstruction": {
      "parts": [{ "text": "..." }]
    },
    "realtimeInputConfig": {
      "automaticActivityDetection": {
        "disabled": true
      }
    }
  }
}
```

> [!IMPORTANT]
>
> - `voiceName: "Aoede"` — Recommended HD voice for Bio DynamX agents.
> - `automaticActivityDetection.disabled = true` — Prevents the "machine scream" feedback loop.

> [!CAUTION]
> `runtimeConfig` / `audioConfiguration` (startSensitivity, endSensitivity) is **NOT supported**
> by the Live API setup message. Including it causes `Unknown name "runtimeConfig"` and instant
> connection death. Do not add it.

### Audio Response Extraction

Vertex AI sends audio inside a nested JSON structure. You **must** parse it — do not
treat the raw JSON as audio data (this causes static noise).

```
serverContent.modelTurn.parts[].inlineData.data → base64 PCM16 @ 24kHz
```

---

## 16kHz Audio Bridge (The "Static Killer")

### The Problem

Browser AudioContexts default to 44.1kHz or 48kHz. The Gemini Live API expects **16kHz
PCM16 input** and returns **24kHz PCM16 output**. Mismatched sample rates produce static,
distortion, or a "screaming radio" effect.

### The Fix: BioAudioStreamer Pattern

```typescript
// Bio DynamX - Native Audio Provider for Vertex AI
// Input: 16kHz PCM | Output: 24kHz PCM

const INPUT_RATE = 16000;  // Model requirement — mic capture
const OUTPUT_RATE = 24000; // Model requirement — speaker playback

// 1. Force the AudioContext to 16kHz for recording
const audioContext = new AudioContext({ sampleRate: INPUT_RATE });

// 2. Convert float32 → 16-bit PCM (the "Static Killer")
function floatTo16BitPCM(input: Float32Array): ArrayBuffer {
  let i = input.length;
  const output = new Int16Array(i);
  while (i--) {
    const s = Math.max(-1, Math.min(1, input[i]));
    output[i] = s < 0 ? s * 0x8000 : s * 0x7FFF;
  }
  return output.buffer;
}

// 3. Playback: create buffer at 24kHz, browser resamples automatically
function playResponse(ctx: AudioContext, base64Data: string) {
  const binary = atob(base64Data);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);

  const pcm16 = new Int16Array(bytes.buffer);
  const float32 = new Float32Array(pcm16.length);
  for (let i = 0; i < pcm16.length; i++) float32[i] = pcm16[i] / 32768;

  const buffer = ctx.createBuffer(1, float32.length, OUTPUT_RATE);
  buffer.getChannelData(0).set(float32);

  const source = ctx.createBufferSource();
  source.buffer = buffer;
  source.connect(ctx.destination);
  source.start();
}
```

### Anti-Feedback Rules

1. **Use headphones** during testing — speaker output will feed back into the mic.
2. **VAD disabled** server-side (`automaticActivityDetection.disabled = true`).
3. **Echo cancellation**: The browser's `getUserMedia({ audio: true })` enables AEC by
   default on most browsers. Do not override with `echoCancellation: false`.
4. **Int16Array guard**: Always trim incoming buffer to even byte length before constructing
   `Int16Array` — Vertex may send odd-length chunks that crash the player.

### Key Files

| File | Purpose |
| :--- | :--- |
| `web-client/src/lib/AudioStream.ts` | BioAudioStreamer — mic capture (16kHz) + playback (24kHz) |
| `src/vertex_client.rs` | WebSocket connection, setup message, audio read/write loops |
| `src/models.rs` | Rust structs with `#[serde(rename_all = "camelCase")]` for Live API |
| `src/main.rs` | Message bridge — extracts `inlineData.data` from Vertex JSON |
| `profiles/*.yaml` | Agent configs — model ID, voice, system prompt |

---

## Deployment Checklist

```bash
# 1. Sync profiles to Cloud Storage
gsutil -m rsync -r profiles/ gs://biodynamx-agent-profiles

# 2. Build + push container
gcloud builds submit --tag us-central1-docker.pkg.dev/bio-dynamx/cloud-run-source-deploy/ironclaw:latest .

# 3. Deploy new revision
gcloud run services update ironclaw-factory \
  --image us-central1-docker.pkg.dev/bio-dynamx/cloud-run-source-deploy/ironclaw:latest \
  --region us-central1

# 4. Verify
curl https://ironclaw-factory-252305301168.us-central1.run.app/status
```

> [!TIP]
> To force a profile reload without rebuilding the container, update an env var:
> `gcloud run services update ironclaw-factory --region us-central1 --set-env-vars REFRESH=$(date +%s)`
