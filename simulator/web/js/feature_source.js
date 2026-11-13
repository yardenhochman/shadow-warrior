// Feature stream feeding the ported ShoutDetector and presence MicSource.
// Real mic: getUserMedia -> AudioWorklet -> FrontEnd (audio_frontend.js port).
// No mic / shout button: synthetic frames shaped like the Rust unit tests'
// bg()/shout() fixtures (shout_detector.rs tests), at the real 16 ms frame rate.
import { FrontEnd, FRAME_SECS } from './audio_frontend.js';

export class FeatureSource {
  constructor(onFrame) {
    this.onFrame = onFrame;
    this.frontEnd = null;
    this.ctx = null;
    this.stream = null;
    this.micActive = false;
    this.synthHeld = false;
    this.synthUntil = 0;
    this.synthDb = 24;
    this.playbackActive = false;
    this.index = 0;
    this.synthClock = performance.now();
    this.status = 'mic off';
  }

  async startMic(audioCtx) {
    if (!navigator.mediaDevices?.getUserMedia) throw new Error('getUserMedia unavailable (needs https or localhost)');
    this.stream = await navigator.mediaDevices.getUserMedia({
      audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false, channelCount: 1 },
    });
    this.ctx = audioCtx;
    const src = audioCtx.createMediaStreamSource(this.stream);
    this.frontEnd = new FrontEnd(audioCtx.sampleRate);
    this.frontEnd.setPlaybackActive(this.playbackActive);
    const sink = audioCtx.createGain(); sink.gain.value = 0; sink.connect(audioCtx.destination);
    const feed = (block) => {
      for (const f of this.frontEnd.process(block)) this.emit(f);
    };
    if (audioCtx.audioWorklet) {
      await audioCtx.audioWorklet.addModule(new URL('./mic-worklet.js', import.meta.url));
      const node = new AudioWorkletNode(audioCtx, 'sw-tap');
      node.port.onmessage = (e) => feed(e.data);
      src.connect(node); node.connect(sink);
    } else {
      const sp = audioCtx.createScriptProcessor(1024, 1, 1);
      sp.onaudioprocess = (e) => feed(e.inputBuffer.getChannelData(0).slice());
      src.connect(sp); sp.connect(sink);
    }
    this.micActive = true;
    this.status = `mic on @ ${audioCtx.sampleRate} Hz`;
  }

  stopMic() {
    this.stream?.getTracks().forEach((t) => t.stop());
    this.stream = null; this.micActive = false; this.frontEnd = null; this.status = 'mic off';
    this.synthClock = performance.now();
  }

  setPlaybackActive(a) { this.playbackActive = a; this.frontEnd?.setPlaybackActive(a); }
  resetFloor() { this.frontEnd?.resetFloor(); }

  /** Tap: shout for `ms`. Hold: setHeld(true/false). */
  pulse(ms = 450) { this.synthUntil = performance.now() + ms; }
  setHeld(h) { this.synthHeld = h; }
  get synthShouting() { return this.synthHeld || performance.now() < this.synthUntil; }

  emit(f) {
    f.index = this.index++;
    if (this.synthShouting) f = this.synthShout(f.index, f);
    this.onFrame(f);
  }

  /** Called every animation frame; produces synthetic frames when the mic is off. */
  pump() {
    if (this.micActive) return;
    const now = performance.now();
    let n = Math.floor((now - this.synthClock) / (FRAME_SECS * 1000));
    if (n > 30) { this.synthClock = now - 30 * FRAME_SECS * 1000; n = 30; } // tab was hidden
    for (let i = 0; i < n; i++) {
      this.synthClock += FRAME_SECS * 1000;
      const idx = this.index++;
      this.onFrame(this.synthShouting ? this.synthShout(idx) : this.synthBg(idx));
    }
  }

  synthBg(index) {
    const j = Math.random();
    return {
      index, band_db: -50 + j * 2, noise_floor_db: -50, db_above_floor: j * 2, vad_prob: 0.1,
      pitch_hz: 0, pitch_conf: 0.05, spectral_centroid_hz: 600, spectral_tilt_db_per_khz: -30,
      spectral_flatness: 0.7, playback_active: this.playbackActive, synthetic: true,
    };
  }

  synthShout(index, base) {
    const floor = base ? base.noise_floor_db : -50;
    const above = this.synthDb + (Math.random() - 0.5) * 3;
    return {
      index, band_db: floor + above, noise_floor_db: floor, db_above_floor: above, vad_prob: 0.9,
      pitch_hz: 380 + Math.random() * 30, pitch_conf: 0.8, spectral_centroid_hz: 1800 + Math.random() * 200,
      spectral_tilt_db_per_khz: -8, spectral_flatness: 0.15, playback_active: this.playbackActive, synthetic: true,
    };
  }
}
