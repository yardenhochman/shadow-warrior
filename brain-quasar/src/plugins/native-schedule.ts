import { registerPlugin } from '@capacitor/core';

export interface NativeSchedulePlugin {
  /**
   * Schedule a daily alarm using Android AlarmManager
   */
  scheduleDailyAlarm(options: {
    id: string;
    hour: number;
    minute: number;
    type: 'activate' | 'suspend';
  }): Promise<void>;

  /**
   * Cancel all scheduled alarms
   */
  cancelAllAlarms(): Promise<void>;

  /**
   * Check if current time is within active window
   */
  isWithinActiveHours(options: {
    startHour: number;
    startMinute: number;
    endHour: number;
    endMinute: number;
  }): Promise<{ withinHours: boolean }>;
}

const NativeSchedule = registerPlugin<NativeSchedulePlugin>('NativeSchedule', {
  web: () => import('./native-schedule.web').then(m => new m.NativeScheduleWeb()),
});

export default NativeSchedule;
