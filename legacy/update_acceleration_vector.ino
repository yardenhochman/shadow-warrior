#include <ArduinoBLE.h>
#include "LSM6DS3.h"
#include "Wire.h"
#include <mic.h>

static const char BLE_NAME[] = "SEEEDUINO1";

#define SAMPLES 100

mic_config_t mic_config{
  .channel_cnt = 1,
  .sampling_rate = 16000,
  .buf_size = 1600,
  .debug_pin = LED_BUILTIN
};

NRF52840_ADC_Class Mic(&mic_config);

int16_t recording_buf[SAMPLES];
volatile uint8_t recording = 0;
volatile static bool record_ready = false;

LSM6DS3 myIMU(I2C_MODE, 0x6A);
float sound_sample = 0;
float acc[5];
float old_acc[5];
long previousMillis = 0;
int float_array_len = 5; 
BLEService accService("1100");
BLECharacteristic accChar("2803", BLERead | BLENotify, 20);
bool is_serial = false;

void setup()
{
  for (int i=0; i < float_array_len; i++) {
      acc[i] = 0;
      old_acc[i] = -1;
  }
  Serial.begin(9600);
  if (is_serial) {
    while (!Serial);
  }
  pinMode(LED_BUILTIN, OUTPUT);
  if (!BLE.begin()) 
  {
    if (is_serial) {
      Serial.println("starting BLE failed!");
      while (1);
    }
  }
  BLE.setLocalName(BLE_NAME);
  BLE.setAdvertisedService(accService);
  accService.addCharacteristic(accChar);
  BLE.addService(accService);
  accChar.setValue((const uint8_t*)acc, 20);
  BLE.advertise();
  //Serial.println("Bluetooth® device active, waiting for connections...");
  if (myIMU.begin() != 0) {
    if(is_serial) Serial.println("IMU error");
  } else {
    if(is_serial) Serial.println("IMU OK!");
  }
  Mic.set_callback(audio_rec_callback);
  if (!Mic.begin()) {
    if(is_serial) Serial.println("Mic initialization failed");
    while (1);
  }
  if(is_serial) Serial.println("Mic initialization done.");
}
 
 
void loop()
{
  // wait for a Bluetooth® Low Energy central
  BLEDevice central = BLE.central();
  if (central)
  {
    //Serial.print("Connected to central: ");
    //Serial.println(central.address());
    digitalWrite(LED_BUILTIN, LOW); 
    while (central.connected())
    {
      long currentMillis = millis();
      if(is_serial) Serial.println(currentMillis - previousMillis);
      if (currentMillis - previousMillis >= 1)
      {
        previousMillis = currentMillis;
        update();
      }
    }
    digitalWrite(LED_BUILTIN, HIGH);
    Serial.print("Disconnected from central: ");
    Serial.println(central.address());
  }
}

float get_recording_sample() {
  long sum=0;
  float avg = 0;
  //Serial.println("Finished sampling");
  for (int i = 0; i < SAMPLES; i++) {
    avg += abs(recording_buf[i]);
  }
  avg = avg / SAMPLES;
  record_ready = false; 
  return avg;
}

void update()
{
  float x = myIMU.readFloatAccelX();
  float y = myIMU.readFloatAccelY();
  float z = myIMU.readFloatAccelZ();
  if (old_acc[1] != x || old_acc[2] != y || old_acc[3] != z || record_ready)
  {
    if (record_ready) {
      sound_sample = get_recording_sample();
    }
    if(is_serial) {
      Serial.print(" X = ");
      Serial.println(x, 4);
      Serial.print(" Y = ");
      Serial.println(y, 4);
      Serial.print(" Z = ");
      Serial.println(z, 4);
      Serial.print(" vol = ");
      Serial.println(sound_sample, 4);
    }
    acc[1] = x;
    acc[2] = y;
    acc[3] = z;
    acc[4] = sound_sample;
    accChar.setValue((const uint8_t*)acc, 20);
    for (int i=0; i < float_array_len; i++) {
      old_acc[i] = acc[i];
    }
  }
}

static void audio_rec_callback(uint16_t *buf, uint32_t buf_len) {
  static uint32_t idx = 0;
  // Copy samples from DMA buffer to inference buffer
  {
    for (uint32_t i = 0; i < buf_len; i++) {
      recording_buf[idx++] = buf[i];
      if (idx >= SAMPLES){ 
        idx = 0;
        recording = 0;
        record_ready = true;
        break;
     } 
    }
  }
}