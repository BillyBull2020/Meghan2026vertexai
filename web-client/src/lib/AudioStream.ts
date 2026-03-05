// Bio DynamX - Native Audio Provider for Vertex AI
// Input: 16kHz PCM | Output: 24kHz PCM

export class AudioStream {
    private audioContext: AudioContext | null = null;
    private processor: ScriptProcessorNode | null = null;
    private stream: MediaStream | null = null;
    private onAudioData: (base64: string) => void;
    private inputSampleRate = 16000; // Model Requirement
    private outputSampleRate = 24000; // Model Requirement

    constructor(onAudioData: (base64: string) => void) {
        this.onAudioData = onAudioData;
    }

    async start() {
        // Create context at 16kHz - this is the "Static Killer" pattern.
        this.audioContext = new (window.AudioContext || (window as any).webkitAudioContext)({
            sampleRate: this.inputSampleRate,
        });

        this.stream = await navigator.mediaDevices.getUserMedia({ audio: true });
        const source = this.audioContext.createMediaStreamSource(this.stream);

        // Capture at 16kHz directly.
        this.processor = this.audioContext.createScriptProcessor(4096, 1, 1);

        this.processor.onaudioprocess = (e) => {
            const inputData = e.inputBuffer.getChannelData(0);

            // Convert float32 -> PCM16 (16kHz)
            const pcmBuffer = this.floatTo16BitPCM(inputData);
            const bytes = new Uint8Array(pcmBuffer);
            let binary = '';
            for (let i = 0; i < bytes.byteLength; i++) {
                binary += String.fromCharCode(bytes[i]);
            }
            this.onAudioData(btoa(binary));
        };

        source.connect(this.processor);
        this.processor.connect(this.audioContext.destination);

        if (this.audioContext.state === 'suspended') {
            await this.audioContext.resume();
        }
    }

    stop() {
        this.stream?.getTracks().forEach(track => track.stop());
        this.processor?.disconnect();
        this.audioContext?.close();
    }

    // The "Static Killer" - Converts browser float audio to raw 16_bit PCM
    private floatTo16BitPCM(input: Float32Array): ArrayBuffer {
        let i = input.length;
        const output = new Int16Array(i);
        while (i--) {
            const s = Math.max(-1, Math.min(1, input[i]));
            output[i] = s < 0 ? s * 0x8000 : s * 0x7FFF;
        }
        return output.buffer;
    }

    // Playback logic: create buffer at 24kHz, browser resamples automatically to 16kHz context
    async playChunk(base64Data: string) {
        if (!this.audioContext) return;
        try {
            const binary = atob(base64Data);
            const bytes = new Uint8Array(binary.length);
            for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);

            if (bytes.buffer.byteLength < 2) return;

            // Trim to even byte count
            const evenLength = bytes.buffer.byteLength - (bytes.buffer.byteLength % 2);
            const pcm16 = new Int16Array(bytes.buffer, 0, evenLength / 2);
            const float32 = new Float32Array(pcm16.length);
            for (let i = 0; i < pcm16.length; i++) float32[i] = pcm16[i] / 32768;

            const buffer = this.audioContext.createBuffer(1, float32.length, this.outputSampleRate);
            buffer.getChannelData(0).set(float32);

            const source = this.audioContext.createBufferSource();
            source.buffer = buffer;
            source.connect(this.audioContext.destination);
            source.start();
        } catch (err) {
            console.warn('Audio playback error (skipping chunk):', err);
        }
    }
}
