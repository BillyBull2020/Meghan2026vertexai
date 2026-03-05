// Bio DynamX — Dual-Context Audio Engine
// Capture context: 16kHz mic input → PCM16 → Gemini (ScriptProcessor fires because it connects to destination)
// Playback context: 24kHz PCM16 from Gemini → speakers (separate context, no echo)

export class AudioStream {
    // Capture (input) context — runs at 16kHz
    private captureContext: AudioContext | null = null;
    private processor: ScriptProcessorNode | null = null;
    private stream: MediaStream | null = null;

    // Playback (output) context — runs at 24kHz
    private playbackContext: AudioContext | null = null;
    private nextStartTime = 0;

    private onAudioData: (base64: string) => void;

    constructor(onAudioData: (base64: string) => void) {
        this.onAudioData = onAudioData;
    }

    async start() {
        // 1. Capture context at 16kHz (what Gemini expects as input)
        this.captureContext = new AudioContext({ sampleRate: 16000, latencyHint: 'interactive' });

        // 2. Mic with hardware echo cancellation
        this.stream = await navigator.mediaDevices.getUserMedia({
            audio: {
                echoCancellation: true,
                noiseSuppression: true,
                autoGainControl: true,
                sampleRate: { ideal: 16000 },
            }
        });

        const source = this.captureContext.createMediaStreamSource(this.stream);
        this.processor = this.captureContext.createScriptProcessor(4096, 1, 1);

        this.processor.onaudioprocess = (e) => {
            const inputData = e.inputBuffer.getChannelData(0);
            const pcm16 = this.floatTo16BitPCM(inputData);
            const bytes = new Uint8Array(pcm16);
            let binary = '';
            for (let i = 0; i < bytes.byteLength; i++) {
                binary += String.fromCharCode(bytes[i]);
            }
            this.onAudioData(btoa(binary));
        };

        // CRITICAL: Connect processor → destination so onaudioprocess fires.
        // This does NOT cause echo because we are using a SEPARATE playback context
        // for Gemini's output, so the capture chain never hears Gemini's audio.
        source.connect(this.processor);
        this.processor.connect(this.captureContext.destination);

        if (this.captureContext.state === 'suspended') {
            await this.captureContext.resume();
        }

        // 3. Separate playback context at 24kHz (what Gemini sends as output)
        this.playbackContext = new AudioContext({ sampleRate: 24000 });
        this.nextStartTime = this.playbackContext.currentTime;
    }

    stop() {
        this.stream?.getTracks().forEach(t => t.stop());
        this.processor?.disconnect();
        this.captureContext?.close();
        this.playbackContext?.close();
        this.captureContext = null;
        this.playbackContext = null;
        this.nextStartTime = 0;
    }

    private floatTo16BitPCM(input: Float32Array): ArrayBuffer {
        const output = new Int16Array(input.length);
        for (let i = 0; i < input.length; i++) {
            const s = Math.max(-1, Math.min(1, input[i]));
            output[i] = s < 0 ? s * 0x8000 : s * 0x7FFF;
        }
        return output.buffer;
    }

    // Play a base64-encoded PCM16 chunk from Gemini
    async playChunk(base64Data: string) {
        if (!this.playbackContext) return;
        try {
            const binary = atob(base64Data);
            const bytes = new Uint8Array(binary.length);
            for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
            if (bytes.length < 2) return;

            const evenLen = bytes.length - (bytes.length % 2);
            const pcm16 = new Int16Array(bytes.buffer, 0, evenLen / 2);
            const float32 = new Float32Array(pcm16.length);
            for (let i = 0; i < pcm16.length; i++) float32[i] = pcm16[i] / 32768;

            const buffer = this.playbackContext.createBuffer(1, float32.length, 24000);
            buffer.getChannelData(0).set(float32);

            const src = this.playbackContext.createBufferSource();
            src.buffer = buffer;
            src.connect(this.playbackContext.destination);

            const now = this.playbackContext.currentTime;
            const startAt = Math.max(now, this.nextStartTime);
            src.start(startAt);
            this.nextStartTime = startAt + buffer.duration;
        } catch (err) {
            console.warn('Playback error:', err);
        }
    }
}
