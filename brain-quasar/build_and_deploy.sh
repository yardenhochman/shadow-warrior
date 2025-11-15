#!/bin/bash

# Exit immediately if a command exits with a non-zero status.
set -e

# Build the web assets
npm run build

# Sync the web assets to the native projects
npx cap sync

# Build the Android app
if [[ -r ".android-target" ]]; then
	ANDROID_TARGET=$(<.android-target)
	echo "Using Android target: $ANDROID_TARGET"
	npx cap run android --target "$ANDROID_TARGET"
else
	echo "No .android-target file found. Building with default target."
	npx cap run android
fi

# Optional: Deploy to an Android device
# (cd android && ./gradlew installDebug)

# Optional: Open the Android project in Android Studio
# open -a "Android Studio" android

echo "Build and deployment successful!"
