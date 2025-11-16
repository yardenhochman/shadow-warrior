import { eventBus, Events } from './event-bus';
import { FilePicker } from '@capawesome/capacitor-file-picker';
import NativeAudioExtended from '../plugins/native-audio-extended';
import { Filesystem, Directory } from '@capacitor/filesystem';
import MusicPlayback from '../plugins/music-playback';

export interface MusicTrack {
  id: string;
  name: string;
  path: string; // File path or URI
  duration?: number | undefined;
  isPreloaded?: boolean; // Track if audio is loaded in native audio
}

const ACTIVE_TRACK_ASSET_ID = 'active-music-track';

class MusicPlayerService {
  private currentTrack: MusicTrack | null = null;
  private playlist: MusicTrack[] = [];
  private volume = 0.7;
  private isPlaying = false;
  private currentTrackIndex = 0;
  private shuffle = false;
  private repeat = false;
  private playbackMonitor: number | null = null;

  constructor() {
    // Configure native audio for background playback
    void this.configureNativeAudio();

    // Listen for track completion
    void NativeAudioExtended.addListener('complete', (data) => {
      console.log('Track completed:', data);
      if (data.assetId === ACTIVE_TRACK_ASSET_ID) {
        this.handleTrackEnded();
      }
    });

    // Load saved playlist
    this.loadPlaylist();
  }

  /**
   * Configure native audio for optimal background playback
   */
  private async configureNativeAudio(): Promise<void> {
    try {
      await NativeAudioExtended.configure({
        fade: false,
        focus: true, // Request audio focus for background playback
      });
      console.log('Native audio configured for background playback');
    } catch (error) {
      console.error('Failed to configure native audio:', error);
    }
  }

  /**
   * Handle track ended event
   */
  private handleTrackEnded(): void {
    this.stopPlaybackMonitor();
    this.isPlaying = false;

    if (this.repeat) {
      void this.play(this.currentTrack?.id);
    } else if (this.currentTrackIndex < this.playlist.length - 1 || this.shuffle) {
      this.playNext();
    } else {
      // Playlist ended, stop foreground service
      void MusicPlayback.stopForegroundService();
    }
  }

  /**
   * Pick audio files using native file picker
   * Copies files to app cache for native audio access
   */
  async pickFiles(): Promise<MusicTrack[]> {
    try {
      const result = await FilePicker.pickFiles({
        types: ['audio/*'],
        readData: true, // Need to read data to copy to cache
      });

      if (!result.files || result.files.length === 0) {
        return [];
      }

      const newTracks: MusicTrack[] = [];

      for (const file of result.files) {
        // Skip if already in playlist
        if (this.playlist.some(t => t.name === file.name?.replace(/\.[^/.]+$/, ''))) {
          console.log('Skipping duplicate:', file.name);
          continue;
        }

        if (!file.data || !file.name) {
          console.warn('File has no data or name:', file.name);
          continue;
        }

        try {
          // Copy file to app's cache directory where native audio can access it
          const fileName = `music_${Date.now()}_${file.name}`;
          const writeResult = await Filesystem.writeFile({
            path: `music/${fileName}`,
            data: file.data,
            directory: Directory.Cache,
            recursive: true, // Create parent directory if it doesn't exist
          });

          console.log('Copied file to cache:', writeResult.uri);

          const track: MusicTrack = {
            id: `track-${Date.now()}-${Math.random()}`,
            name: file.name.replace(/\.[^/.]+$/, ''),
            path: writeResult.uri, // Use the file:// URI from cache
            isPreloaded: false,
          };

          this.playlist.push(track);
          newTracks.push(track);

          console.log('Added track:', track.name, 'cached at:', track.path);

          // Load track metadata in background
          this.loadTrackMetadata(track);
        } catch (error) {
          console.error('Failed to copy file to cache:', file.name, error);
        }
      }

      console.log(`Added ${newTracks.length} tracks to playlist (${result.files.length} selected)`);
      this.savePlaylist();
      eventBus.emit(Events.MUSIC_PLAYLIST_UPDATED, { playlist: this.playlist });

      return newTracks;
    } catch (error) {
      console.error('Failed to pick files:', error);
      throw error;
    }
  }

  /**
   * Add tracks from File objects (for web compatibility - not recommended, use native audio)
   * Note: This is primarily for development/testing in browser
   */
  addTracks(files: FileList | File[]): MusicTrack[] {
    console.warn('addTracks() using File objects - native audio preferred for production');
    const newTracks: MusicTrack[] = [];

    for (let i = 0; i < files.length; i++) {
      const file = files[i];

      if (!file) continue;

      // Validate audio file
      if (!file.type.startsWith('audio/')) {
        console.warn(`Skipping non-audio file: ${file.name}`);
        continue;
      }

      // Skip duplicates
      const fileName = file.name;
      if (this.playlist.some(t => t.name === fileName.replace(/\.[^/.]+$/, ''))) {
        console.log('Skipping duplicate:', fileName);
        continue;
      }

      // Create temporary blob URL for path
      const blobUrl = URL.createObjectURL(file);

      const track: MusicTrack = {
        id: `track-${Date.now()}-${i}`,
        name: file.name.replace(/\.[^/.]+$/, ''),
        path: blobUrl, // Use blob URL as path for web compatibility
        isPreloaded: false,
      };

      this.playlist.push(track);
      newTracks.push(track);

      // Get duration
      this.loadTrackMetadata(track);
    }

    console.log(`Added ${newTracks.length} tracks to playlist`);
    this.savePlaylist();
    eventBus.emit(Events.MUSIC_PLAYLIST_UPDATED, { playlist: this.playlist });

    return newTracks;
  }

  /**
   * Remove a track from the playlist
   */
  removeTrack(trackId: string): void {
    const index = this.playlist.findIndex((t) => t.id === trackId);
    if (index === -1) return;

    const track = this.playlist[index];
    if (!track) return;

    // Stop if currently playing
    if (this.currentTrack?.id === trackId) {
      void this.stop();
    }

    // Unload from native audio if preloaded
    if (track.isPreloaded) {
      this.unloadTrack(track);
    }

    this.playlist.splice(index, 1);

    // Adjust current index if needed
    if (this.currentTrackIndex >= index && this.currentTrackIndex > 0) {
      this.currentTrackIndex--;
    }

    this.savePlaylist();
    eventBus.emit(Events.MUSIC_PLAYLIST_UPDATED, { playlist: this.playlist });
  }

  /**
   * Clear all tracks
   */
  clearPlaylist(): void {
    void this.stop();

    // Unload all preloaded tracks from native audio
    this.playlist.forEach((track) => {
      if (track.isPreloaded) {
        this.unloadTrack(track);
      }
    });

    this.playlist = [];
    this.currentTrackIndex = 0;
    this.savePlaylist();
    eventBus.emit(Events.MUSIC_PLAYLIST_UPDATED, { playlist: this.playlist });
  }

  /**
   * Unload a track from native audio memory and delete cache file
   */
  private unloadTrack(track: MusicTrack): void {
    if (track.isPreloaded) {
      NativeAudioExtended.unload({ assetId: track.id })
        .then(() => {
          track.isPreloaded = false;
          console.log('Unloaded track:', track.name);
        })
        .catch(error => {
          console.warn('Failed to unload track:', track.name, error);
        });
    }

    // Clean up cache file if it's in our cache directory
    if (track.path.includes('/cache/') && track.path.includes('/music/')) {
      const fileName = track.path.split('/music/')[1];
      if (fileName) {
        Filesystem.deleteFile({
          path: `music/${fileName}`,
          directory: Directory.Cache,
        })
          .then(() => console.log('Deleted cache file:', fileName))
          .catch(error => console.warn('Failed to delete cache file:', fileName, error));
      }
    }
  }

  /**
   * Play a specific track or resume current
   */
  async play(trackId?: string): Promise<void> {
    if (this.playlist.length === 0) {
      console.warn('Playlist is empty');
      return;
    }

    // If track ID specified, find and play it
    if (trackId) {
      const index = this.playlist.findIndex((t) => t.id === trackId);
      if (index !== -1) {
        this.currentTrackIndex = index;
      }
    }

    const track = this.playlist[this.currentTrackIndex];
    if (!track) return;

    try {
      // Start foreground service for background playback
      console.log('[MusicPlayer] Starting foreground service for:', track.name);
      await MusicPlayback.startForegroundService({ trackName: track.name });
      console.log('[MusicPlayer] Foreground service started');

      // Load track if different from current
      if (this.currentTrack?.id !== track.id) {
        this.currentTrack = track;
        await this.loadAndPlayTrack(track);
      } else {
        // Resume current track
        await NativeAudioExtended.resume({ assetId: ACTIVE_TRACK_ASSET_ID });
      }

      this.isPlaying = true;
      this.startPlaybackMonitor();
      eventBus.emit(Events.MUSIC_PLAYING, { track: this.currentTrack });
    } catch (error) {
      console.error('Failed to play audio:', error);
      eventBus.emit(Events.MUSIC_ERROR, { track: this.currentTrack, error });
      // Try next track on error
      this.playNext();
    }
  }

  /**
   * Pause playback
   */
  async pause(): Promise<void> {
    try {
      await NativeAudioExtended.pause({ assetId: ACTIVE_TRACK_ASSET_ID });
      this.isPlaying = false;
      this.stopPlaybackMonitor();
      eventBus.emit(Events.MUSIC_PAUSED, { track: this.currentTrack });
    } catch (error) {
      console.error('Failed to pause:', error);
    }
  }

  /**
   * Stop playback
   */
  async stop(): Promise<void> {
    try {
      await NativeAudioExtended.stop({ assetId: ACTIVE_TRACK_ASSET_ID });
      this.isPlaying = false;
      this.stopPlaybackMonitor();

      // Stop foreground service when music stops
      await MusicPlayback.stopForegroundService();

      eventBus.emit(Events.MUSIC_STOPPED, { track: this.currentTrack });
    } catch (error) {
      console.error('Failed to stop:', error);
    }
  }

  /**
   * Play next track
   */
  playNext(): void {
    if (this.playlist.length === 0) return;

    if (this.shuffle) {
      // Random track
      this.currentTrackIndex = Math.floor(Math.random() * this.playlist.length);
    } else {
      this.currentTrackIndex = (this.currentTrackIndex + 1) % this.playlist.length;
    }

    void this.play();
  }

  /**
   * Play previous track
   */
  playPrevious(): void {
    if (this.playlist.length === 0) return;

    this.currentTrackIndex = this.currentTrackIndex - 1;
    if (this.currentTrackIndex < 0) {
      this.currentTrackIndex = this.playlist.length - 1;
    }
    void this.play();
  }

  /**
   * Set volume (0-1)
   */
  async setVolume(volume: number): Promise<void> {
    this.volume = Math.max(0, Math.min(1, volume));

    try {
      // Set volume for currently loaded track
      if (this.currentTrack) {
        await NativeAudioExtended.setVolume({
          assetId: ACTIVE_TRACK_ASSET_ID,
          volume: this.volume,
        });
      }
      eventBus.emit(Events.MUSIC_VOLUME_CHANGED, { volume: this.volume });
    } catch (error) {
      console.warn('Failed to set volume:', error);
    }
  }

  /**
   * Toggle shuffle
   */
  toggleShuffle(): void {
    this.shuffle = !this.shuffle;
    console.log('Shuffle:', this.shuffle);
  }

  /**
   * Toggle repeat
   */
  toggleRepeat(): void {
    this.repeat = !this.repeat;
    console.log('Repeat:', this.repeat);
  }

  /**
   * Get states
   */
  getVolume(): number {
    return this.volume;
  }

  getIsPlaying(): boolean {
    return this.isPlaying;
  }

  getCurrentTrack(): MusicTrack | null {
    return this.currentTrack;
  }

  getPlaylist(): MusicTrack[] {
    return [...this.playlist];
  }

  getShuffle(): boolean {
    return this.shuffle;
  }

  getRepeat(): boolean {
    return this.repeat;
  }

  /**
   * Load track metadata (duration) using native audio
   */
  private loadTrackMetadata(track: MusicTrack): void {
    // Load duration using native audio in background
    const tempAssetId = `temp-${track.id}`;

    NativeAudioExtended.preload({
      assetId: tempAssetId,
      assetPath: track.path,
      volume: 0, // Silent preload
      audioChannelNum: 1,
      isUrl: track.path.startsWith('http') || track.path.startsWith('file://') || track.path.startsWith('blob:'),
    })
      .then(() => NativeAudioExtended.getDuration({ assetId: tempAssetId }))
      .then((result) => {
        track.duration = result.duration;
        eventBus.emit(Events.MUSIC_TRACK_LOADED, { track });
        console.log(`Loaded metadata for ${track.name}: ${track.duration}s`);
        // Cleanup temp asset
        return NativeAudioExtended.unload({ assetId: tempAssetId });
      })
      .catch((error) => {
        console.warn(`Failed to load metadata for ${track.name}:`, error);
        // Try to cleanup anyway
        NativeAudioExtended.unload({ assetId: tempAssetId }).catch(() => {
          // Ignore cleanup errors
        });
      });
  }

  /**
   * Get current playback time
   */
  async getCurrentTime(): Promise<number> {
    try {
      if (this.currentTrack) {
        const result = await NativeAudioExtended.getCurrentTime({ assetId: ACTIVE_TRACK_ASSET_ID });
        return result.currentTime;
      }
    } catch (error) {
      console.warn('Failed to get current time:', error);
    }
    return 0;
  }

  /**
   * Seek to time (seconds)
   * Note: Native audio doesn't support direct seeking during playback.
   * We need to stop and restart with time parameter.
   */
  async seek(time: number): Promise<void> {
    if (!this.currentTrack) return;

    try {
      const wasPlaying = this.isPlaying;
      await this.stop();

      if (wasPlaying) {
        // Replay from the specified time
        await this.loadAndPlayTrack(this.currentTrack, time);
        this.isPlaying = true;
        this.startPlaybackMonitor();
      }
    } catch (error) {
      console.error('Failed to seek:', error);
    }
  }

  /**
   * Load and play a track using native audio
   */
  private async loadAndPlayTrack(track: MusicTrack, startTime = 0): Promise<void> {
    try {
      console.log('Loading track for playback:', track.name, 'from:', track.path);

      // Unload previous track if exists
      try {
        await NativeAudioExtended.unload({ assetId: ACTIVE_TRACK_ASSET_ID });
      } catch {
        // Ignore unload errors (asset might not exist)
      }

      // Preload the new track
      await NativeAudioExtended.preload({
        assetId: ACTIVE_TRACK_ASSET_ID,
        assetPath: track.path,
        volume: this.volume,
        audioChannelNum: 1,
        isUrl: track.path.startsWith('http') || track.path.startsWith('file://') || track.path.startsWith('blob:'),
      });

      track.isPreloaded = true;

      // Play with optional start time
      await NativeAudioExtended.play({
        assetId: ACTIVE_TRACK_ASSET_ID,
        time: startTime,
      });

      // Get duration if not already set
      if (!track.duration) {
        this.loadTrackMetadata(track);
      }

      console.log('Track loaded and playing:', track.name);
    } catch (error) {
      console.error('Failed to load and play track:', error);
      throw error;
    }
  }

  /**
   * Start monitoring playback progress
   */
  private startPlaybackMonitor(): void {
    if (this.playbackMonitor) {
      clearInterval(this.playbackMonitor);
    }

    // Update progress every second
    this.playbackMonitor = window.setInterval(() => {
      if (this.isPlaying && this.currentTrack) {
        void this.getCurrentTime(); // Keep the current time updated
      }
    }, 1000);
  }

  /**
   * Stop monitoring playback progress
   */
  private stopPlaybackMonitor(): void {
    if (this.playbackMonitor) {
      clearInterval(this.playbackMonitor);
      this.playbackMonitor = null;
    }
  }

  /**
   * Save playlist to localStorage
   */
  private savePlaylist(): void {
    try {
      const playlistData = this.playlist.map((track) => ({
        id: track.id,
        name: track.name,
        path: track.path,
        duration: track.duration,
        isPreloaded: false, // Don't persist preloaded state
      }));

      localStorage.setItem('shadow-warrior-music-playlist', JSON.stringify(playlistData));
    } catch (error) {
      console.error('Failed to save playlist:', error);
    }
  }

  /**
   * Load playlist from localStorage
   */
  private loadPlaylist(): void {
    try {
      const saved = localStorage.getItem('shadow-warrior-music-playlist');
      if (!saved) return;

      interface PlaylistData {
        id: string;
        name: string;
        path: string;
        duration?: number;
        isPreloaded?: boolean;
      }

      const playlistData: PlaylistData[] = JSON.parse(saved);

      this.playlist = playlistData.map((data) => {
        const track: MusicTrack = {
          id: data.id,
          name: data.name,
          path: data.path,
          isPreloaded: false, // Always false on load
        };
        if (data.duration !== undefined) {
          track.duration = data.duration;
        }
        return track;
      });

      console.log('Loaded playlist with', this.playlist.length, 'tracks');
      eventBus.emit(Events.MUSIC_PLAYLIST_UPDATED, { playlist: this.playlist });
    } catch (error) {
      console.error('Failed to load playlist:', error);
    }
  }

  /**
   * Cleanup
   */
  destroy(): void {
    void this.stop();
    this.clearPlaylist();
    this.stopPlaybackMonitor();
  }
}

export const musicPlayerService = new MusicPlayerService();
