import { readFileSync, writeFileSync } from 'fs';
import { execSync } from 'child_process';
import path from 'path';

/**
 * Entropy Release & Update Generator
 * 
 * This script automates:
 * 1. Generating signatures for the updater
 * 2. Creating the update.json manifest for your website
 * 3. Preparing the landing site for deployment
 */

const TAURI_CONF_PATH = './src-tauri/tauri.conf.json';
const LANDING_SITE_PATH = '../landing-site';
const GITHUB_REPO = 'https://github.com/entropy-messenger/entropy'; // Update if repo name is different

async function publish() {
    console.log('🚀 Preparing Entropy Release...');

    // 1. Read version from tauri.conf
    const tauriConf = JSON.parse(readFileSync(TAURI_CONF_PATH, 'utf-8'));
    const version = tauriConf.version;
    console.log(`📦 Targeted Version: ${version}`);

    // 2. Define the manifest structure
    const manifest = {
        version: version,
        notes: `Release v${version}`,
        pub_date: new Date().toISOString(),
        platforms: {
            "linux-x86_64": {
                "signature": "", // Will be filled manually or via build output
                "url": `${GITHUB_REPO}/releases/latest/download/entropy_amd64.AppImage`
            },
            "windows-x86_64": {
                "signature": "",
                "url": `${GITHUB_REPO}/releases/latest/download/entropy_x64_en-US.msi.zip`
            },
            "darwin-x86_64": {
                "signature": "",
                "url": `${GITHUB_REPO}/releases/latest/download/entropy_x64.dmg`
            }
        }
    };

    // 3. Save to Landing Site
    const manifestPath = path.join(LANDING_SITE_PATH, 'update.json');
    writeFileSync(manifestPath, JSON.stringify(manifest, null, 2));
    
    console.log(`✅ Update manifest generated at ${manifestPath}`);
    console.log(`\nNext Steps:`);
    console.log(`1. Run 'npm run tauri build' with your TAURI_SIGNING_PRIVATE_KEY set.`);
    console.log(`2. Copy the .sig file contents into the manifest.`);
    console.log(`3. Push the 'landing-site' changes to your web server.`);
}

publish().catch(console.error);
