// Speaker service for audio playback
import { eventBus, Events } from './event-bus';

interface AudioTrack {
  name: string;
  url: string;
  loop?: boolean;
  volume?: number;
}

interface SpeakerCommand {
  action: 'play' | 'stop' | 'pause' | 'resume';
  track?: string;
  volume?: number;
}

class SpeakerService {
  private tracks: Map<string, AudioTrack> = new Map();
  private audioElements: Map<string, HTMLAudioElement> = new Map();
  private currentTrack: string | null = null;
  private volume = 0.8; // Default volume

  constructor() {
    // Define default tracks
    this.registerTrack({
      name: 'fight',
      url: '/audio/fight-music.mp3',
      loop: true,
      volume: 0.9,
    });

    this.registerTrack({
      name: 'victory',
      url: '/audio/victory-music.mp3',
      loop: false,
      volume: 1.0,
    });

    // Listen for speaker commands from event bus
    eventBus.on(Events.SPEAKER_COMMAND, (command: SpeakerCommand) => {
      void this.handleCommand(command);
    });
  }

  registerTrack(track: AudioTrack): void {
    this.tracks.set(track.name, track);
    console.log('Registered audio track:', track.name);
  }

  private async handleCommand(command: SpeakerCommand): Promise<void> {
    switch (command.action) {
      case 'play':
        if (command.track) {
          await this.play(command.track, command.volume);
        }
        break;

      case 'stop':
        this.stop();
        break;

      case 'pause':
        this.pause();
        break;

      case 'resume':
        this.resume();
        break;
    }
  }

  async play(trackName: string, volume?: number): Promise<void> {
    const track = this.tracks.get(trackName);
    if (!track) {
      console.warn('Audio track not found:', trackName);
      return;
    }

    try {
      // Stop current track if playing
      this.stop();

      // Get or create audio element
      let audio = this.audioElements.get(trackName);
      if (!audio) {
        audio = new Audio(track.url);
        audio.loop = track.loop || false;
        this.audioElements.set(trackName, audio);

        // Handle errors
        audio.addEventListener('error', (e) => {
          console.error('Audio playback error for %s:', trackName, e);
        });

        // Handle track end
        audio.addEventListener('ended', () => {
          if (this.currentTrack === trackName) {
            this.currentTrack = null;
          }
        });
      }

      // Set volume
      audio.volume = volume ?? track.volume ?? this.volume;

      // Play
      await audio.play();
      this.currentTrack = trackName;

      console.log('Playing audio track:', trackName);
    } catch (error) {
      console.error('Failed to play audio track %s:', trackName, error);
    }
  }

  stop(): void {
    if (this.currentTrack) {
      const audio = this.audioElements.get(this.currentTrack);
      if (audio) {
        audio.pause();
        audio.currentTime = 0;
      }
      this.currentTrack = null;
      console.log('Audio stopped');
    }
  }

  pause(): void {
    if (this.currentTrack) {
      const audio = this.audioElements.get(this.currentTrack);
      if (audio) {
        audio.pause();
        console.log('Audio paused');
      }
    }
  }

  resume(): void {
    if (this.currentTrack) {
      const audio = this.audioElements.get(this.currentTrack);
      if (audio) {
        audio.play().catch((error) => {
          console.error('Failed to resume audio:', error);
        });
        console.log('Audio resumed');
      }
    }
  }

  setVolume(volume: number): void {
    this.volume = Math.max(0, Math.min(1, volume));

    // Update current track volume
    if (this.currentTrack) {
      const audio = this.audioElements.get(this.currentTrack);
      if (audio) {
        audio.volume = this.volume;
      }
    }

    console.log('Volume set to %f', this.volume);
  }

  getVolume(): number {
    return this.volume;
  }

  isPlaying(): boolean {
    if (!this.currentTrack) {
      return false;
    }

    const audio = this.audioElements.get(this.currentTrack);
    return audio ? !audio.paused : false;
  }

  getCurrentTrack(): string | null {
    return this.currentTrack;
  }

  // Preload audio files for smoother playback
  async preloadAll(): Promise<void> {
    const promises: Promise<void>[] = [];

    this.tracks.forEach((track, name) => {
      const promise = new Promise<void>((resolve) => {
        const audio = new Audio(track.url);
        audio.preload = 'auto';
        audio.addEventListener('canplaythrough', () => {
          this.audioElements.set(name, audio);
          console.log('Preloaded audio track:', name);
          resolve();
        });
        audio.addEventListener('error', () => {
          console.warn('Failed to preload audio track:', name);
          resolve(); // Continue even if one fails
        });
      });
      promises.push(promise);
    });

    await Promise.all(promises);
    console.log('All audio tracks preloaded');
  }
}

// Singleton instance
export const speakerService = new SpeakerService();
