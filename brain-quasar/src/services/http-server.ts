import type { Express, Request, Response } from 'express';
import express from 'express';
import cors from 'cors';
import { useStateMachineStore } from 'src/stores/state-machine';

/**
 * HTTP Server for exposing arena state to monitoring devices
 * Runs on port 8080 and binds to all network interfaces (0.0.0.0)
 */
class ArenaHTTPServer {
  private app: Express | null = null;
  private server: ReturnType<Express['listen']> | null = null;
  private port = 8080;
  private hostname = '0.0.0.0';
  private isRunning = false;

  /**
   * Initialize the HTTP server
   */
  initialize(): void {
    if (this.app) {
      console.log('[HTTP Server] Already initialized');
      return;
    }

    this.app = express();

    // Middleware
    this.app.use(cors());
    this.app.use(express.json());

    // Logging middleware
    this.app.use((req: Request, res: Response, next) => {
      console.log(`[HTTP Server] ${req.method} ${req.path}`);
      next();
    });

    // Health check endpoint
    this.app.get('/api/health', (req: Request, res: Response) => {
      res.json({ status: 'ok', timestamp: Date.now() });
    });

    // Arena state endpoint
    this.app.get('/api/arena-state', (req: Request, res: Response) => {
      const stateMachine = useStateMachineStore();
      const now = Date.now();

      const response = {
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
      };

      res.json(response);
    });

    // Arena configuration endpoint
    this.app.get('/api/arena-config', (req: Request, res: Response) => {
      const stateMachine = useStateMachineStore();
      res.json(stateMachine.config);
    });

    // Arena history endpoint (last N transitions)
    this.app.get('/api/arena-history', (req: Request, res: Response) => {
      const stateMachine = useStateMachineStore();
      const limit = Math.min(parseInt(req.query.limit as string) || 50, 200);
      const history = stateMachine.history.slice(-limit);
      res.json({ history, total: stateMachine.history.length });
    });

    // Music player state endpoint (placeholder)
    this.app.get('/api/music-state', (req: Request, res: Response) => {
      res.json({
        currentTrack: null,
        isPlaying: false,
        volume: 0.7,
        timestamp: Date.now(),
      });
    });

    // LED controllers status endpoint (placeholder)
    this.app.get('/api/led-controllers', (req: Request, res: Response) => {
      res.json({
        controllers: [],
        timestamp: Date.now(),
      });
    });

    // Server info endpoint
    this.app.get('/api/info', (req: Request, res: Response) => {
      res.json({
        appName: 'Shadow Warrior Brain',
        version: '0.0.1',
        serverVersion: '1.0.0',
        timestamp: Date.now(),
      });
    });

    // 404 handler
    this.app.use((req: Request, res: Response) => {
      res.status(404).json({
        error: 'Not found',
        path: req.path,
      });
    });

    // Error handler
    this.app.use(
      (
        err: Error,
        req: Request,
        res: Response,
        // eslint-disable-next-line @typescript-eslint/no-unused-vars
        _next: express.NextFunction
      ) => {
        console.error('[HTTP Server] Error:', err.message);
        res.status(500).json({
          error: 'Internal server error',
          message: err.message,
        });
      }
    );

    console.log('[HTTP Server] Initialized');
  }

  /**
   * Start the HTTP server
   */
  start(): Promise<void> {
    return new Promise((resolve, reject) => {
      if (!this.app) {
        this.initialize();
      }

      if (this.isRunning) {
        console.log('[HTTP Server] Already running');
        resolve();
        return;
      }

      try {
        this.server = this.app!.listen(this.port, this.hostname, () => {
          this.isRunning = true;
          console.log(
            `[HTTP Server] Started on ${this.hostname}:${this.port}`
          );
          resolve();
        });

        this.server.on('error', (err: Error) => {
          console.error('[HTTP Server] Server error:', err);
          reject(err);
        });
      } catch (err) {
        console.error('[HTTP Server] Failed to start:', err);
        reject(err instanceof Error ? err : new Error(String(err)));
      }
    });
  }

  /**
   * Stop the HTTP server
   */
  stop(): Promise<void> {
    return new Promise((resolve) => {
      if (!this.isRunning || !this.server) {
        console.log('[HTTP Server] Not running');
        resolve();
        return;
      }

      try {
        this.server.close(() => {
          this.isRunning = false;
          console.log('[HTTP Server] Stopped');
          resolve();
        });

        // Force close after 5 seconds
        setTimeout(() => {
          console.warn('[HTTP Server] Force closing...');
          this.server?.closeAllConnections?.();
          resolve();
        }, 5000);
      } catch (err) {
        console.error('[HTTP Server] Failed to stop:', err);
        resolve();
      }
    });
  }

  /**
   * Check if server is running
   */
  getStatus(): { isRunning: boolean; url: string | null } {
    return {
      isRunning: this.isRunning,
      url: this.isRunning ? `http://0.0.0.0:${this.port}` : null,
    };
  }

  /**
   * Get the port the server is running on
   */
  getPort(): number {
    return this.port;
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
export const arenaHTTPServer = new ArenaHTTPServer();
