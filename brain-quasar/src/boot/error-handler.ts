// Global error handler for Shadow Warrior
import { boot } from 'quasar/wrappers';
import { Notify } from 'quasar';

export default boot(({ app }) => {
  // Global error handler for Vue components
  app.config.errorHandler = (error, instance, info) => {
    console.error('Vue error:', error);
    console.error('Component:', instance);
    console.error('Info:', info);

    Notify.create({
      type: 'negative',
      message: 'An error occurred',
      caption: error instanceof Error ? error.message : String(error),
      position: 'top',
      timeout: 5000,
    });
  };

  // Global warning handler
  app.config.warnHandler = (msg, instance, trace) => {
    console.warn('Vue warning:', msg);
    console.warn('Component:', instance);
    console.warn('Trace:', trace);
  };

  // Global unhandled promise rejection handler
  window.addEventListener('unhandledrejection', (event) => {
    console.error('Unhandled promise rejection:', event.reason);

    Notify.create({
      type: 'negative',
      message: 'Unexpected error',
      caption:
        event.reason instanceof Error ? event.reason.message : String(event.reason),
      position: 'top',
      timeout: 5000,
    });

    event.preventDefault();
  });

  // Global error handler for uncaught exceptions
  window.addEventListener('error', (event) => {
    console.error('Uncaught error:', event.error);

    Notify.create({
      type: 'negative',
      message: 'System error',
      caption: event.message,
      position: 'top',
      timeout: 5000,
    });
  });

  console.log('Error handler initialized');
});
