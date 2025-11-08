use esp32_nimble::utilities::BleUuid;
use esp32_nimble::{BLEDevice, BLEServer, NimbleProperties};
use std::sync::{Arc, Mutex};

// BLE UUIDs matching the MicroPython implementation
pub const SERVICE_UUID: &str = "d08d81bb-7270-45de-a475-5b52feb820b6";
pub const RX_CHAR_UUID: &str = "8f97424f-8c2f-4a86-9e53-92059ccb1559"; // Write to device
pub const TX_CHAR_UUID: &str = "6E400003-B5A3-F393-E0A9-E50E24DCCA9E"; // Read/Notify from device
pub const IP_ADDR_UUID: &str = "00000001-0000-1000-8000-00805f9b34fb"; // IP address characteristic

pub type CommandCallback = Arc<dyn Fn(&[u8]) + Send + Sync>;

pub struct BleService {
    _server: &'static mut BLEServer,
    command_buffer: Arc<Mutex<Option<Vec<u8>>>>,
}

impl BleService {
    pub fn new(device_name: &str, on_command: CommandCallback) -> anyhow::Result<Self> {
        log::info!("Initializing BLE service with name: {}", device_name);

        // Initialize BLE device
        let ble_device = BLEDevice::take();
        BLEDevice::set_device_name(device_name)
            .map_err(|_| anyhow::anyhow!("Failed to set device name"))?;

        // Create BLE server
        let server = ble_device.get_server();

        // Create the UART service
        let service_uuid = BleUuid::from_uuid128_string(SERVICE_UUID)
            .map_err(|e| anyhow::anyhow!("Invalid service UUID: {}", e))?;
        let service = server.create_service(service_uuid);

        let command_buffer = Arc::new(Mutex::new(None));
        let buffer_clone = command_buffer.clone();

        // Create RX characteristic (write from client to device)
        let rx_uuid = BleUuid::from_uuid128_string(RX_CHAR_UUID)
            .map_err(|e| anyhow::anyhow!("Invalid RX UUID: {}", e))?;
        let rx_characteristic = service.lock().create_characteristic(
            rx_uuid,
            NimbleProperties::WRITE,
        );

        // Set callback for received data
        rx_characteristic.lock().on_write(move |args| {
            log::info!("Received BLE command: {:?}", args.recv_data());
            let data = args.recv_data().to_vec();

            // Store command in buffer
            if let Ok(mut buffer) = buffer_clone.lock() {
                *buffer = Some(data.clone());
            }

            // Execute callback
            on_command(&data);
        });

        // Create TX characteristic (read/notify from device to client)
        let tx_uuid = BleUuid::from_uuid128_string(TX_CHAR_UUID)
            .map_err(|e| anyhow::anyhow!("Invalid TX UUID: {}", e))?;
        let _tx_characteristic = service.lock().create_characteristic(
            tx_uuid,
            NimbleProperties::READ | NimbleProperties::NOTIFY,
        );

        // Create IP address characteristic (read-only)
        let ip_uuid = BleUuid::from_uuid128_string(IP_ADDR_UUID)
            .map_err(|e| anyhow::anyhow!("Invalid IP UUID: {}", e))?;
        let ip_characteristic = service.lock().create_characteristic(
            ip_uuid,
            NimbleProperties::READ,
        );

        // Set initial IP address
        ip_characteristic.lock().set_value(b"0.0.0.0");

        // Start advertising
        let ble_advertising = ble_device.get_advertising();
        let adv_service_uuid = BleUuid::from_uuid128_string(SERVICE_UUID)
            .map_err(|e| anyhow::anyhow!("Invalid service UUID for advertising: {}", e))?;

        ble_advertising
            .lock()
            .set_data(
                esp32_nimble::BLEAdvertisementData::new()
                    .name(device_name)
                    .add_service_uuid(adv_service_uuid)
            )
            .map_err(|_| anyhow::anyhow!("Failed to set advertising data"))?;

        ble_advertising
            .lock()
            .start()
            .map_err(|_| anyhow::anyhow!("Failed to start advertising"))?;

        log::info!("BLE service initialized and advertising started");

        Ok(Self {
            _server: server,
            command_buffer,
        })
    }

    pub fn get_last_command(&self) -> Option<Vec<u8>> {
        self.command_buffer.lock().ok()?.take()
    }

    pub fn update_ip_address(&self, ip: &str) -> anyhow::Result<()> {
        // Find the IP characteristic and update it
        log::info!("Updating IP address characteristic to: {}", ip);
        // Note: In a real implementation, we'd need to store a reference to the characteristic
        // This is a simplified version
        Ok(())
    }
}
