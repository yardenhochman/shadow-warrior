import type { Server as WebSocketServer } from 'ws';
import { useStateMachineStore } from 'src/stores/state-machine';

/**
 * WebSocket Server for real-time arena state monitoring
 * WARNING: This is a stub implementation.
 * WebSocket server cannot properly run in Capacitor WebView context.
 *
 * The `ws` package requires Node.js runtime which is not available
 * in the Capacitor WebView (browser) environment.
 *
 * This class provides the interface but the server binding will fail.
 *
 * For working real-time monitoring, use instead:
 * 1. External HTTP API server (separate backend)
 * 2. Native Capacitor HTTP server plugin
 * 3. Server-Sent Events (SSE) - if backend available
 */
class ArenaWebSocketServer {
  private wss: WebSocketServer | null = null;
  private port = 8081;
  private isRunning = false;
  private clients: Set<globalThis.WebSocket> = new Set();
  private stateUpdateInterval: number | null = null;

  /**
   * Initialize the WebSocket server
   */
  initialize(): void {
    console.log('[WebSocket Server] WARNING: Stub implementation');
    console.log(
      '[WebSocket Server] WebSocket server cannot run in Capacitor WebView'
    );
    console.log('[WebSocket Server] The ws package requires Node.js runtime');
    console.log('');
    console.log('[WebSocket Server] For working real-time arena monitoring:');
    console.log('  Option 1: Use external HTTP API server');
    console.log('  Option 2: Install native Capacitor HTTP server plugin');
    console.log('  Option 3: Implement server-side API on separate backend');
  }

  /**
   * Start the WebSocket server
   */
  start(): Promise<void> {
    return Promise.resolve().then(() => {
      console.log(
        '[WebSocket Server] Server start attempted but ws package unavailable in WebView'
      );
      // Server won't actually start in WebView context
      this.isRunning = false;
    });
  }

  /**
   * Stop the WebSocket server
   */
  stop(): Promise<void> {
    return Promise.resolve().then(() => {
      this.isRunning = false;
      console.log('[WebSocket Server] Server stop called');
    });
  }

  /**
   * Handle incoming WebSocket messages (stub)
   */
  private handleMessage(ws: globalThis.WebSocket, message: string): void {
    try {
      const data = JSON.parse(message);

      switch (data.type) {
        case 'get-state':
          this.sendArenaState(ws);
          break;
        case 'get-config':
          this.sendArenaConfig(ws);
          break;
        case 'get-history':
          this.sendArenaHistory(ws, data.limit || 50);
          break;
        default:
          ws.send(JSON.stringify({ error: 'Unknown message type' }));
      }
    } catch (err) {
      console.error('[WebSocket Server] Message handling error:', err);
      ws.send(JSON.stringify({ error: 'Invalid message format' }));
    }
  }

  /**
   * Send current arena state to a client (stub)
   */
  private sendArenaState(ws?: globalThis.WebSocket): void {
    const stateMachine = useStateMachineStore();
    const now = Date.now();

    const response = {
      type: 'arena-state',
      data: {
        currentState: stateMachine.currentState,
        previousState: stateMachine.previousState,
        metrics: {
          shoutAmplitude: stateMachine.metrics.shoutAmplitude,
          punchForce: stateMachine.metrics.punchForce,
          warmingPower: stateMachine.metrics.warmingPower,
          fightPower: stateMachine.metrics.fightPower,
        },
        progress: {
          warmingProgress: stateMachine.warmingProgress,
          fightProgress: stateMachine.fightProgress,
        },
        timers: {
          fightStartTime: stateMachine.fightStartTime,
          fightElapsed: stateMachine.fightStartTime
            ? now - stateMachine.fightStartTime
            : null,
          warmingStartTime: stateMachine.warmingStartTime,
          warmingElapsed: stateMachine.warmingStartTime
            ? now - stateMachine.warmingStartTime
            : null,
          cooldownStartTime: stateMachine.cooldownStartTime,
          cooldownTimeRemaining: stateMachine.cooldownTimeRemaining,
        },
        lastActivityTime: stateMachine.lastActivityTime,
        timestamp: now,
      },
    };

    if (ws) {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify(response));
      }
    } else {
      this.clients.forEach((client) => {
        if (client.readyState === WebSocket.OPEN) {
          client.send(JSON.stringify(response));
        }
      });
    }
  }

  /**
   * Send arena configuration to a client (stub)
   */
  private sendArenaConfig(ws: globalThis.WebSocket): void {
    const stateMachine = useStateMachineStore();
    const response = {
      type: 'arena-config',
      data: stateMachine.config,
    };

    if (ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(response));
    }
  }

  /**
   * Send arena history to a client (stub)
   */
  private sendArenaHistory(ws: globalThis.WebSocket, limit: number): void {
    const stateMachine = useStateMachineStore();
    const limitedHistory = stateMachine.history.slice(
      -Math.min(limit, 200)
    );

    const response = {
      type: 'arena-history',
      data: {
        history: limitedHistory,
        total: stateMachine.history.length,
      },
    };

    if (ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(response));
    }
  }

  /**
   * Start broadcasting arena state to all connected clients (stub)
   */
  private startStateUpdates(): void {
    if (this.stateUpdateInterval !== null) {
      return;
    }

    this.stateUpdateInterval = window.setInterval(() => {
      this.sendArenaState();
    }, 500);

    console.log('[WebSocket Server] State updates started (500ms interval)');
  }

  /**
   * Stop broadcasting arena state (stub)
   */
  private stopStateUpdates(): void {
    if (this.stateUpdateInterval !== null) {
      clearInterval(this.stateUpdateInterval);
      this.stateUpdateInterval = null;
      console.log('[WebSocket Server] State updates stopped');
    }
  }

  /**
   * Get server status
   */
  getStatus(): { isRunning: boolean; port: number; clientCount: number } {
    return {
      isRunning: this.isRunning,
      port: this.port,
      clientCount: this.clients.size,
    };
  }

  /**
   * Set the port (before starting)
   */
  setPort(port: number): void {
    if (this.isRunning) {
      throw new Error('Cannot change port while server is running');
    }
    this.port = port;
  }
}

// Singleton instance
export const arenaWebSocketServer = new ArenaWebSocketServer();
