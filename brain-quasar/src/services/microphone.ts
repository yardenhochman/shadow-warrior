// Microphone service for shout detection using Web Audio API
import { eventBus, Events } from './event-bus';
import { Capacitor } from '@capacitor/core';

interface ShoutDetectionConfig {
  shoutThreshold: number; // Amplitude threshold for shout detection (0-1)
  presenceDetectionThreshold: number; // Amplitude threshold for presence detection (0-1)
  smoothingFactor: number; // FFT smoothing (0-1)
  fftSize: number; // FFT size for frequency analysis
  updateIntervalMs: number; // How often to check audio levels
  enabled: boolean;
  gain: number; // Audio gain multiplier (0.1-10.0, default 1.5)
  // getUserMedia audio constraints (require restart to apply)
  echoCancellation: boolean; // Remove echo (default false)
  noiseSuppression: boolean; // Reduce background noise (default true)
  autoGainControl: boolean; // Automatic volume adjustment (default false)
  // Optional filtering config for shout frequency band (20-150 Hz by default)
  filterEnabled?: boolean;
  filterLowHz?: number;
  filterHighHz?: number;
}

class MicrophoneService {
  private config: ShoutDetectionConfig = {
    shoutThreshold: 0.3, // 30% threshold
    presenceDetectionThreshold: 0.1, // 10% presence detection
    smoothingFactor: 0.8,
    fftSize: 2048,
    updateIntervalMs: 50, // 20 Hz update rate
    enabled: false,
    gain: 1.5, // 1.5x gain by default
    echoCancellation: false,
    noiseSuppression: false,
    autoGainControl: false,
    // Default filter settings target a wider shout band by default
    filterEnabled: true,
    filterLowHz: 150,
    filterHighHz: 800,
  };

  private audioContext: AudioContext | null = null;
  private analyser: AnalyserNode | null = null;
  private microphone: MediaStreamAudioSourceNode | null = null;
  private gainNode: GainNode | null = null;
  private highpassFilter: BiquadFilterNode | null = null;
  private lowpassFilter: BiquadFilterNode | null = null;
  private mediaStream: MediaStream | null = null;
  private intervalId: number | null = null;
  private dataArray: Uint8Array | null = null;

  async start(): Promise<void> {
    if (this.config.enabled) {
      console.log('Microphone already started');
      return;
    }

    try {
      console.log('Requesting microphone access via getUserMedia...');
      // Request microphone access - this will prompt for permission if needed
      this.mediaStream = await navigator.mediaDevices.getUserMedia({
        audio: {
          echoCancellation: this.config.echoCancellation,
          noiseSuppression: this.config.noiseSuppression,
          autoGainControl: this.config.autoGainControl,
        },
      });
      console.log('getUserMedia successful, tracks:', this.mediaStream.getAudioTracks().length);

      // Create audio context AFTER getUserMedia succeeds (important for mobile)
      console.log('Creating AudioContext...');
      this.audioContext = new AudioContext();
      console.log('AudioContext created, state:', this.audioContext.state);

      // Resume audio context on mobile (required after user interaction)
      if (Capacitor.isNativePlatform() && this.audioContext.state === 'suspended') {
        console.log('Resuming audio context...');
        await this.audioContext.resume();
        console.log('AudioContext state after resume:', this.audioContext.state);
      }

      console.log('Creating analyser...');
      this.analyser = this.audioContext.createAnalyser();
      this.analyser.fftSize = this.config.fftSize;
      this.analyser.smoothingTimeConstant = this.config.smoothingFactor;

      console.log('Creating microphone source and connecting audio graph...');
      // Create microphone source
      this.microphone = this.audioContext.createMediaStreamSource(this.mediaStream);

      // Create gain node
      this.gainNode = this.audioContext.createGain();
      this.gainNode.gain.value = this.config.gain;

      // Read filter settings from config (with defaults)
      // Read optional filter settings from config with safe defaults
      const filterEnabled =
        this.config.filterEnabled !== undefined ? this.config.filterEnabled : true;
      const filterLowHz = this.config.filterLowHz !== undefined ? this.config.filterLowHz : 150;
      const filterHighHz = this.config.filterHighHz !== undefined ? this.config.filterHighHz : 800;

      if (filterEnabled) {
        // Create highpass and lowpass biquad filters to form a band-pass
        this.highpassFilter = this.audioContext.createBiquadFilter();
        this.highpassFilter.type = 'highpass';
        this.highpassFilter.frequency.value = filterLowHz;

        this.lowpassFilter = this.audioContext.createBiquadFilter();
        this.lowpassFilter.type = 'lowpass';
        this.lowpassFilter.frequency.value = filterHighHz;

        // Chain: microphone -> gain -> highpass -> lowpass -> analyser
        this.microphone.connect(this.gainNode);
        this.gainNode.connect(this.highpassFilter);
        this.highpassFilter.connect(this.lowpassFilter);
        this.lowpassFilter.connect(this.analyser);
      } else {
        // Chain: microphone -> gain -> analyser
        this.microphone.connect(this.gainNode);
        this.gainNode.connect(this.analyser);
      }

      // Create data array for amplitude analysis
      const bufferLength = this.analyser.frequencyBinCount;
      this.dataArray = new Uint8Array(bufferLength);

      // Start monitoring
      this.intervalId = window.setInterval(
        this.analyzeAudio.bind(this),
        this.config.updateIntervalMs,
      );

      this.config.enabled = true;
      console.log('Microphone service started successfully');
    } catch (error) {
      console.error('Failed to start microphone:', error);
      if (error instanceof Error) {
        console.error('Error name:', error.name);
        console.error('Error message:', error.message);

        // Provide user-friendly error messages
        if (error.name === 'NotAllowedError') {
          console.warn(
            'MICROPHONE PERMISSION DENIED: Microphone will not work, but app continues with accelerometer',
          );
          // Don't throw - let the app continue without microphone
          return;
        } else if (error.name === 'NotFoundError') {
          console.warn('No microphone found on this device');
          return;
        } else if (error.name === 'NotReadableError') {
          console.warn('Microphone is already in use by another app');
          return;
        } else {
          throw new Error(`Microphone error: ${error.message}`);
        }
      } else {
        console.error('Non-Error object thrown:', typeof error, error);
        throw new Error('Unknown microphone error occurred');
      }
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
    // Disconnect and null gain node
    if (this.gainNode) {
      try {
        this.gainNode.disconnect();
      } catch {
        /* ignore */
      }
      this.gainNode = null;
    }
    // Disconnect and null filters
    if (this.highpassFilter) {
      try {
        this.highpassFilter.disconnect();
      } catch {
        /* ignore */
      }
      this.highpassFilter = null;
    }
    if (this.lowpassFilter) {
      try {
        this.lowpassFilter.disconnect();
      } catch {
        /* ignore */
      }
      this.lowpassFilter = null;
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
      console.log('Microphone: analyser or dataArray not ready');
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

    // Debug logging - only log occasionally to avoid spam
    if (Math.random() < 0.01) {
      // Log ~1% of the time
      console.log('Microphone amplitude:', amplitude, 'RMS:', rms);
    }

    if (amplitude > this.config.presenceDetectionThreshold) {
      eventBus.emit(Events.PRESENCE_DETECTED, {});
    }

    // Emit shout event if above threshold
    if (amplitude > this.config.shoutThreshold) {
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
    this.config = { ...this.config, ...config };

    // Update gain dynamically if changed (no restart needed)
    if (config.gain !== undefined && this.gainNode) {
      this.gainNode.gain.value = config.gain;
      console.log('Gain updated to:', config.gain);
    }

    // Restart microphone if enabled (simpler than trying to detect which settings changed)
    if (this.config.enabled) {
      console.log('Microphone config changed, restarting service...');
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

  private async requestMicrophonePermission(): Promise<void> {
    // Removed - getUserMedia handles permission requests directly
  }

  async ensureAudioContextResumed(): Promise<void> {
    if (this.audioContext && this.audioContext.state === 'suspended') {
      try {
        await this.audioContext.resume();
        console.log('Audio context resumed');
      } catch (error) {
        console.error('Failed to resume audio context:', error);
      }
    }
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
