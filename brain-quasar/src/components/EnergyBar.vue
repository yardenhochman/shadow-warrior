<template>
  <div class="energy-bar-container">
    <div class="energy-bar-label">{{ label }}</div>
    <div class="energy-bar-wrapper">
      <div
        class="energy-bar-fill"
        :style="fillStyle"
        :class="intensityClass"
      >
        <div class="energy-bar-shine"></div>
      </div>
      <div class="energy-bar-percentage">{{ percentageDisplay }}</div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

interface Props {
  label: string;
  value: number; // 0-1
  color?: string;
  showPercentage?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  color: 'primary',
  showPercentage: true,
});

const fillStyle = computed(() => {
  const percentage = Math.min(100, Math.max(0, props.value * 100));
  return {
    width: `${percentage}%`,
  };
});

const percentageDisplay = computed(() => {
  if (!props.showPercentage) return '';
  return `${Math.round(props.value * 100)}%`;
});

const intensityClass = computed(() => {
  const value = props.value;
  if (value < 0.3) return 'intensity-low';
  if (value < 0.6) return 'intensity-medium';
  return 'intensity-high';
});
</script>

<style scoped>
.energy-bar-container {
  width: 100%;
  margin-bottom: 12px;
}

.energy-bar-label {
  font-size: 12px;
  font-weight: 600;
  margin-bottom: 6px;
  color: rgba(0, 0, 0, 0.7);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.energy-bar-wrapper {
  position: relative;
  height: 32px;
  background: linear-gradient(to right, #e0e0e0, #f5f5f5);
  border-radius: 16px;
  overflow: hidden;
  box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.1);
}

.energy-bar-fill {
  position: absolute;
  left: 0;
  top: 0;
  height: 100%;
  border-radius: 16px;
  transition: width 0.15s ease-out;
  overflow: hidden;
}

.energy-bar-fill.intensity-low {
  background: linear-gradient(90deg, #4caf50, #66bb6a);
}

.energy-bar-fill.intensity-medium {
  background: linear-gradient(90deg, #ff9800, #ffa726);
}

.energy-bar-fill.intensity-high {
  background: linear-gradient(90deg, #f44336, #ef5350);
  box-shadow: 0 0 10px rgba(244, 67, 54, 0.5);
}

.energy-bar-shine {
  position: absolute;
  top: 0;
  left: -100%;
  width: 100%;
  height: 100%;
  background: linear-gradient(
    90deg,
    transparent,
    rgba(255, 255, 255, 0.3),
    transparent
  );
  animation: shine 2s infinite;
}

@keyframes shine {
  0% {
    left: -100%;
  }
  100% {
    left: 100%;
  }
}

.energy-bar-percentage {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  font-size: 14px;
  font-weight: bold;
  color: rgba(0, 0, 0, 0.8);
  text-shadow: 0 1px 2px rgba(255, 255, 255, 0.8);
  pointer-events: none;
  z-index: 10;
}

/* Dark mode support */
body.body--dark .energy-bar-label {
  color: rgba(255, 255, 255, 0.7);
}

body.body--dark .energy-bar-wrapper {
  background: linear-gradient(to right, #2a2a2a, #333333);
}

body.body--dark .energy-bar-percentage {
  color: rgba(255, 255, 255, 0.9);
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.8);
}
</style>
