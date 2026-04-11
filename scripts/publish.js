import { readFileSync, writeFileSync } from 'fs';
import path from 'path';

/**
 * Entropy Release Manifest Generator
 * 
 * Syncs the current version from the app to the landing site's update manifest.
 */

const TAURI_CONF_PATH = './src-tauri/tauri.conf.json';
const LANDING_SITE_PATH = '../landing-site';
const GITHUB_RELEASES = 'https://github.com/entropy-messenger/entropydesktop-releases/releases/latest/download';

async function publish() {
    console.log('🚀 Preparing Entropy Release Manifest...');

    // 1. Read version from tauri.conf
    const tauriConf = JSON.parse(readFileSync(TAURI_CONF_PATH, 'utf-8'));
    const version = tauriConf.version;
    console.log(`📦 Targeted Version: ${version}`);

    // 2. Define the manifest structure for ALL supported platforms
    const manifest = {
        version: version,
        pub_date: new Date().toISOString(),
        platforms: {
            "linux-x86_64": {
                "appimage": `${GITHUB_RELEASES}/Entropy_${version}_amd64.AppImage`,
                "deb": `${GITHUB_RELEASES}/entropy_${version}_amd64.deb`,
                "rpm": `${GITHUB_RELEASES}/Entropy-${version}-1.x86_64.rpm`
            },
            "windows-x86_64": {
                "nsis": `${GITHUB_RELEASES}/Entropy_${version}_x64-setup.exe`,
                "msi": `${GITHUB_RELEASES}/Entropy_${version}_x64_en-US.msi`
            },
            "darwin-x86_64": {
                "dmg": `${GITHUB_RELEASES}/Entropy_${version}_x64.dmg`
            },
            "darwin-aarch64": {
                "dmg": `${GITHUB_RELEASES}/Entropy_${version}_aarch64.dmg`
            }
        }
    };

    // 3. Save to Landing Site
    const manifestPath = path.join(LANDING_SITE_PATH, 'update.json');
    writeFileSync(manifestPath, JSON.stringify(manifest, null, 2));
    
    console.log(`✅ Update manifest generated for all platforms at ${manifestPath}`);
}

publish().catch(console.error);
