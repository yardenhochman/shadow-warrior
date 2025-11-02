#!/bin/bash

# Exit immediately if a command exits with a non-zero status.
set -e

# Build the web assets
npx quasar build

# Sync the web assets to the native projects
npx cap sync

# Build the Android app
(cd android && ./gradlew assembleDebug)

# Optional: Deploy to an Android device
# (cd android && ./gradlew installDebug)

# Optional: Open the Android project in Android Studio
# open -a "Android Studio" android

echo "Build and deployment successful!"
