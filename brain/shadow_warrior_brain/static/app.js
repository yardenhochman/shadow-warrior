/**
 * Shadow Warrior Brain Controller PWA
 * SSE-based real-time state updates
 */

class BrainController {
    constructor() {
        this.eventSource = null;
        this.connectionStatus = document.getElementById('connection-status');
        this.lastUpdate = document.getElementById('last-update');
        this.reconnectAttempts = 0;
        this.maxReconnectAttempts = 5;
        this.reconnectDelay = 1000; // Start with 1 second

        // SSE Events Ring Buffer (size 100)
        this.sseEventsBuffer = [];
        this.sseBufferSize = 100;
        this.sseEventsContainer = document.getElementById('sse-events-container');
        this.sseEventsContent = document.getElementById('sse-events-content');

        // Event Detail Window elements
        this.sseDetailWindow = document.getElementById('sse-detail-window');
        this.sseDetailOverlay = document.getElementById('sse-detail-overlay');
        this.sseDetailContent = document.getElementById('sse-detail-json');
        this.sseDetailTimestamp = document.getElementById('sse-detail-timestamp');

        // Statistics Window elements
        this.statsWindow = document.getElementById('stats-window');
        this.statsContent = document.getElementById('stats-content');

        // State API Window elements
        this.stateWindow = document.getElementById('state-window');
        this.stateContent = document.getElementById('state-content');

        this.initializeSSE();
        this.initializeSSEEventsWindow();
        this.initializeSSEDetailWindow();
        this.initializeStatisticsWindow();
        this.initializeStateWindow();
    }

    initializeSSE() {
        this.updateConnectionStatus('connecting', 'Connecting...');

        try {
            this.eventSource = new EventSource('/api/events');

            this.eventSource.onopen = () => {
                console.log('SSE connection opened');
                this.updateConnectionStatus('connected', 'Connected');
                this.reconnectAttempts = 0;
                this.reconnectDelay = 1000;
            };

            this.eventSource.onmessage = (event) => {
                try {
                    const data = JSON.parse(event.data);

                    if (data.error) {
                        console.error('SSE error:', data.error);
                        this.updateConnectionStatus('disconnected', `Error: ${data.error}`);
                        return;
                    }

                    // Add event to ring buffer for debugging
                    this.addEventToBuffer(event.data);

                    this.updateUI(data);
                    this.updateLastUpdateTime();

                } catch (error) {
                    console.error('Error parsing SSE data:', error);
                    // Add raw event data to buffer even if parsing fails
                    this.addEventToBuffer(event.data, true);
                }
            };

            this.eventSource.onerror = (event) => {
                console.error('SSE error:', event);
                this.updateConnectionStatus('disconnected', 'Connection Lost');

                if (this.eventSource.readyState === EventSource.CLOSED) {
                    this.scheduleReconnect();
                }
            };

        } catch (error) {
            console.error('Failed to initialize SSE:', error);
            this.updateConnectionStatus('disconnected', 'Connection Failed');
            this.scheduleReconnect();
        }
    }

    scheduleReconnect() {
        if (this.reconnectAttempts >= this.maxReconnectAttempts) {
            this.updateConnectionStatus('disconnected', 'Connection Failed (Max attempts reached)');
            return;
        }

        this.reconnectAttempts++;
        const delay = this.reconnectDelay * Math.pow(2, this.reconnectAttempts - 1); // Exponential backoff

        console.log(`Reconnecting in ${delay}ms (attempt ${this.reconnectAttempts}/${this.maxReconnectAttempts})`);

        setTimeout(() => {
            if (this.eventSource) {
                this.eventSource.close();
            }
            this.initializeSSE();
        }, delay);
    }

    updateConnectionStatus(status, message) {
        this.connectionStatus.className = `connection-status connection-${status}`;
        this.connectionStatus.textContent = message;
    }

    updateLastUpdateTime() {
        const now = new Date();
        this.lastUpdate.textContent = now.toLocaleTimeString();
    }

    updateUI(data) {
        // Update system status
        if (data.system) {
            this.updateElement('system-status', data.system.status || 'Unknown');
            this.updateElement('system-version', data.system.version || '-');
            this.updateIndicator('system-indicator', data.system.status === 'running' ? 'connected' : 'unknown');
        }

        // Calculate and display uptime from startup_timestamp
        if (data.startup_timestamp) {
            const startupTime = new Date(data.startup_timestamp);
            const now = new Date();
            const uptimeMs = now - startupTime;
            const uptimeSeconds = Math.floor(uptimeMs / 1000);

            const hours = Math.floor(uptimeSeconds / 3600);
            const minutes = Math.floor((uptimeSeconds % 3600) / 60);
            const seconds = uptimeSeconds % 60;

            let uptimeStr;
            if (hours > 0) {
                uptimeStr = `${hours}:${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}`;
            } else {
                uptimeStr = `${minutes}:${seconds.toString().padStart(2, '0')}`;
            }

            this.updateElement('system-uptime', uptimeStr);
        }

        // Update punching bag status
        if (data.punching_bag) {
            const bag = data.punching_bag;
            const isConnected = bag.connected || false;

            this.updateElement('bag-status', isConnected ? 'Connected' : 'Disconnected');
            this.updateIndicator('bag-indicator', isConnected ? 'connected' : 'disconnected');

            this.updateElement('bag-device', bag.device_name || (isConnected ? 'Unknown Device' : '-'));
            this.updateElement('bag-rssi', bag.rssi ? `${bag.rssi} dBm` : '-');

            // Update acceleration data if available
            if (bag.latest_acceleration) {
                this.updateAccelerationDisplay(bag.latest_acceleration);
            }
        }

        // Update audio status
        if (data.audio) {
            const audio = data.audio;
            const isConnected = audio.connected || false;

            this.updateElement('audio-status', isConnected ? 'Active' : 'Inactive');
            this.updateIndicator('audio-indicator', isConnected ? 'connected' : 'disconnected');

            this.updateElement('audio-device', audio.device_name || (isConnected ? 'Unknown Device' : '-'));

            if (audio.current_level && audio.current_level.level_db !== undefined) {
                this.updateElement('audio-level', `${audio.current_level.level_db.toFixed(1)} dB`);
            } else {
                this.updateElement('audio-level', '-');
            }
        }

        // Update session status
        if (data.session) {
            const session = data.session;

            this.updateElement('session-state', session.current_state || 'Unknown');

            let indicatorStatus = 'unknown';
            if (session.current_state) {
                indicatorStatus = session.current_state === 'active' ? 'connected' : 'unknown';
            }
            this.updateIndicator('session-indicator', indicatorStatus);

            // Calculate duration if transition_timestamp is available
            if (session.transition_timestamp) {
                const transitionTime = new Date(session.transition_timestamp);
                const now = new Date();
                const durationMs = now - transitionTime;
                const durationSeconds = Math.floor(durationMs / 1000);
                this.updateElement('session-duration', this.formatDuration(durationSeconds));
            } else {
                this.updateElement('session-duration', '-');
            }

            // Update punch count if available
            if (session.punch_count !== undefined) {
                this.updateElement('session-punches', session.punch_count.toString());
            }
        }

        if (data.session_data) {
            const sessionData = data.session_data;
            this.updateElement('session-duration', this.formatDuration(sessionData.duration) || '-');
            this.updateElement('session-punches', sessionData.punch_count || '0');
        }

        // Update LED status
        if (data.leds) {
            const leds = data.leds;
            this.updateElement('led-controllers', leds.connected_controllers || '0');
            this.updateElement('led-status', leds.status || 'Unknown');
            this.updateIndicator('led-indicator', leds.status === 'active' ? 'connected' : 'unknown');
        }
    }

    updateAccelerationDisplay(accelerationData) {
        const display = document.getElementById('acceleration-display');

        if (accelerationData && (accelerationData.x !== undefined || accelerationData.y !== undefined || accelerationData.z !== undefined)) {
            display.style.display = 'block';

            const x = accelerationData.x || 0;
            const y = accelerationData.y || 0;
            const z = accelerationData.z || 0;

            this.updateAccelerationAxis('x', x);
            this.updateAccelerationAxis('y', y);
            this.updateAccelerationAxis('z', z);
        } else {
            display.style.display = 'none';
        }
    }

    updateAccelerationAxis(axis, value) {
        const valueElement = document.getElementById(`accel-${axis}-value`);
        const fillElement = document.getElementById(`accel-${axis}-fill`);

        if (valueElement && fillElement) {
            valueElement.textContent = value.toFixed(2);

            // Normalize value for display (assuming max ±20 m/s²)
            const normalizedValue = Math.abs(value) / 20.0;
            const percentage = Math.min(normalizedValue * 100, 100);

            fillElement.style.width = `${percentage}%`;

            // Color based on intensity
            if (normalizedValue > 0.8) {
                fillElement.style.backgroundColor = '#f44336'; // Red for high acceleration
            } else if (normalizedValue > 0.5) {
                fillElement.style.backgroundColor = '#ff9800'; // Orange for medium acceleration
            } else if (normalizedValue > 0.2) {
                fillElement.style.backgroundColor = '#4caf50'; // Green for low acceleration
            } else {
                fillElement.style.backgroundColor = '#2196f3'; // Blue for very low acceleration
            }
        }
    }

    updateElement(id, value) {
        const element = document.getElementById(id);
        if (element) {
            element.textContent = value;
        }
    }

    updateIndicator(id, status) {
        const indicator = document.getElementById(id);
        if (indicator) {
            indicator.className = `status-indicator status-${status}`;
        }
    }

    initializeSSEEventsWindow() {
        const toggleBtn = document.getElementById('sse-events-toggle');
        const closeBtn = document.getElementById('sse-close-btn');
        const clearBtn = document.getElementById('sse-clear-btn');

        // Toggle SSE events window
        toggleBtn.addEventListener('click', (e) => {
            e.preventDefault();
            const isVisible = this.sseEventsContainer.style.display === 'flex';
            this.sseEventsContainer.style.display = isVisible ? 'none' : 'flex';

            if (!isVisible) {
                // Clear initial placeholder and refresh display
                this.refreshSSEEventsDisplay();
            }
        });

        // Close SSE events window
        closeBtn.addEventListener('click', () => {
            this.sseEventsContainer.style.display = 'none';
        });

        // Clear SSE events buffer
        clearBtn.addEventListener('click', () => {
            this.sseEventsBuffer = [];
            this.refreshSSEEventsDisplay();
        });
    }

    addEventToBuffer(eventData, isError = false) {
        const event = {
            timestamp: new Date(),
            data: eventData,
            isError: isError
        };

        // Add to ring buffer (maintain max size)
        this.sseEventsBuffer.push(event);
        if (this.sseEventsBuffer.length > this.sseBufferSize) {
            this.sseEventsBuffer.shift(); // Remove oldest event
        }

        // Update display if window is visible
        if (this.sseEventsContainer.style.display === 'flex') {
            this.refreshSSEEventsDisplay();
        }
    }

    refreshSSEEventsDisplay() {
        if (!this.sseEventsContent) return;

        if (this.sseEventsBuffer.length === 0) {
            this.sseEventsContent.innerHTML = `
                <div class="sse-event">
                    <div class="sse-event-collapsed">
                        <div class="sse-event-time">--:--:--</div>
                        <div class="sse-event-preview">No events yet. Events will appear here as they are received.</div>
                    </div>
                </div>
            `;
            return;
        }

        // Don't auto-scroll if user is scrolling or has scrolled up
        const wasAtTop = this.sseEventsContent.scrollTop === 0;

        // Display events in reverse chronological order (newest first)
        const eventsHtml = this.sseEventsBuffer
            .slice()
            .reverse()
            .map((event, index) => {
                const timeStr = event.timestamp.toLocaleTimeString().slice(0, 8); // HH:MM:SS format

                // Create preview text from event data
                let preview = event.data;
                if (!event.isError) {
                    try {
                        const parsed = JSON.parse(event.data);
                        // Create a more readable preview
                        if (parsed.system && parsed.system.status) {
                            preview = `System: ${parsed.system.status}`;
                        } else if (parsed.punching_bag && parsed.punching_bag.latest_acceleration) {
                            const acc = parsed.punching_bag.latest_acceleration;
                            preview = `Acceleration: x=${acc.x?.toFixed(2)}, y=${acc.y?.toFixed(2)}, z=${acc.z?.toFixed(2)}`;
                        } else if (parsed.audio && parsed.audio.current_level) {
                            preview = `Audio: ${parsed.audio.current_level.level_db?.toFixed(1)} dB`;
                        } else {
                            preview = `Event: ${Object.keys(parsed).join(', ')}`;
                        }
                    } catch (e) {
                        preview = event.data.substring(0, 50) + (event.data.length > 50 ? '...' : '');
                    }
                }

                const errorStyle = event.isError ? ' style="border-left-color: #f44336;"' : '';
                const eventIndex = this.sseEventsBuffer.length - 1 - index; // Original index in buffer

                return `
                    <div class="sse-event" data-event-index="${eventIndex}"${errorStyle}>
                        <div class="sse-event-collapsed">
                            <div class="sse-event-time">${timeStr}</div>
                            <div class="sse-event-preview">${this.escapeHtml(preview)}</div>
                        </div>
                        <div class="sse-event-actions">
                            <button class="sse-action-btn" data-action="copy" data-event-index="${eventIndex}" title="Copy to clipboard">📋</button>
                        </div>
                    </div>
                `;
            })
            .join('');

        this.sseEventsContent.innerHTML = eventsHtml;

        // Only auto-scroll to top if user was at the top
        if (wasAtTop) {
            this.sseEventsContent.scrollTop = 0;
        }

        // Add event listeners for the new events
        this.addEventListeners();
    }

    addEventListeners() {
        // Add click listeners to events for expansion
        const events = this.sseEventsContent.querySelectorAll('.sse-event');
        events.forEach(eventElement => {
            const eventIndex = parseInt(eventElement.dataset.eventIndex);

            // Click on event to expand
            eventElement.addEventListener('click', (e) => {
                // Don't expand if clicking on action buttons
                if (e.target.classList.contains('sse-action-btn')) {
                    return;
                }
                this.showEventDetail(eventIndex);
            });
        });

        // Add click listeners to copy buttons
        const copyButtons = this.sseEventsContent.querySelectorAll('.sse-action-btn[data-action="copy"]');
        copyButtons.forEach(button => {
            button.addEventListener('click', (e) => {
                e.stopPropagation();
                const eventIndex = parseInt(button.dataset.eventIndex);
                this.copyEventToClipboard(eventIndex);
            });
        });
    }

    initializeSSEDetailWindow() {
        const closeBtn = document.getElementById('sse-detail-close-btn');
        const copyBtn = document.getElementById('sse-detail-copy-btn');

        // Close detail window
        closeBtn.addEventListener('click', () => {
            this.hideEventDetail();
        });

        // Close on overlay click
        this.sseDetailOverlay.addEventListener('click', () => {
            this.hideEventDetail();
        });

        // Copy detail content
        copyBtn.addEventListener('click', () => {
            this.copyDetailToClipboard();
        });

        // Close on escape key
        document.addEventListener('keydown', (e) => {
            if (e.key === 'Escape' && this.sseDetailWindow.style.display === 'flex') {
                this.hideEventDetail();
            }
        });
    }

    showEventDetail(eventIndex) {
        const event = this.sseEventsBuffer[eventIndex];
        if (!event) return;

        // Format timestamp with milliseconds
        const timeStr = event.timestamp.toLocaleTimeString() + '.' +
                       event.timestamp.getMilliseconds().toString().padStart(3, '0');

        this.sseDetailTimestamp.textContent = timeStr;

        // Pretty format JSON if possible
        let displayData = event.data;
        if (!event.isError) {
            try {
                const parsed = JSON.parse(event.data);
                displayData = JSON.stringify(parsed, null, 2);
            } catch (e) {
                // Keep original data if not valid JSON
            }
        }

        this.sseDetailContent.textContent = displayData;

        // Store current event data for copying
        this.currentDetailEvent = event;

        // Show the detail window
        this.sseDetailOverlay.style.display = 'block';
        this.sseDetailWindow.style.display = 'flex';
    }

    hideEventDetail() {
        this.sseDetailOverlay.style.display = 'none';
        this.sseDetailWindow.style.display = 'none';
        this.currentDetailEvent = null;
    }

    async copyEventToClipboard(eventIndex) {
        const event = this.sseEventsBuffer[eventIndex];
        if (!event) return;

        try {
            await navigator.clipboard.writeText(event.data);
            this.showCopyFeedback('📋 Copied!');
        } catch (err) {
            console.error('Failed to copy to clipboard:', err);
            this.showCopyFeedback('❌ Copy failed');
        }
    }

    async copyDetailToClipboard() {
        if (!this.currentDetailEvent) return;

        try {
            await navigator.clipboard.writeText(this.currentDetailEvent.data);
            this.showCopyFeedback('📋 Copied!');
        } catch (err) {
            console.error('Failed to copy to clipboard:', err);
            this.showCopyFeedback('❌ Copy failed');
        }
    }

    showCopyFeedback(message) {
        // Simple feedback - could be enhanced with a toast notification
        const originalText = document.getElementById('sse-detail-copy-btn').textContent;
        const copyBtn = document.getElementById('sse-detail-copy-btn');

        if (copyBtn) {
            copyBtn.textContent = message;
            setTimeout(() => {
                copyBtn.textContent = originalText;
            }, 1000);
        }
    }

    initializeStatisticsWindow() {
        const toggleBtn = document.getElementById('stats-toggle');
        const closeBtn = document.getElementById('stats-close-btn');
        const refreshBtn = document.getElementById('stats-refresh-btn');

        // Toggle statistics window
        toggleBtn.addEventListener('click', (e) => {
            e.preventDefault();
            const isVisible = this.statsWindow.style.display === 'flex';
            this.statsWindow.style.display = isVisible ? 'none' : 'flex';

            if (!isVisible) {
                // Load statistics when opening
                this.loadStatistics();
            }
        });

        // Close statistics window
        closeBtn.addEventListener('click', () => {
            this.statsWindow.style.display = 'none';
        });

        // Refresh statistics
        refreshBtn.addEventListener('click', () => {
            this.loadStatistics();
        });
    }

    async loadStatistics() {
        try {
            // Show loading state
            this.statsContent.innerHTML = `
                <div class="stats-section">
                    <div class="stats-section-title">🔄 Loading statistics...</div>
                </div>
            `;

            const response = await fetch('/api/statistics');
            if (!response.ok) {
                throw new Error(`HTTP ${response.status}: ${response.statusText}`);
            }

            const stats = await response.json();
            this.displayStatistics(stats);

        } catch (error) {
            console.error('Failed to load statistics:', error);
            this.statsContent.innerHTML = `
                <div class="stats-section" style="border-left-color: #f44336;">
                    <div class="stats-section-title">❌ Error Loading Statistics</div>
                    <div class="stats-item">
                        <span class="stats-label">Error:</span>
                        <span class="stats-value">${error.message}</span>
                    </div>
                </div>
            `;
        }
    }

    displayStatistics(stats) {
        let html = '';

        // Timestamp and System Info
        const timestamp = new Date(stats.timestamp);
        const timeStr = timestamp.toLocaleTimeString() + '.' +
                       timestamp.getMilliseconds().toString().padStart(3, '0');

        // System Information Section
        html += '<div class="stats-section">';
        html += '<div class="stats-section-title">🖥️ System Information <span class="stats-timestamp">' + timeStr + '</span></div>';

        if (stats.uptime) {
            html += '<div class="stats-item"><span class="stats-label">Uptime:</span><span class="stats-value">' + stats.uptime + '</span></div>';
        }

        if (stats.startup_timestamp) {
            const startupTime = new Date(stats.startup_timestamp);
            html += '<div class="stats-item"><span class="stats-label">Started:</span><span class="stats-value">' + startupTime.toLocaleString() + '</span></div>';
        }

        html += '</div>';

        // Sensor Data Section
        if (stats.sensor_data) {
            html += '<div class="stats-section">';
            html += '<div class="stats-section-title">🔬 Sensor Data</div>';

            // Punching Bag Sensor Data
            if (stats.sensor_data.punching_bag) {
                const pb = stats.sensor_data.punching_bag;
                html += '<div class="stats-item"><span class="stats-label">Buffer Size:</span><span class="stats-value">' + pb.data_buffer_size + '</span></div>';
                html += '<div class="stats-item"><span class="stats-label">Recent Readings:</span><span class="stats-value">' + pb.recent_readings_count + '</span></div>';

                if (pb.latest_acceleration) {
                    const acc = pb.latest_acceleration;
                    html += '<div class="stats-nested">';
                    html += '<div class="stats-item"><span class="stats-label">Latest X:</span><span class="stats-value">' + (acc.x?.toFixed(3) || 'N/A') + ' m/s²</span></div>';
                    html += '<div class="stats-item"><span class="stats-label">Latest Y:</span><span class="stats-value">' + (acc.y?.toFixed(3) || 'N/A') + ' m/s²</span></div>';
                    html += '<div class="stats-item"><span class="stats-label">Latest Z:</span><span class="stats-value">' + (acc.z?.toFixed(3) || 'N/A') + ' m/s²</span></div>';
                    html += '<div class="stats-item"><span class="stats-label">Magnitude:</span><span class="stats-value">' + (acc.acceleration?.toFixed(3) || 'N/A') + ' m/s²</span></div>';
                    html += '</div>';
                }
            }

            // Audio Sensor Data
            if (stats.sensor_data.audio) {
                const audio = stats.sensor_data.audio;
                html += '<div class="stats-item"><span class="stats-label">Audio Device:</span><span class="stats-value">' + (audio.device_name || 'Unknown') + '</span></div>';
                if (audio.current_level) {
                    html += '<div class="stats-item"><span class="stats-label">Audio Level:</span><span class="stats-value">' + (audio.current_level.level_db?.toFixed(1) || 'N/A') + ' dB</span></div>';
                }
            }

            html += '</div>';
        }

        // Session Statistics Section
        if (stats.session_statistics && stats.session_statistics.current_session) {
            const session = stats.session_statistics.current_session;
            html += '<div class="stats-section">';
            html += '<div class="stats-section-title">🎯 Session Statistics</div>';
            html += '<div class="stats-item"><span class="stats-label">Punch Count:</span><span class="stats-value">' + session.punch_count + '</span></div>';
            html += '<div class="stats-item"><span class="stats-label">Session Active:</span><span class="stats-value">' + (session.session_active ? 'Yes' : 'No') + '</span></div>';
            html += '<div class="stats-item"><span class="stats-label">Current State:</span><span class="stats-value">' + session.current_state + '</span></div>';

            // Show session duration if available
            if (session.session_duration !== undefined) {
                html += '<div class="stats-item"><span class="stats-label">Session Duration:</span><span class="stats-value">' + this.formatDuration(session.session_duration) + '</span></div>';
            }

            // Calculate state duration from transition timestamp if available
            if (session.transition_timestamp) {
                const transitionTime = new Date(session.transition_timestamp);
                const now = new Date();
                const durationMs = now - transitionTime;
                const durationSeconds = Math.floor(durationMs / 1000);
                html += '<div class="stats-item"><span class="stats-label">State Duration:</span><span class="stats-value">' + this.formatDuration(durationSeconds) + '</span></div>';
                html += '<div class="stats-item"><span class="stats-label">State Since:</span><span class="stats-value">' + transitionTime.toLocaleTimeString() + '</span></div>';
            }

            if (session.state_machine_status) {
                html += '<div class="stats-nested">';
                const sm = session.state_machine_status;
                html += '<div class="stats-item"><span class="stats-label">SM State:</span><span class="stats-value">' + sm.current_state + '</span></div>';
                html += '<div class="stats-item"><span class="stats-label">SM Transitions:</span><span class="stats-value">' + sm.transition_count + '</span></div>';
                html += '</div>';
            }

            html += '</div>';
        }

        // Device Statistics Section
        if (stats.device_statistics && stats.device_statistics.punching_bag) {
            const device = stats.device_statistics.punching_bag;
            html += '<div class="stats-section">';
            html += '<div class="stats-section-title">📱 Device Statistics</div>';
            html += '<div class="stats-item"><span class="stats-label">Device Name:</span><span class="stats-value">' + (device.device_name || 'Unknown') + '</span></div>';
            html += '<div class="stats-item"><span class="stats-label">Address:</span><span class="stats-value">' + (device.device_address || 'N/A') + '</span></div>';

            if (device.connection_time) {
                const connTime = new Date(device.connection_time);
                html += '<div class="stats-item"><span class="stats-label">Connected:</span><span class="stats-value">' + connTime.toLocaleTimeString() + '</span></div>';
            }

            if (device.parameters) {
                const params = device.parameters;
                html += '<div class="stats-nested">';
                html += '<div class="stats-item"><span class="stats-label">Alpha:</span><span class="stats-value">' + params.alpha + '</span></div>';
                html += '<div class="stats-item"><span class="stats-label">Threshold:</span><span class="stats-value">' + params.threshold + '</span></div>';
                html += '<div class="stats-item"><span class="stats-label">Fight Mode:</span><span class="stats-value">' + (params.fight_mode ? 'On' : 'Off') + '</span></div>';
                html += '</div>';
            }

            html += '</div>';
        }

        // Historical Data Section
        if (stats.session_statistics && stats.session_statistics.historical_data) {
            const historical = stats.session_statistics.historical_data;
            html += '<div class="stats-section">';
            html += '<div class="stats-section-title">📈 Historical Data</div>';
            html += '<div class="stats-item"><span class="stats-label">Duration:</span><span class="stats-value">' + this.formatDuration(historical.duration) + '</span></div>';
            html += '<div class="stats-item"><span class="stats-label">Total Punches:</span><span class="stats-value">' + historical.punch_count + '</span></div>';
            html += '</div>';
        }

        this.statsContent.innerHTML = html;
    }

    initializeStateWindow() {
        const toggleBtn = document.getElementById('state-toggle');
        const closeBtn = document.getElementById('state-close-btn');
        const refreshBtn = document.getElementById('state-refresh-btn');

        // Toggle state window
        toggleBtn.addEventListener('click', (e) => {
            e.preventDefault();
            const isVisible = this.stateWindow.style.display === 'flex';
            this.stateWindow.style.display = isVisible ? 'none' : 'flex';

            if (!isVisible) {
                // Load state when opening
                this.loadState();
            }
        });

        // Close state window
        closeBtn.addEventListener('click', () => {
            this.stateWindow.style.display = 'none';
        });

        // Refresh state
        refreshBtn.addEventListener('click', () => {
            this.loadState();
        });
    }

    async loadState() {
        try {
            // Show loading state
            this.stateContent.innerHTML = `
                <div class="state-section">
                    <div class="state-section-title">🔄 Loading state...</div>
                </div>
            `;

            const response = await fetch('/api/state');
            if (!response.ok) {
                throw new Error(`HTTP ${response.status}: ${response.statusText}`);
            }

            const state = await response.json();
            this.displayState(state);

        } catch (error) {
            console.error('Failed to load state:', error);
            this.stateContent.innerHTML = `
                <div class="state-section" style="border-left-color: #f44336;">
                    <div class="state-section-title">❌ Error Loading State</div>
                    <div class="state-item">
                        <span class="state-label">Error:</span>
                        <span class="state-value">${error.message}</span>
                    </div>
                </div>
            `;
        }
    }

    displayState(state) {
        let html = '';

        // Timestamp and System Info
        const timestamp = new Date(state.timestamp);
        const timeStr = timestamp.toLocaleTimeString() + '.' +
                       timestamp.getMilliseconds().toString().padStart(3, '0');

        // System Information Section
        if (state.system || state.startup_timestamp) {
            html += '<div class="state-section">';
            html += '<div class="state-section-title">🖥️ System <span class="state-timestamp">' + timeStr + '</span></div>';

            if (state.system) {
                html += '<div class="state-item"><span class="state-label">Status:</span><span class="state-value">' + (state.system.status || 'Unknown') + '</span></div>';
                html += '<div class="state-item"><span class="state-label">Version:</span><span class="state-value">' + (state.system.version || 'Unknown') + '</span></div>';
            }

            if (state.startup_timestamp) {
                const startupTime = new Date(state.startup_timestamp);
                const now = new Date();
                const uptimeMs = now - startupTime;
                const uptimeSeconds = Math.floor(uptimeMs / 1000);

                html += '<div class="state-item"><span class="state-label">Started:</span><span class="state-value">' + startupTime.toLocaleString() + '</span></div>';
                html += '<div class="state-item"><span class="state-label">Uptime:</span><span class="state-value">' + this.formatDuration(uptimeSeconds) + '</span></div>';
            }

            html += '</div>';
        }

        // Punching Bag Section
        if (state.punching_bag) {
            const bag = state.punching_bag;
            html += '<div class="state-section">';
            html += '<div class="state-section-title">🥊 Punching Bag</div>';
            html += '<div class="state-item"><span class="state-label">Connected:</span><span class="state-value">' + (bag.connected ? 'Yes' : 'No') + '</span></div>';

            if (bag.device_name) {
                html += '<div class="state-item"><span class="state-label">Device:</span><span class="state-value">' + bag.device_name + '</span></div>';
            }

            if (bag.rssi) {
                html += '<div class="state-item"><span class="state-label">RSSI:</span><span class="state-value">' + bag.rssi + ' dBm</span></div>';
            }

            if (bag.connection_time) {
                const connTime = new Date(bag.connection_time);
                html += '<div class="state-item"><span class="state-label">Connected At:</span><span class="state-value">' + connTime.toLocaleTimeString() + '</span></div>';
            }

            if (bag.latest_acceleration) {
                const acc = bag.latest_acceleration;
                html += '<div class="state-nested">';
                html += '<div class="state-item"><span class="state-label">X:</span><span class="state-value">' + (acc.x?.toFixed(3) || 'N/A') + ' m/s²</span></div>';
                html += '<div class="state-item"><span class="state-label">Y:</span><span class="state-value">' + (acc.y?.toFixed(3) || 'N/A') + ' m/s²</span></div>';
                html += '<div class="state-item"><span class="state-label">Z:</span><span class="state-value">' + (acc.z?.toFixed(3) || 'N/A') + ' m/s²</span></div>';
                html += '</div>';
            }

            html += '</div>';
        }

        // Audio Section
        if (state.audio) {
            const audio = state.audio;
            html += '<div class="state-section">';
            html += '<div class="state-section-title">🎤 Audio</div>';
            html += '<div class="state-item"><span class="state-label">Connected:</span><span class="state-value">' + (audio.connected ? 'Yes' : 'No') + '</span></div>';

            if (audio.device_name) {
                html += '<div class="state-item"><span class="state-label">Device:</span><span class="state-value">' + audio.device_name + '</span></div>';
            }

            if (audio.current_level && audio.current_level.level_db !== undefined) {
                html += '<div class="state-item"><span class="state-label">Level:</span><span class="state-value">' + audio.current_level.level_db.toFixed(1) + ' dB</span></div>';
            }

            html += '</div>';
        }

        // Session Section
        if (state.session) {
            const session = state.session;
            html += '<div class="state-section">';
            html += '<div class="state-section-title">🎯 Session</div>';
            html += '<div class="state-item"><span class="state-label">State:</span><span class="state-value">' + (session.current_state || 'Unknown') + '</span></div>';

            if (session.transition_timestamp) {
                const transitionTime = new Date(session.transition_timestamp);
                const now = new Date();
                const durationMs = now - transitionTime;
                const durationSeconds = Math.floor(durationMs / 1000);
                html += '<div class="state-item"><span class="state-label">Duration:</span><span class="state-value">' + this.formatDuration(durationSeconds) + '</span></div>';
                html += '<div class="state-item"><span class="state-label">Since:</span><span class="state-value">' + transitionTime.toLocaleTimeString() + '</span></div>';
            }

            if (session.punch_count !== undefined) {
                html += '<div class="state-item"><span class="state-label">Punches:</span><span class="state-value">' + session.punch_count + '</span></div>';
            }

            html += '</div>';
        }

        // LEDs Section
        if (state.leds) {
            const leds = state.leds;
            html += '<div class="state-section">';
            html += '<div class="state-section-title">💡 LEDs</div>';
            html += '<div class="state-item"><span class="state-label">Controllers:</span><span class="state-value">' + (leds.connected_controllers || 0) + '</span></div>';
            html += '<div class="state-item"><span class="state-label">Status:</span><span class="state-value">' + (leds.status || 'Unknown') + '</span></div>';
            html += '</div>';
        }

        this.stateContent.innerHTML = html;
    }

    formatDuration(seconds) {
        if (!seconds || seconds < 0) return '0s';

        const hours = Math.floor(seconds / 3600);
        const minutes = Math.floor((seconds % 3600) / 60);
        const secs = Math.floor(seconds % 60);

        if (hours > 0) {
            return `${hours}h ${minutes}m ${secs}s`;
        } else if (minutes > 0) {
            return `${minutes}m ${secs}s`;
        } else {
            return `${secs}s`;
        }
    }

    escapeHtml(text) {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }

    cleanup() {
        if (this.eventSource) {
            this.eventSource.close();
        }
    }
}

// Initialize the app when the DOM is loaded
document.addEventListener('DOMContentLoaded', () => {
    window.brainController = new BrainController();
});

// Clean up when the page is unloaded
window.addEventListener('beforeunload', () => {
    if (window.brainController) {
        window.brainController.cleanup();
    }
});

// Service Worker registration for PWA capabilities
if ('serviceWorker' in navigator) {
    window.addEventListener('load', () => {
        navigator.serviceWorker.register('/static/sw.js')
            .then((registration) => {
                console.log('SW registered: ', registration);
            })
            .catch((registrationError) => {
                console.log('SW registration failed: ', registrationError);
            });
    });
}