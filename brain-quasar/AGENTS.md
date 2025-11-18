BEFORE ANYTHING ELSE: run 'bd onboard' and follow the instructions

# Dev
to build: `npm run build && npx cap sync android`
to build and deploy: ./build_and_deploy.sh
(the following assumes fish shell)
If JAVA_HOME is unset: `set -x JAVA_HOME /Applications/Android\ Studio.app/Contents/jbr/Contents/Home`
If adb is not found: `set -x PATH $PATH ~/Library/Android/sdk/platform-tools/`

# Shadow Warrior brain

This project is capacitor/quasar app which is a controller for an art installation (Shadow Warrior arena) which has the following parts:

- Led controllers (connected by BLE/WiFi) to brain
- Brain (this app)
- Speakers (connected via Bluetooth to brain)
- Optional UV lights (smart WiFi relay)
  Brain device is mounted on a punching bag and uses accelerometer to detect punches. It also uses the microphone to detect shouts.

The flow is:

1. Idle state - Leds in "standby" mode, punching bag is disabled
2. Warming - when shouts are detected Leds are sent signal to pulse (correlated with shout amplitude). When threshold is reached switch to _fight_ mode
3. Fight - Music is played from the speakers, punching bag is active. Punches are recorded by the accelerometer as well as shout amplitude and leds pulse correlated with punch and shouting intensity. After threshold is reached, switch to _victory_
4. Victory - victory music is played, leds show victory pattern. After that switch to _cooldown_ mode
5. Cooldown - arena is inactive. Leds are off music is off. After 5 minutes go back to Idle

## State machine

Idle -> Warming
Warming -> [Fight, Idle]
Fight -> [Idle, Victory]
Victory -> Cooldown
Cooldown -> Idle

# Dev

We are using Android phone (do not build IOS)

Build command: `npx quasar build -m capacitor -T android`
