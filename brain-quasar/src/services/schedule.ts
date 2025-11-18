// Schedule service for managing daily active/inactive periods
import { eventBus, Events } from './event-bus';
import NativeSchedule from 'src/plugins/native-schedule';
import { useStateMachineStore } from 'src/stores/state-machine';

class ScheduleService {
  private initialized = false;

  async initialize() {
    if (this.initialized) return;

    // Listen for native schedule alarms
    this.setupAlarmListener();

    // Check if schedule is enabled and set up alarms
    const stateMachine = useStateMachineStore();
    if (stateMachine.config.schedule.enabled) {
      await this.scheduleAlarms();
    }

    this.initialized = true;
  }

  private setupAlarmListener() {
    // Listen for the custom intent from ScheduleAlarmReceiver
    window.addEventListener('scheduleAlarm', (event) => {
      const customEvent = event as CustomEvent<{ alarmType: string }>;
      const alarmType = customEvent.detail?.alarmType;
      console.log('Schedule alarm received:', alarmType);

      if (alarmType === 'schedule_suspend') {
        eventBus.emit(Events.SCHEDULE_SUSPEND_REQUESTED);
      } else if (alarmType === 'schedule_activate') {
        eventBus.emit(Events.SCHEDULE_ACTIVATE_AVAILABLE);
      }
    });
  }

  async updateSchedule(scheduleConfig: { enabled: boolean; dailyActiveStart: string; dailyActiveEnd: string }) {
    console.log('Updating schedule:', scheduleConfig);

    if (scheduleConfig.enabled) {
      await this.scheduleAlarms();
    } else {
      await this.cancelAlarms();
    }
  }

  private async scheduleAlarms() {
    try {
      const stateMachine = useStateMachineStore();
      const schedule = stateMachine.config.schedule;

      console.log('Scheduling daily alarms:', schedule);

      // Parse times
      const [suspendHour, suspendMinute] = this.parseTime(schedule.dailyActiveEnd);
      const [activateHour, activateMinute] = this.parseTime(schedule.dailyActiveStart);

      // Schedule suspend alarm
      await NativeSchedule.scheduleDailyAlarm({
        id: 'schedule_suspend',
        hour: suspendHour,
        minute: suspendMinute,
        type: 'suspend',
      });

      // Schedule activate alarm
      await NativeSchedule.scheduleDailyAlarm({
        id: 'schedule_activate',
        hour: activateHour,
        minute: activateMinute,
        type: 'activate',
      });
    } catch (error) {
      console.error('Failed to schedule alarms:', error);
    }
  }

  private parseTime(timeString: string): [number, number] {
    const parts = timeString.split(':');
    const hour = parseInt(parts[0] || '0', 10);
    const minute = parseInt(parts[1] || '0', 10);
    return [hour, minute];
  }

  private async cancelAlarms() {
    try {
      console.log('Canceling all schedule alarms');
      await NativeSchedule.cancelAllAlarms();
      console.log('Schedule alarms cancelled successfully');
    } catch (error) {
      // Ignore UNIMPLEMENTED errors - this just means no alarms were scheduled
      const capacitorError = error as { code?: string };
      if (capacitorError?.code === 'UNIMPLEMENTED') {
        console.log('No native alarm implementation available (expected on web platform)');
      } else {
        console.error('Failed to cancel alarms:', error);
      }
    }
  }
}

export const scheduleService = new ScheduleService();
