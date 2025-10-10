// Shadow Warrior - Known Tracks Configuration
// This file contains a list of available tracks for the Shadow Warrior training app

export const KNOWN_TRACKS = [
  {
    id: 'war-is-coming',
    name: 'War is Coming',
    artist: 'danmed',
    genre: 'Electronic/Trance',
    bpm: 135,
    duration: '3:45', // Approximate duration
    description: 'High-energy electronic track perfect for intense training sessions',
    url: 'https://firebasestorage.googleapis.com/v0/b/leds-shadow.firebasestorage.app/o/tracks%2Flooperman-t-2156125-0257358-danmed-war-is-coming.mp3?alt=media&token=18552274-2d01-4ca0-b954-6066e9245a20',
    source: 'Looperman',
    loopermanId: '257358',
    uploadDate: '2025',
    fileSize: '~8MB',
    tags: ['electronic', 'trance', 'high-energy', 'training', 'intense'],
    energy: 'high', // low, medium, high
    mood: 'aggressive', // calm, energetic, aggressive, euphoric
    recommendedFor: ['intense training', 'cardio', 'martial arts', 'high-energy workouts']
  }
  // Add more tracks here as they become available
];

// Track categories for easy filtering
export const TRACK_CATEGORIES = {
  HIGH_ENERGY: 'high-energy',
  MEDIUM_ENERGY: 'medium-energy', 
  LOW_ENERGY: 'low-energy',
  TRAINING: 'training',
  WARMUP: 'warmup',
  COOLDOWN: 'cooldown'
};

// Helper functions for track management
export const getTrackById = (id) => {
  return KNOWN_TRACKS.find(track => track.id === id);
};

export const getTracksByGenre = (genre) => {
  return KNOWN_TRACKS.filter(track => 
    track.genre.toLowerCase().includes(genre.toLowerCase())
  );
};

export const getTracksByEnergy = (energy) => {
  return KNOWN_TRACKS.filter(track => track.energy === energy);
};

export const getTracksByMood = (mood) => {
  return KNOWN_TRACKS.filter(track => track.mood === mood);
};

export const getRandomTrack = () => {
  const randomIndex = Math.floor(Math.random() * KNOWN_TRACKS.length);
  return KNOWN_TRACKS[randomIndex];
};

// Track validation
export const validateTrack = (track) => {
  const requiredFields = ['id', 'name', 'url'];
  return requiredFields.every(field => track.hasOwnProperty(field) && track[field]);
};

// Add a new track to the list
export const addTrack = (trackData) => {
  if (validateTrack(trackData)) {
    KNOWN_TRACKS.push(trackData);
    return true;
  }
  return false;
};

// Get track statistics
export const getTrackStats = () => {
  return {
    totalTracks: KNOWN_TRACKS.length,
    genres: [...new Set(KNOWN_TRACKS.map(t => t.genre))],
    energyLevels: [...new Set(KNOWN_TRACKS.map(t => t.energy))],
    moods: [...new Set(KNOWN_TRACKS.map(t => t.mood))],
    totalDuration: KNOWN_TRACKS.reduce((total, track) => {
      // Simple duration parsing (assumes MM:SS format)
      const [minutes, seconds] = track.duration.split(':').map(Number);
      return total + (minutes * 60 + seconds);
    }, 0)
  };
};
