import type { NativeSchedulePlugin } from './native-schedule';

export class NativeScheduleWeb implements NativeSchedulePlugin {
  scheduleDailyAlarm(): Promise<void> {
    console.warn('NativeSchedule: Web platform - using JavaScript timers');
    return Promise.resolve();
  }

  cancelAllAlarms(): Promise<void> {
    // No-op on web
    return Promise.resolve();
  }

  isWithinActiveHours(options: {
    startHour: number;
    startMinute: number;
    endHour: number;
    endMinute: number;
  }): Promise<{ withinHours: boolean }> {
    const now = new Date();
    const currentMinutes = now.getHours() * 60 + now.getMinutes();
    const startMinutes = options.startHour * 60 + options.startMinute;
    const endMinutes = options.endHour * 60 + options.endMinute;

    // Handle midnight crossing
    const withinHours = endMinutes > startMinutes
      ? currentMinutes >= startMinutes && currentMinutes < endMinutes
      : currentMinutes >= startMinutes || currentMinutes < endMinutes;

    return Promise.resolve({ withinHours });
  }
}
