// Bio DynamX - Professional Audio Engine for Gemini Live
// Features: Low-latency capture (512 samples) and Look-ahead Scheduling for playback.

export class AudioStream {
    private audioContext: AudioContext | null = null;
    private processor: ScriptProcessorNode | null = null;
    private stream: MediaStream | null = null;
    private onAudioData: (base64: string) => void;
    private inputSampleRate = 16000;
    private outputSampleRate = 24000;

    // Playback scheduling variables
    private nextStartTime = 0;
    private bufferQueue: Float32Array[] = [];

    constructor(onAudioData: (base64: string) => void) {
        this.onAudioData = onAudioData;
    }

    async start() {
        this.audioContext = new (window.AudioContext || (window as any).webkitAudioContext)({
            sampleRate: this.inputSampleRate,
            latencyHint: 'interactive'
        });

        this.stream = await navigator.mediaDevices.getUserMedia({
            audio: {
                echoCancellation: { ideal: true },
                noiseSuppression: { ideal: true },
                autoGainControl: { ideal: true }
            }
        });

        const source = this.audioContext.createMediaStreamSource(this.stream);

        // Low-latency capture: 512 samples (~32ms at 16kHz)
        this.processor = this.audioContext.createScriptProcessor(512, 1, 1);

        this.processor.onaudioprocess = (e) => {
            const inputData = e.inputBuffer.getChannelData(0);
            const pcmBuffer = this.floatTo16BitPCM(inputData);
            const bytes = new Uint8Array(pcmBuffer);
            let binary = '';
            for (let i = 0; i < bytes.byteLength; i++) {
                binary += String.fromCharCode(bytes[i]);
            }
            this.onAudioData(btoa(binary));
        };

        source.connect(this.processor);

        // Final Echo Kill: Create an unconnected GainNode to keep the processor active
        // without ANY path to the destination. Chrome triggers onaudioprocess 
        // as long as the processor is part of an active stream source.
        const drain = this.audioContext.createGain();
        drain.gain.value = 0;
        this.processor.connect(drain);
        // Do NOT connect drain to context.destination.

        if (this.audioContext.state === 'suspended') {
            await this.audioContext.resume();
        }

        this.nextStartTime = this.audioContext.currentTime;
    }

    stop() {
        this.stream?.getTracks().forEach(track => track.stop());
        this.processor?.disconnect();
        this.audioContext?.close();
        this.audioContext = null;
        this.nextStartTime = 0;
    }

    private floatTo16BitPCM(input: Float32Array): ArrayBuffer {
        let i = input.length;
        const output = new Int16Array(i);
        while (i--) {
            const s = Math.max(-1, Math.min(1, input[i]));
            output[i] = s < 0 ? s * 0x8000 : s * 0x7FFF;
        }
        return output.buffer;
    }

    // Look-ahead Scheduling: Queues chunks back-to-back with precise timing
    async playChunk(base64Data: string) {
        if (!this.audioContext) return;

        try {
            const binary = atob(base64Data);
            const bytes = new Uint8Array(binary.length);
            for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);

            if (bytes.buffer.byteLength < 2) return;

            const evenLength = bytes.buffer.byteLength - (bytes.buffer.byteLength % 2);
            const pcm16 = new Int16Array(bytes.buffer, 0, evenLength / 2);
            const float32 = new Float32Array(pcm16.length);
            for (let i = 0; i < pcm16.length; i++) float32[i] = pcm16[i] / 32768;

            const buffer = this.audioContext.createBuffer(1, float32.length, this.outputSampleRate);
            buffer.getChannelData(0).set(float32);

            const source = this.audioContext.createBufferSource();
            source.buffer = buffer;
            source.connect(this.audioContext.destination);

            // Schedule to start exactly when the previous chunk ends
            const now = this.audioContext.currentTime;
            const startTime = Math.max(now, this.nextStartTime);

            source.start(startTime);
            this.nextStartTime = startTime + buffer.duration;

        } catch (err) {
            console.warn('Playback scheduling error:', err);
        }
    }
}
