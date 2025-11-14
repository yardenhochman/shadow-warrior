#!/usr/bin/env node
import { readFileSync, writeFileSync } from 'fs';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const packageJsonPath = join(__dirname, '..', 'package.json');

// Read package.json
const packageJson = JSON.parse(readFileSync(packageJsonPath, 'utf-8'));

// Initialize or increment build number
if (!packageJson.buildNumber) {
  packageJson.buildNumber = 1;
} else {
  packageJson.buildNumber = parseInt(packageJson.buildNumber) + 1;
}

// Write back to package.json
writeFileSync(packageJsonPath, JSON.stringify(packageJson, null, 2) + '\n');

console.log(`Build number incremented to: ${packageJson.buildNumber}`);
console.log(`Version: ${packageJson.version}-${packageJson.buildNumber}`);
