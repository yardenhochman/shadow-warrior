<template>
  <div class="console-wrapper">
    <transition name="expand">
      <div v-if="isOpen" class="console-panel">
        <div class="console-header">
          <div class="console-title">Console</div>
          <q-btn
            flat
            dense
            round
            icon="unfold_less"
            size="sm"
            @click="toggleConsole"
            class="text-white"
          />
        </div>
        <div
          ref="consoleContent"
          class="console-content"
        >
          <div
            v-for="(message, index) in messages"
            :key="index"
            :class="['console-message', `console-${message.level}`]"
          >
            <span class="console-timestamp">{{ formatTime(message.timestamp) }}</span>
            <span class="console-level">[{{ message.level.toUpperCase() }}]</span>
            <span class="console-text">{{ message.text }}</span>
          </div>
        </div>
      </div>
    </transition>

    <div
      v-if="!isOpen && messages.length > 0"
      class="console-badge"
      @click="toggleConsole"
    >
      <q-icon name="unfold_more" />
      <span class="badge-count">{{ messages.length }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';

interface ConsoleMessage {
  level: 'log' | 'warn' | 'error';
  text: string;
  timestamp: number;
}

const MAX_MESSAGES = 100;
const messages = ref<ConsoleMessage[]>([]);
const consoleContent = ref<HTMLElement>();
const isOpen = ref(false);

// Store original console methods
const originalConsoleWarn = console.warn;
const originalConsoleError = console.error;
const originalConsoleLog = console.log;

function addMessage(level: 'log' | 'warn' | 'error', text: string) {
  messages.value.push({
    level,
    text,
    timestamp: Date.now(),
  });

  // Keep only last MAX_MESSAGES
  if (messages.value.length > MAX_MESSAGES) {
    messages.value.shift();
  }

  // Auto-scroll to bottom
  setTimeout(() => {
    if (consoleContent.value) {
      consoleContent.value.scrollTop = consoleContent.value.scrollHeight;
    }
  }, 0);
}

function formatTime(timestamp: number): string {
  const date = new Date(timestamp);
  return date.toLocaleTimeString('en-US', {
    hour12: false,
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

function toggleConsole() {
  isOpen.value = !isOpen.value;
}

// Override console methods
function overrideConsole() {
  const consoleObj = console as unknown as Record<string, unknown>;
  
  consoleObj.warn = function (...args: unknown[]) {
    const text = args.map(arg => {
      if (typeof arg === 'object' && arg !== null) {
        return JSON.stringify(arg);
      }
      return String(arg);
    }).join(' ');
    addMessage('warn', text);
    originalConsoleWarn.apply(console, args);
  };

  consoleObj.error = function (...args: unknown[]) {
    const text = args.map(arg => {
      if (typeof arg === 'object' && arg !== null) {
        return JSON.stringify(arg);
      }
      return String(arg);
    }).join(' ');
    addMessage('error', text);
    originalConsoleError.apply(console, args);
  };

  consoleObj.log = function (...args: unknown[]) {
    // Only log warnings and errors to keep console clean
    // Uncomment to also log regular logs:
    // const text = args.map(arg => {
    //   if (typeof arg === 'object' && arg !== null) {
    //     return JSON.stringify(arg);
    //   }
    //   return String(arg);
    // }).join(' ');
    // addMessage('log', text);
    originalConsoleLog.apply(console, args);
  };
}

function restoreConsole() {
  const consoleObj = console as unknown as Record<string, unknown>;
  consoleObj.warn = originalConsoleWarn;
  consoleObj.error = originalConsoleError;
  consoleObj.log = originalConsoleLog;
}

onMounted(() => {
  overrideConsole();
});

onUnmounted(() => {
  restoreConsole();
});
</script>

<style scoped lang="scss">
.console-wrapper {
  position: fixed;
  bottom: env(safe-area-inset-bottom);
  left: 0;
  right: 0;
  z-index: 1000;
  pointer-events: none;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  padding: 12px;
  gap: 8px;

  > * {
    pointer-events: auto;
  }
}

.console-panel {
  position: absolute;
  bottom: 0;
  right: 0;
  width: 90vw;
  max-width: 600px;
  height: 400px;
  background-color: #1a1a1a;
  border: 2px solid #00ff00;
  border-radius: 2px;
  display: flex;
  flex-direction: column;
  font-family: 'Courier New', monospace;
  font-size: 11px;
  box-shadow: 0 0 20px rgba(0, 255, 0, 0.3), inset 0 0 20px rgba(0, 255, 0, 0.05);
}

.console-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 12px;
  background-color: rgba(0, 255, 0, 0.1);
  border-bottom: 1px solid #00ff00;
  flex-shrink: 0;
}

.console-title {
  color: #00ff00;
  font-weight: bold;
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 2px;
}

.console-content {
  flex: 1;
  overflow-y: auto;
  padding: 8px 12px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  background-color: #0a0a0a;
}

.console-message {
  display: flex;
  gap: 8px;
  line-height: 1.4;
  word-break: break-word;

  &.console-log {
    color: #00ff00;
  }

  &.console-warn {
    color: #ffff00;
  }

  &.console-error {
    color: #ff0000;
  }
}

.console-timestamp {
  color: #00aa00;
  flex-shrink: 0;
  min-width: 45px;
}

.console-level {
  color: #00aa00;
  flex-shrink: 0;
  min-width: 55px;
}

.console-text {
  flex: 1;
  word-break: break-all;
  color: #00ff00;
}

// Scrollbar styling
.console-content {
  &::-webkit-scrollbar {
    width: 8px;
  }

  &::-webkit-scrollbar-track {
    background: #0a0a0a;
  }

  &::-webkit-scrollbar-thumb {
    background: #00ff00;
    border-radius: 1px;
    opacity: 0.3;

    &:hover {
      opacity: 0.6;
    }
  }
}

// Floating badge when closed
.console-badge {
  position: fixed;
  bottom: calc(env(safe-area-inset-bottom) + 12px);
  right: 12px;
  width: 50px;
  height: 50px;
  background-color: #1a1a1a;
  border: 2px solid #00ff00;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  flex-direction: column;
  gap: 2px;
  box-shadow: 0 0 15px rgba(0, 255, 0, 0.5);
  transition: all 0.2s ease;
  z-index: 1001;

  &:hover {
    box-shadow: 0 0 25px rgba(0, 255, 0, 0.8);
    transform: scale(1.05);
  }

  &:active {
    transform: scale(0.95);
  }

  .q-icon {
    color: #00ff00;
    font-size: 24px;
  }

  .badge-count {
    color: #ff0000;
    font-size: 10px;
    font-weight: bold;
    min-width: 16px;
    text-align: center;
  }
}

// Expand animation
.expand-enter-active,
.expand-leave-active {
  transition: all 0.3s ease;
}

.expand-enter-from {
  opacity: 0;
  transform: scale(0.8) translateY(20px);
}

.expand-leave-to {
  opacity: 0;
  transform: scale(0.8) translateY(20px);
}
</style>
