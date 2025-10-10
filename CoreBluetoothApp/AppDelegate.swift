import Cocoa
import CoreBluetooth

@main
class AppDelegate: NSObject, NSApplicationDelegate {
    
    var window: NSWindow!
    var peripheralManager: CBPeripheralManager!
    var service: CBMutableService!
    var characteristic: CBMutableCharacteristic!
    
    // BLE Service and Characteristic UUIDs (matching Shadow Warrior web app)
    let serviceUUID = CBUUID(string: "12345678-1234-1234-1234-123456789ABC")
    let characteristicUUID = CBUUID(string: "87654321-4321-4321-4321-CBA987654321")
    
    // Loudness meter data
    var currentLoudnessLevel: Float = 0.0
    var maxLoudnessLevel: Float = 0.0
    var loudnessHistory: [Float] = []
    let maxHistoryPoints = 100
    
    // UI Elements
    var statusLabel: NSTextField!
    var startButton: NSButton!
    var stopButton: NSButton!
    var connectedDevicesLabel: NSTextField!
    var logTextView: NSTextView!
    
    // Loudness meter UI elements
    var loudnessMeterView: NSView!
    var loudnessLevelLabel: NSTextField!
    var loudnessBarView: NSView!
    var loudnessBarFill: NSView!
    var maxLevelLabel: NSTextField!
    var loudnessGraphView: NSView!
    
    var connectedCentrals: [CBCentral] = []
    
    func applicationDidFinishLaunching(_ aNotification: Notification) {
        setupUI()
        setupBLE()
    }
    
    func setupUI() {
        // Create main window
        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 600, height: 500),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = "Shadow Warrior - Loudness Meter"
        window.center()
        window.makeKeyAndOrderFront(nil)
        
        // Create main view
        let mainView = NSView(frame: window.contentView!.bounds)
        window.contentView = mainView
        
        // Title
        let titleLabel = NSTextField(labelWithString: "🔊 Shadow Warrior Loudness Meter")
        titleLabel.frame = NSRect(x: 20, y: 450, width: 300, height: 30)
        titleLabel.font = NSFont.boldSystemFont(ofSize: 18)
        mainView.addSubview(titleLabel)
        
        // Status label
        statusLabel = NSTextField(labelWithString: "BLE Status: Not Started")
        statusLabel.frame = NSRect(x: 20, y: 420, width: 200, height: 20)
        mainView.addSubview(statusLabel)
        
        // Start button
        startButton = NSButton(title: "Start Advertising", target: self, action: #selector(startAdvertising))
        startButton.frame = NSRect(x: 20, y: 390, width: 120, height: 25)
        mainView.addSubview(startButton)
        
        // Stop button
        stopButton = NSButton(title: "Stop Advertising", target: self, action: #selector(stopAdvertising))
        stopButton.frame = NSRect(x: 150, y: 390, width: 120, height: 25)
        stopButton.isEnabled = false
        mainView.addSubview(stopButton)
        
        // Connected devices label
        connectedDevicesLabel = NSTextField(labelWithString: "Connected Devices: 0")
        connectedDevicesLabel.frame = NSRect(x: 20, y: 360, width: 200, height: 20)
        mainView.addSubview(connectedDevicesLabel)
        
        // Loudness Meter Section
        setupLoudnessMeter(containerView: mainView)
        
        // Log text view
        let logLabel = NSTextField(labelWithString: "Activity Log:")
        logLabel.frame = NSRect(x: 20, y: 200, width: 100, height: 20)
        mainView.addSubview(logLabel)
        
        let scrollView = NSScrollView(frame: NSRect(x: 20, y: 20, width: 560, height: 170))
        logTextView = NSTextView(frame: scrollView.bounds)
        logTextView.isEditable = false
        logTextView.font = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
        scrollView.documentView = logTextView
        scrollView.hasVerticalScroller = true
        mainView.addSubview(scrollView)
    }
    
    func setupLoudnessMeter(containerView: NSView) {
        // Loudness meter container
        loudnessMeterView = NSView(frame: NSRect(x: 20, y: 220, width: 560, height: 120))
        loudnessMeterView.wantsLayer = true
        loudnessMeterView.layer?.backgroundColor = NSColor.controlBackgroundColor.cgColor
        loudnessMeterView.layer?.cornerRadius = 8
        loudnessMeterView.layer?.borderWidth = 1
        loudnessMeterView.layer?.borderColor = NSColor.separatorColor.cgColor
        containerView.addSubview(loudnessMeterView)
        
        // Loudness meter title
        let meterTitle = NSTextField(labelWithString: "🎵 Live Loudness Level")
        meterTitle.frame = NSRect(x: 10, y: 90, width: 200, height: 20)
        meterTitle.font = NSFont.boldSystemFont(ofSize: 14)
        loudnessMeterView.addSubview(meterTitle)
        
        // Current level display
        loudnessLevelLabel = NSTextField(labelWithString: "0.00")
        loudnessLevelLabel.frame = NSRect(x: 10, y: 70, width: 100, height: 20)
        loudnessLevelLabel.font = NSFont.monospacedSystemFont(ofSize: 16, weight: .bold)
        loudnessLevelLabel.textColor = NSColor.systemBlue
        loudnessMeterView.addSubview(loudnessLevelLabel)
        
        // Loudness bar background
        loudnessBarView = NSView(frame: NSRect(x: 10, y: 40, width: 300, height: 20))
        loudnessBarView.wantsLayer = true
        loudnessBarView.layer?.backgroundColor = NSColor.controlColor.cgColor
        loudnessBarView.layer?.cornerRadius = 10
        loudnessBarView.layer?.borderWidth = 1
        loudnessBarView.layer?.borderColor = NSColor.separatorColor.cgColor
        loudnessMeterView.addSubview(loudnessBarView)
        
        // Loudness bar fill
        loudnessBarFill = NSView(frame: NSRect(x: 0, y: 0, width: 0, height: 20))
        loudnessBarFill.wantsLayer = true
        loudnessBarFill.layer?.backgroundColor = NSColor.systemGreen.cgColor
        loudnessBarFill.layer?.cornerRadius = 10
        loudnessBarView.addSubview(loudnessBarFill)
        
        // Max level display
        maxLevelLabel = NSTextField(labelWithString: "Peak: 0.00")
        maxLevelLabel.frame = NSRect(x: 320, y: 40, width: 100, height: 20)
        maxLevelLabel.font = NSFont.systemFont(ofSize: 12)
        maxLevelLabel.textColor = NSColor.systemRed
        loudnessMeterView.addSubview(maxLevelLabel)
        
        // Instructions
        let instructions = NSTextField(labelWithString: "Connect from Shadow Warrior web app to see live audio levels")
        instructions.frame = NSRect(x: 10, y: 10, width: 400, height: 20)
        instructions.font = NSFont.systemFont(ofSize: 11)
        instructions.textColor = NSColor.secondaryLabelColor
        loudnessMeterView.addSubview(instructions)
    }
    
    func setupBLE() {
        peripheralManager = CBPeripheralManager(delegate: self, queue: nil)
    }
    
    @objc func startAdvertising() {
        if peripheralManager.state == .poweredOn {
            peripheralManager.startAdvertising([
                CBAdvertisementDataServiceUUIDsKey: [serviceUUID],
                CBAdvertisementDataLocalNameKey: "ShadowWarrior-BLE"
            ])
            logMessage("Started advertising BLE service")
        }
    }
    
    @objc func stopAdvertising() {
        peripheralManager.stopAdvertising()
        logMessage("Stopped advertising BLE service")
    }
    
    
    func logMessage(_ message: String) {
        let timestamp = DateFormatter.localizedString(from: Date(), dateStyle: .none, timeStyle: .medium)
        let logEntry = "[\(timestamp)] \(message)\n"
        
        DispatchQueue.main.async {
            self.logTextView.string += logEntry
            self.logTextView.scrollToEndOfDocument(nil)
        }
    }
    
    func applicationWillTerminate(_ aNotification: Notification) {
        peripheralManager.stopAdvertising()
    }
    
    func applicationSupportsSecureRestorableState(_ app: NSApplication) -> Bool {
        return true
    }
}

// MARK: - CBPeripheralManagerDelegate
extension AppDelegate: CBPeripheralManagerDelegate {
    
    func peripheralManagerDidUpdateState(_ peripheral: CBPeripheralManager) {
        DispatchQueue.main.async {
            switch peripheral.state {
            case .poweredOn:
                self.statusLabel.stringValue = "BLE Status: Powered On"
                self.startButton.isEnabled = true
                self.setupService()
                self.logMessage("BLE is powered on")
            case .poweredOff:
                self.statusLabel.stringValue = "BLE Status: Powered Off"
                self.startButton.isEnabled = false
                self.stopButton.isEnabled = false
                self.logMessage("BLE is powered off")
            case .resetting:
                self.statusLabel.stringValue = "BLE Status: Resetting"
                self.logMessage("BLE is resetting")
            case .unauthorized:
                self.statusLabel.stringValue = "BLE Status: Unauthorized"
                self.logMessage("BLE access is unauthorized")
            case .unsupported:
                self.statusLabel.stringValue = "BLE Status: Unsupported"
                self.logMessage("BLE is not supported on this device")
            case .unknown:
                self.statusLabel.stringValue = "BLE Status: Unknown"
                self.logMessage("BLE state is unknown")
            @unknown default:
                self.statusLabel.stringValue = "BLE Status: Unknown"
                self.logMessage("BLE state is unknown")
            }
        }
    }
    
    func setupService() {
        // Create characteristic
        characteristic = CBMutableCharacteristic(
            type: characteristicUUID,
            properties: [.read, .write, .notify],
            value: nil,
            permissions: [.readable, .writeable]
        )
        
        // Create service
        service = CBMutableService(type: serviceUUID, primary: true)
        service.characteristics = [characteristic]
        
        // Add service to peripheral manager
        peripheralManager.add(service)
        logMessage("Added BLE service with UUID: \(serviceUUID)")
    }
    
    func peripheralManager(_ peripheral: CBPeripheralManager, didAdd service: CBService, error: Error?) {
        if let error = error {
            logMessage("Error adding service: \(error.localizedDescription)")
        } else {
            logMessage("Successfully added service")
        }
    }
    
    func peripheralManagerDidStartAdvertising(_ peripheral: CBPeripheralManager, error: Error?) {
        DispatchQueue.main.async {
            if let error = error {
                self.logMessage("Error starting advertising: \(error.localizedDescription)")
            } else {
                self.logMessage("Successfully started advertising")
                self.startButton.isEnabled = false
                self.stopButton.isEnabled = true
            }
        }
    }
    
    func peripheralManager(_ peripheral: CBPeripheralManager, central: CBCentral, didSubscribeTo characteristic: CBCharacteristic) {
        DispatchQueue.main.async {
            if !self.connectedCentrals.contains(central) {
                self.connectedCentrals.append(central)
                self.connectedDevicesLabel.stringValue = "Connected Devices: \(self.connectedCentrals.count)"
                self.logMessage("Central subscribed: \(central.identifier)")
            }
        }
    }
    
    func peripheralManager(_ peripheral: CBPeripheralManager, central: CBCentral, didUnsubscribeFrom characteristic: CBCharacteristic) {
        DispatchQueue.main.async {
            if let index = self.connectedCentrals.firstIndex(of: central) {
                self.connectedCentrals.remove(at: index)
                self.connectedDevicesLabel.stringValue = "Connected Devices: \(self.connectedCentrals.count)"
                self.logMessage("Central unsubscribed: \(central.identifier)")
            }
        }
    }
    
    func peripheralManager(_ peripheral: CBPeripheralManager, didReceiveRead request: CBATTRequest) {
        if request.characteristic.uuid == characteristicUUID {
            let response = "Hello from macOS BLE Peripheral!"
            request.value = response.data(using: .utf8)
            peripheral.respond(to: request, withResult: .success)
            logMessage("Responded to read request")
        } else {
            peripheral.respond(to: request, withResult: .attributeNotFound)
        }
    }
    
    func peripheralManager(_ peripheral: CBPeripheralManager, didReceiveWrite requests: [CBATTRequest]) {
        for request in requests {
            if request.characteristic.uuid == characteristicUUID {
                if let data = request.value {
                    // Handle loudness data from Shadow Warrior web app
                    if data.count == 1 {
                        // Single byte represents loudness level (0-255)
                        let loudnessByte = data[0]
                        let loudnessLevel = Float(loudnessByte) / 255.0
                        updateLoudnessMeter(level: loudnessLevel)
                        logMessage("Received loudness level: \(String(format: "%.2f", loudnessLevel))")
                    } else if let message = String(data: data, encoding: .utf8) {
                        // Handle text messages
                        logMessage("Received message: \(message)")
                    }
                }
                peripheral.respond(to: request, withResult: .success)
            } else {
                peripheral.respond(to: request, withResult: .attributeNotFound)
            }
        }
    }
    
    func updateLoudnessMeter(level: Float) {
        DispatchQueue.main.async {
            // Update current level
            self.currentLoudnessLevel = level
            
            // Update max level if current is higher
            if level > self.maxLoudnessLevel {
                self.maxLoudnessLevel = level
            }
            
            // Add to history
            self.loudnessHistory.append(level)
            if self.loudnessHistory.count > self.maxHistoryPoints {
                self.loudnessHistory.removeFirst()
            }
            
            // Update UI
            self.loudnessLevelLabel.stringValue = String(format: "%.2f", level)
            self.maxLevelLabel.stringValue = String(format: "Peak: %.2f", self.maxLoudnessLevel)
            
            // Update progress bar
            let barWidth = CGFloat(level) * self.loudnessBarView.frame.width
            self.loudnessBarFill.frame = NSRect(x: 0, y: 0, width: barWidth, height: 20)
            
            // Change color based on level
            if level <= 0.3 {
                self.loudnessBarFill.layer?.backgroundColor = NSColor.systemGreen.cgColor
            } else if level <= 0.7 {
                self.loudnessBarFill.layer?.backgroundColor = NSColor.systemYellow.cgColor
            } else {
                self.loudnessBarFill.layer?.backgroundColor = NSColor.systemRed.cgColor
            }
        }
    }
}
