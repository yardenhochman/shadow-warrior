<template>
  <q-card>
    <q-card-section>
      <div class="row items-center q-mb-md">
        <div class="text-h6">Music Player</div>
        <q-space />
        <q-chip v-if="playlist.length > 0" size="sm" color="primary">
          {{ playlist.length }} tracks
        </q-chip>
      </div>

      <!-- Current Track Display -->
      <div v-if="currentTrack" class="q-mb-md">
        <div class="text-subtitle2">Now Playing</div>
        <div class="text-body1">{{ currentTrack.name }}</div>

        <!-- Progress Bar -->
        <q-linear-progress
          v-if="currentTrack.duration"
          :value="currentTime / currentTrack.duration"
          color="primary"
          class="q-mt-sm"
        />

        <!-- Playback Controls -->
        <div class="row q-mt-sm items-center q-gutter-sm">
          <q-btn icon="skip_previous" flat round @click="playPrevious" :disable="playlist.length === 0" />
          <q-btn
            :icon="isPlaying ? 'pause' : 'play_arrow'"
            color="primary"
            round
            @click="togglePlay"
            :disable="playlist.length === 0"
          />
          <q-btn icon="skip_next" flat round @click="playNext" :disable="playlist.length === 0" />
          <q-btn icon="stop" flat round @click="stop" :disable="!isPlaying" />

          <!-- Shuffle & Repeat -->
          <q-btn
            icon="shuffle"
            flat
            round
            :color="shuffle ? 'primary' : 'grey'"
            @click="toggleShuffle"
          >
            <q-tooltip>Shuffle</q-tooltip>
          </q-btn>
          <q-btn
            icon="repeat"
            flat
            round
            :color="repeat ? 'primary' : 'grey'"
            @click="toggleRepeat"
          >
            <q-tooltip>Repeat</q-tooltip>
          </q-btn>

          <!-- Volume Control -->
          <q-space />
          <q-icon name="volume_up" />
          <q-slider
            v-model="volume"
            :min="0"
            :max="100"
            @update:model-value="onVolumeChange"
            style="width: 100px"
            dense
          />
        </div>
      </div>

      <!-- Playlist -->
      <q-list v-if="playlist.length > 0" dense bordered class="rounded-borders" style="max-height: 300px; overflow-y: auto">
        <q-item
          v-for="track in playlist"
          :key="track.id"
          clickable
          @click="playTrack(track.id)"
          :active="currentTrack?.id === track.id"
        >
          <q-item-section avatar>
            <q-icon
              :name="currentTrack?.id === track.id && isPlaying ? 'volume_up' : 'music_note'"
              :color="currentTrack?.id === track.id ? 'primary' : 'grey'"
            />
          </q-item-section>
          <q-item-section>
            <q-item-label>{{ track.name }}</q-item-label>
            <q-item-label caption v-if="track.duration">
              {{ formatDuration(track.duration) }}
            </q-item-label>
          </q-item-section>
          <q-item-section side>
            <q-btn icon="delete" flat round size="sm" @click.stop="removeTrack(track.id)" />
          </q-item-section>
        </q-item>
      </q-list>

      <!-- Empty State -->
      <div v-else class="text-center q-pa-md">
        <q-icon name="library_music" size="3em" color="grey" />
        <div class="text-subtitle1 q-mt-sm text-grey">No tracks added</div>
        <div class="text-caption text-grey">Add music files to start playing</div>
      </div>

      <!-- Add Music Controls -->
      <q-separator class="q-my-md" />
      <div class="row q-gutter-sm">
        <q-btn label="Add Music Files" icon="add" color="primary" @click="pickMusicFiles" />
        <q-btn
          v-if="playlist.length > 0"
          label="Clear All"
          icon="clear_all"
          color="negative"
          outline
          @click="clearPlaylist"
        />
      </div>
    </q-card-section>
  </q-card>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { musicPlayerService, type MusicTrack } from 'src/services/music-player';
import { eventBus, Events } from 'src/services/event-bus';

const playlist = ref<MusicTrack[]>([]);
const currentTrack = ref<MusicTrack | null>(null);
const isPlaying = ref(false);
const volume = ref(70); // 0-100
const currentTime = ref(0);
const shuffle = ref(false);
const repeat = ref(false);

let progressInterval: number | null = null;

async function pickMusicFiles() {
  try {
    await musicPlayerService.pickFiles();
  } catch (error) {
    console.error('Failed to pick music files:', error);
  }
}

function playTrack(trackId: string) {
  void musicPlayerService.play(trackId);
}

function togglePlay() {
  if (isPlaying.value) {
    void musicPlayerService.pause();
  } else {
    void musicPlayerService.play();
  }
}

function playNext() {
  musicPlayerService.playNext();
}

function playPrevious() {
  musicPlayerService.playPrevious();
}

function stop() {
  void musicPlayerService.stop();
}

function toggleShuffle() {
  musicPlayerService.toggleShuffle();
  shuffle.value = musicPlayerService.getShuffle();
}

function toggleRepeat() {
  musicPlayerService.toggleRepeat();
  repeat.value = musicPlayerService.getRepeat();
}

function removeTrack(trackId: string) {
  musicPlayerService.removeTrack(trackId);
}

function clearPlaylist() {
  if (confirm('Remove all tracks from playlist?')) {
    musicPlayerService.clearPlaylist();
  }
}

function onVolumeChange(value: number | null) {
  if (value !== null) {
    void musicPlayerService.setVolume(value / 100);
  }
}

function formatDuration(seconds: number): string {
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs.toString().padStart(2, '0')}`;
}

async function updateProgress() {
  if (isPlaying.value) {
    currentTime.value = await musicPlayerService.getCurrentTime();
  }
}

// Event handlers
const handlePlaying = () => {
  isPlaying.value = true;
  currentTrack.value = musicPlayerService.getCurrentTrack();
};

const handlePaused = () => {
  isPlaying.value = false;
};

const handleStopped = () => {
  isPlaying.value = false;
  currentTime.value = 0;
};

const handlePlaylistUpdated = () => {
  playlist.value = musicPlayerService.getPlaylist();
};

onMounted(() => {
  // Load initial state
  playlist.value = musicPlayerService.getPlaylist();
  currentTrack.value = musicPlayerService.getCurrentTrack();
  isPlaying.value = musicPlayerService.getIsPlaying();
  volume.value = musicPlayerService.getVolume() * 100;
  shuffle.value = musicPlayerService.getShuffle();
  repeat.value = musicPlayerService.getRepeat();

  // Register event listeners
  eventBus.on(Events.MUSIC_PLAYING, handlePlaying);
  eventBus.on(Events.MUSIC_PAUSED, handlePaused);
  eventBus.on(Events.MUSIC_STOPPED, handleStopped);
  eventBus.on(Events.MUSIC_PLAYLIST_UPDATED, handlePlaylistUpdated);

  // Update progress bar
  progressInterval = window.setInterval(() => {
    void updateProgress();
  }, 100);
});

onUnmounted(() => {
  // Cleanup event listeners
  eventBus.off(Events.MUSIC_PLAYING, handlePlaying);
  eventBus.off(Events.MUSIC_PAUSED, handlePaused);
  eventBus.off(Events.MUSIC_STOPPED, handleStopped);
  eventBus.off(Events.MUSIC_PLAYLIST_UPDATED, handlePlaylistUpdated);

  if (progressInterval !== null) {
    clearInterval(progressInterval);
  }
});
</script>
