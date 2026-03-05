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
        // Create context at 24kHz - this matches the model output exactly
        this.audioContext = new (window.AudioContext || (window as any).webkitAudioContext)({
            sampleRate: 24000,
        });

        this.stream = await navigator.mediaDevices.getUserMedia({ audio: true });
        const source = this.audioContext.createMediaStreamSource(this.stream);

        // We capture at 24kHz but the model wants 16kHz input.
        // ScriptProcessor will automatically resample to context rate, 
        // so we must send a 16kHz chunk.
        this.processor = this.audioContext.createScriptProcessor(4096, 1, 1);

        this.processor.onaudioprocess = (e) => {
            const inputData = e.inputBuffer.getChannelData(0);

            // Basic decimation from 24kHz to 16kHz (factor of 1.5)
            // or we just send 24kHz if the model is gemini-live-2.5-flash-native-audio
            // Re-evaluating based on Jules standards: Input 16kHz PCM
            const pcm16Data = this.downsampleTo16k(inputData, 24000);

            const pcmBuffer = this.floatTo16BitPCM(pcm16Data);
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

    private downsampleTo16k(input: Float32Array, fromRate: number): Float32Array {
        if (fromRate === 16000) return input;
        const ratio = fromRate / 16000;
        const length = Math.floor(input.length / ratio);
        const result = new Float32Array(length);
        for (let i = 0; i < length; i++) {
            result[i] = input[Math.floor(i * ratio)];
        }
        return result;
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

    // Playback logic for the 24kHz response
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

            const buffer = this.audioContext.createBuffer(1, float32.length, 24000);
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
