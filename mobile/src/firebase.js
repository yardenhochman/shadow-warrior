// Import the functions you need from the SDKs you need
import { initializeApp } from "firebase/app";
import { getAuth, signInAnonymously, onAuthStateChanged } from "firebase/auth";
import { getDatabase, ref, push, set } from "firebase/database";

// Your web app's Firebase configuration
const firebaseConfig = {
  apiKey: "AIzaSyD_C9S5AjaNSYy6mweZ2HUg7qf0agVpP2U",
  authDomain: "leds-shadow.firebaseapp.com",
  databaseURL: "https://leds-shadow-default-rtdb.europe-west1.firebasedatabase.app",
  projectId: "leds-shadow",
  storageBucket: "leds-shadow.firebasestorage.app",
  messagingSenderId: "129513863309",
  appId: "1:129513863309:web:069171b9bf558fc126cac4",
  measurementId: "G-V8CLTSLN3B"
};

// Initialize Firebase
const app = initializeApp(firebaseConfig);
const auth = getAuth(app);
const database = getDatabase(app);

class FirebaseManager {
  constructor() {
    this.user = null;
    this.userId = null;
    this.isAuthenticated = false;
    this.sessionId = null;
    this.sessionStartTime = null;
  }

  async initialize() {
    return new Promise((resolve, reject) => {
      onAuthStateChanged(auth, async (user) => {
        if (user) {
          this.user = user;
          this.userId = user.uid;
          this.isAuthenticated = true;
          this.sessionStartTime = new Date().toISOString();
          this.sessionId = Date.now().toString();
          console.log('User authenticated:', user.uid);
          console.log('Session started:', this.sessionId);
          resolve(user);
        } else {
          // Try to sign in anonymously
          try {
            const result = await signInAnonymously(auth);
            this.user = result.user;
            this.userId = result.user.uid;
            this.isAuthenticated = true;
            this.sessionStartTime = new Date().toISOString();
            this.sessionId = Date.now().toString();
            console.log('Anonymous user created:', result.user.uid);
            console.log('Session started:', this.sessionId);
            resolve(result.user);
          } catch (error) {
            console.error('Anonymous authentication failed:', error);
            reject(error);
          }
        }
      });
    });
  }

  async logError(error, context = '') {
    if (!this.isAuthenticated || !this.userId) {
      console.warn('Cannot log error: user not authenticated');
      return;
    }

    try {
      const eventTime = new Date().toISOString();
      const errorData = {
        timestamp: eventTime,
        message: error.message || String(error),
        stack: error.stack || '',
        context: context,
        userAgent: navigator.userAgent,
        url: window.location.href,
        sessionStartTime: this.sessionStartTime,
        sessionId: this.sessionId
      };

      const errorRef = ref(database, `mobile_logs/${this.userId}/${this.sessionId}/errors/${eventTime.replace(/[:.]/g, '-')}`);
      await set(errorRef, errorData);
      console.log('Error logged to Firebase:', errorData);
    } catch (logError) {
      console.error('Failed to log error to Firebase:', logError);
    }
  }

  async logEvent(eventType, data = {}) {
    if (!this.isAuthenticated || !this.userId) {
      console.warn('Cannot log event: user not authenticated');
      return;
    }

    try {
      const eventTime = new Date().toISOString();
      const eventData = {
        timestamp: eventTime,
        type: eventType,
        data: data,
        userAgent: navigator.userAgent,
        url: window.location.href,
        sessionStartTime: this.sessionStartTime,
        sessionId: this.sessionId
      };

      const eventRef = ref(database, `mobile_logs/${this.userId}/${this.sessionId}/events/${eventTime.replace(/[:.]/g, '-')}`);
      await set(eventRef, eventData);
      console.log('Event logged to Firebase:', eventData);
    } catch (logError) {
      console.error('Failed to log event to Firebase:', logError);
    }
  }

  getUserId() {
    return this.userId;
  }

  isUserAuthenticated() {
    return this.isAuthenticated;
  }
}

// Create and export a singleton instance
const firebaseManager = new FirebaseManager();
export default firebaseManager;
