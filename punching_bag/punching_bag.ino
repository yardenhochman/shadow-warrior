/*
 * Shadow Warrior Punching Bag Sensor
 * Hardware: Seeed Studio XIAO Sense
 * Libraries required:
 * - Bluefruit52Lib (from Seeed nRF52 core)
 * - Seeed Arduino LSM6DS3 (vendor driver; the Adafruit driver hangs on the
 *   Sense IMU bus)
 */

#include <LSM6DS3.h>
#include <bluefruit.h>
#include <Wire.h>
#include <string.h>

// Custom BLE Service and Characteristic UUIDs
const char *SW_SERVICE_UUID = "6E400001-B5A3-F393-E0A9-E50E24DCCA9E";
const char *SW_ACCEL_CHAR_UUID = "6E400002-B5A3-F393-E0A9-E50E24DCCA9E";
const char *SW_GYRO_CHAR_UUID = "6E400003-B5A3-F393-E0A9-E50E24DCCA9E";
const char *SW_ALPHA_CHAR_UUID = "6E400004-B5A3-F393-E0A9-E50E24DCCA9E";
const char *SW_THRESHOLD_CHAR_UUID = "6E400005-B5A3-F393-E0A9-E50E24DCCA9E";
const char *SW_FIGHT_MODE_CHAR_UUID = "6E400006-B5A3-F393-E0A9-E50E24DCCA9E";

// BLE Service and Characteristics
BLEService shadowWarriorService = BLEService(SW_SERVICE_UUID);
BLECharacteristic accelCharacteristic = BLECharacteristic(SW_ACCEL_CHAR_UUID);
BLECharacteristic gyroCharacteristic = BLECharacteristic(SW_GYRO_CHAR_UUID);
BLECharacteristic alphaCharacteristic = BLECharacteristic(SW_ALPHA_CHAR_UUID);
BLECharacteristic thresholdCharacteristic =
    BLECharacteristic(SW_THRESHOLD_CHAR_UUID);
BLECharacteristic fightModeCharacteristic =
    BLECharacteristic(SW_FIGHT_MODE_CHAR_UUID);
const int BLE_TX_POWER = 0;
const char *BLE_DEVICE_NAME = "ShadowWarriorPunchingBag";

// IMU Instance
LSM6DS3 imu(I2C_MODE, 0x6A);
const float G_TO_MS2 = 9.80665f;      // keep m/s^2 units for accel
const float DPS_TO_RADS = 0.0174533f;  // keep rad/s units for gyro

// Configuration Variables
float accelerationAlpha = 0.8f;
float accelerationThreshold = 10.0f;
bool fightMode = false;
float emaAcceleration = 0.0f;



// XIAO nRF52840 Sense: the LSM6DS3TR-C sits on the internal Wire1 bus (0x6A)
// and is only powered while PIN_LSM6DS3TR_C_POWER is driven high.
const int IMU_PWR_PIN = PIN_LSM6DS3TR_C_POWER;

void connectCallback(uint16_t connHandle) {
  BLEConnection *connection = Bluefruit.Connection(connHandle);
  if (!connection) {
    return;
  }

  char centralName[32] = {0};
  connection->getPeerName(centralName, sizeof(centralName));
  Serial.print("Connected to central: ");
  Serial.println(centralName);
}

// -1 = no request, 0 = stop, 1 = start (set by BLE callback, handled in loop)
volatile int8_t pendingFightMode = -1;

void stopFightMode() {
  if (!fightMode) {
    return;
  }

  fightMode = false;
  fightModeCharacteristic.write8(0);
  digitalWrite(IMU_PWR_PIN, LOW); // save power while idle
  Serial.println("Fight mode: OFF");
}

void startFightMode() {
  if (fightMode) {
    return;
  }

  // imu.begin() powers the IMU via PIN_LSM6DS3TR_C_POWER and inits Wire1.
  if (imu.begin() != 0) {
    Serial.println("IMU Initialization failed!");
    digitalWrite(IMU_PWR_PIN, LOW);
    fightModeCharacteristic.write8(0);
    return;
  }

  delay(100); // first samples after power-up are not ready (read as ~0)
  emaAcceleration = 0.0f;
  fightMode = true;
  fightModeCharacteristic.write8(1);
  Serial.println("Fight mode: ON");
}

void disconnectCallback(uint16_t connHandle, uint8_t reason) {
  (void)connHandle;
  Serial.print("Disconnected from central, reason=0x");
  Serial.println(reason, HEX);
  stopFightMode();
}

void alphaWriteCallback(uint16_t connHandle, BLECharacteristic *chr,
                        uint8_t *data, uint16_t len) {
  (void)connHandle;
  (void)chr;
  if (len != sizeof(float)) {
    return;
  }

  memcpy(&accelerationAlpha, data, sizeof(float));
  Serial.print("Alpha updated: ");
  Serial.println(accelerationAlpha, 4);
}

void thresholdWriteCallback(uint16_t connHandle, BLECharacteristic *chr,
                            uint8_t *data, uint16_t len) {
  (void)connHandle;
  (void)chr;
  if (len != sizeof(float)) {
    return;
  }

  memcpy(&accelerationThreshold, data, sizeof(float));
  Serial.print("Threshold updated: ");
  Serial.println(accelerationThreshold, 4);
}

void fightModeWriteCallback(uint16_t connHandle, BLECharacteristic *chr,
                            uint8_t *data, uint16_t len) {
  (void)connHandle;
  (void)chr;
  if (len != 1) {
    return;
  }

  // Runs in the BLE stack context: no I2C or delay() here. loop() applies it.
  pendingFightMode = data[0] != 0 ? 1 : 0;
}

void startAdvertising() {
  Bluefruit.Advertising.addFlags(BLE_GAP_ADV_FLAGS_LE_ONLY_GENERAL_DISC_MODE);
  Bluefruit.Advertising.addTxPower();
  Bluefruit.Advertising.addService(shadowWarriorService);
  Bluefruit.ScanResponse.addName();
  Bluefruit.Advertising.restartOnDisconnect(true);
  Bluefruit.Advertising.setInterval(32, 244);
  Bluefruit.Advertising.setFastTimeout(30);
  Bluefruit.Advertising.start(0);
}

void setup() {
  Serial.begin(115200);
  while (!Serial) {
    delay(10); // Wait for Serial to be ready
  }
  Serial.println("=== Shadow Warrior BLE Device ===");
  Serial.println("Serial initialized successfully!");

  // Initialize BLE
  Bluefruit.begin();
  Bluefruit.setName(BLE_DEVICE_NAME);
  Bluefruit.setTxPower(0);
  Bluefruit.Periph.setConnectCallback(connectCallback);
  Bluefruit.Periph.setDisconnectCallback(disconnectCallback);

  shadowWarriorService.begin();

  accelCharacteristic.setProperties(CHR_PROPS_READ | CHR_PROPS_NOTIFY);
  accelCharacteristic.setPermission(SECMODE_OPEN, SECMODE_NO_ACCESS);
  accelCharacteristic.setFixedLen(sizeof(float));
  accelCharacteristic.begin();
  accelCharacteristic.writeFloat(0.0f);

  gyroCharacteristic.setProperties(CHR_PROPS_READ | CHR_PROPS_NOTIFY);
  gyroCharacteristic.setPermission(SECMODE_OPEN, SECMODE_NO_ACCESS);
  gyroCharacteristic.setFixedLen(12);
  gyroCharacteristic.begin();
  float zeroGyro[3] = {0.0f, 0.0f, 0.0f};
  gyroCharacteristic.write(zeroGyro, 12);

  alphaCharacteristic.setProperties(CHR_PROPS_READ | CHR_PROPS_WRITE);
  alphaCharacteristic.setPermission(SECMODE_OPEN, SECMODE_OPEN);
  alphaCharacteristic.setFixedLen(sizeof(float));
  alphaCharacteristic.setWriteCallback(alphaWriteCallback);
  alphaCharacteristic.begin();
  alphaCharacteristic.writeFloat(accelerationAlpha);

  thresholdCharacteristic.setProperties(CHR_PROPS_READ | CHR_PROPS_WRITE);
  thresholdCharacteristic.setPermission(SECMODE_OPEN, SECMODE_OPEN);
  thresholdCharacteristic.setFixedLen(sizeof(float));
  thresholdCharacteristic.setWriteCallback(thresholdWriteCallback);
  thresholdCharacteristic.begin();
  thresholdCharacteristic.writeFloat(accelerationThreshold);

  fightModeCharacteristic.setProperties(CHR_PROPS_READ | CHR_PROPS_WRITE);
  fightModeCharacteristic.setPermission(SECMODE_OPEN, SECMODE_OPEN);
  fightModeCharacteristic.setFixedLen(1);
  fightModeCharacteristic.setWriteCallback(fightModeWriteCallback);
  fightModeCharacteristic.begin();
  fightModeCharacteristic.write8(0);

  startAdvertising();
  Serial.printf("BLE Advertising as %s...\n", BLE_DEVICE_NAME);
}

void loop() {
  int8_t req = pendingFightMode;
  if (req >= 0) {
    pendingFightMode = -1;
    if (req == 1) {
      startFightMode();
    } else {
      stopFightMode();
    }
  }
  if (fightMode && Bluefruit.connected()) {
    processImuData();
  } else {
    delay(100);
  }
}

void processImuData() {
  float ax = imu.readFloatAccelX() * G_TO_MS2;
  float ay = imu.readFloatAccelY() * G_TO_MS2;
  float az = imu.readFloatAccelZ() * G_TO_MS2;

  float currentAccelTotal = sqrt(ax * ax + ay * ay + az * az);
  // Remove gravity: only deviation from 1 g counts, so a bag at rest reads ~0
  // and thresholds are in m/s^2 of real motion.
  currentAccelTotal = fabsf(currentAccelTotal - G_TO_MS2);

  // Exponential Moving Average
  emaAcceleration = (accelerationAlpha * currentAccelTotal) +
                    ((1.0f - accelerationAlpha) * emaAcceleration);

  if (emaAcceleration > accelerationThreshold) {
    accelCharacteristic.notify32(emaAcceleration);

    // Also send raw gyro data if needed
    float gx = imu.readFloatGyroX() * DPS_TO_RADS;
    float gy = imu.readFloatGyroY() * DPS_TO_RADS;
    float gz = imu.readFloatGyroZ() * DPS_TO_RADS;
    float gyroData[3] = {gx, gy, gz};
    gyroCharacteristic.notify((uint8_t *)gyroData, 12);

    Serial.print("Punch Detected! Accel: ");
    Serial.println(emaAcceleration);
  }

  delay(100); // 10Hz update rate
}
