// Microphone service for shout detection using Web Audio API
import { eventBus, Events } from './event-bus';

interface ShoutDetectionConfig {
  threshold: number; // Amplitude threshold for shout detection (0-1)
  smoothingFactor: number; // FFT smoothing (0-1)
  fftSize: number; // FFT size for frequency analysis
  updateIntervalMs: number; // How often to check audio levels
  enabled: boolean;
}

class MicrophoneService {
  private config: ShoutDetectionConfig = {
    threshold: 0.3, // 30% threshold
    smoothingFactor: 0.8,
    fftSize: 2048,
    updateIntervalMs: 50, // 20 Hz update rate
    enabled: false,
  };

  private audioContext: AudioContext | null = null;
  private analyser: AnalyserNode | null = null;
  private microphone: MediaStreamAudioSourceNode | null = null;
  private mediaStream: MediaStream | null = null;
  private intervalId: number | null = null;
  private dataArray: Uint8Array | null = null;

  async start(): Promise<void> {
    if (this.config.enabled) {
      console.log('Microphone already started');
      return;
    }

    try {
      // Request microphone access
      this.mediaStream = await navigator.mediaDevices.getUserMedia({
        audio: {
          echoCancellation: true,
          noiseSuppression: true,
          autoGainControl: false,
        },
      });

      // Create audio context and analyser
      this.audioContext = new AudioContext();
      this.analyser = this.audioContext.createAnalyser();
      this.analyser.fftSize = this.config.fftSize;
      this.analyser.smoothingTimeConstant = this.config.smoothingFactor;

      // Connect microphone to analyser
      this.microphone =
        this.audioContext.createMediaStreamSource(this.mediaStream);
      this.microphone.connect(this.analyser);

      // Create data array for amplitude analysis
      const bufferLength = this.analyser.frequencyBinCount;
      this.dataArray = new Uint8Array(bufferLength);

      // Start monitoring
      this.intervalId = window.setInterval(
        this.analyzeAudio.bind(this),
        this.config.updateIntervalMs
      );

      this.config.enabled = true;
      console.log('Microphone service started');
    } catch (error) {
      console.error('Failed to start microphone:', error);
      throw error;
    }
  }

  async stop(): Promise<void> {
    if (!this.config.enabled) {
      return;
    }

    // Stop monitoring
    if (this.intervalId !== null) {
      clearInterval(this.intervalId);
      this.intervalId = null;
    }

    // Disconnect audio nodes
    if (this.microphone) {
      this.microphone.disconnect();
      this.microphone = null;
    }

    // Stop media stream
    if (this.mediaStream) {
      this.mediaStream.getTracks().forEach((track) => track.stop());
      this.mediaStream = null;
    }

    // Close audio context
    if (this.audioContext) {
      await this.audioContext.close();
      this.audioContext = null;
    }

    this.analyser = null;
    this.dataArray = null;
    this.config.enabled = false;

    console.log('Microphone service stopped');
  }

  private analyzeAudio(): void {
    if (!this.analyser || !this.dataArray) {
      return;
    }

    // Get time domain data (waveform)
    this.analyser.getByteTimeDomainData(this.dataArray as Uint8Array<ArrayBuffer>);

    // Calculate RMS (Root Mean Square) for amplitude
    let sum = 0;
    for (let i = 0; i < this.dataArray.length; i++) {
      const value = this.dataArray[i];
      if (value !== undefined) {
        const normalized = (value - 128) / 128; // Normalize to -1 to 1
        sum += normalized * normalized;
      }
    }
    const rms = Math.sqrt(sum / this.dataArray.length);

    // Normalize amplitude to 0-1 range
    const amplitude = Math.min(rms * 2, 1.0); // Scale RMS

    // Emit shout event if above threshold
    if (amplitude > this.config.threshold) {
      eventBus.emit(Events.SHOUT_DETECTED, {
        amplitude,
        timestamp: Date.now(),
      });

      console.log('Shout detected: amplitude=%f', amplitude);
    }

    // Always emit metrics update for visualization
    eventBus.emit('microphone:amplitude', {
      amplitude,
      timestamp: Date.now(),
    });
  }

  updateConfig(config: Partial<ShoutDetectionConfig>): void {
    const needsRestart =
      this.config.enabled &&
      (config.fftSize !== undefined ||
        config.smoothingFactor !== undefined ||
        config.updateIntervalMs !== undefined);

    this.config = { ...this.config, ...config };

    // Restart if FFT settings changed while running
    if (needsRestart) {
      void this.stop().then(() => this.start());
    }

    console.log('Microphone config updated:', this.config);
  }

  getConfig(): ShoutDetectionConfig {
    return { ...this.config };
  }

  isEnabled(): boolean {
    return this.config.enabled;
  }

  getCurrentAmplitude(): number {
    if (!this.analyser || !this.dataArray) {
      return 0;
    }

    this.analyser.getByteTimeDomainData(this.dataArray as Uint8Array<ArrayBuffer>);

    let sum = 0;
    for (let i = 0; i < this.dataArray.length; i++) {
      const value = this.dataArray[i];
      if (value !== undefined) {
        const normalized = (value - 128) / 128;
        sum += normalized * normalized;
      }
    }
    const rms = Math.sqrt(sum / this.dataArray.length);
    return Math.min(rms * 2, 1.0);
  }
}

// Singleton instance
export const microphoneService = new MicrophoneService();
