import { readFileSync, writeFileSync } from 'fs';
import path from 'path';
import https from 'https';

/**
 * Entropy Release Manifest Generator
 * 
 * Syncs the current version from the app to the landing site's update manifest.
 * Automated to fetch real SHA-256 hashes from GitHub checksums.txt.
 */

const TAURI_CONF_PATH = './src-tauri/tauri.conf.json';
const LANDING_SITE_PATH = '../landing-site';
const REPO_OWNER = 'entropy-messenger';
const REPO_NAME = 'entropydesktop-releases';
const GITHUB_RELEASES = `https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/latest/download`;

async function getGitHubData(url) {
    return new Promise((resolve, reject) => {
        const options = {
            headers: { 'User-Agent': 'Entropy-Publisher' }
        };
        https.get(url, options, (res) => {
            if (res.statusCode === 302) return getGitHubData(res.headers.location).then(resolve).catch(reject);
            let data = '';
            res.on('data', (chunk) => data += chunk);
            res.on('end', () => resolve(data));
        }).on('error', reject);
    });
}

async function publish() {
    console.log('🚀 Preparing Entropy Release Manifest...');

    const tauriConf = JSON.parse(readFileSync(TAURI_CONF_PATH, 'utf-8'));
    const version = tauriConf.version;
    console.log(`📦 Targeted Version: ${version}`);

    console.log('🔍 Checking GitHub for checksums.txt...');
    let onlineHashes = {};
    try {
        const releaseJson = await getGitHubData(`https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}/releases/latest`);
        const release = JSON.parse(releaseJson);
        const checksumAsset = release.assets.find(a => a.name === 'checksums.txt');
        
        if (checksumAsset) {
            console.log('📄 Found checksums.txt, auto-populating hashes...');
            const rawChecksums = await getGitHubData(checksumAsset.browser_download_url);
            // Parse common sha256sum format: "[hash] [filename]"
            rawChecksums.split('\n').forEach(line => {
                const parts = line.trim().split(/\s+/);
                if (parts.length >= 2) {
                    const [hash, filename] = parts;
                    if (filename.includes('.AppImage')) onlineHashes['linux-appimage'] = hash;
                    if (filename.includes('.deb')) onlineHashes['linux-deb'] = hash;
                    if (filename.includes('.rpm')) onlineHashes['linux-rpm'] = hash;
                    if (filename.includes('x64-setup.exe')) onlineHashes['windows-nsis'] = hash;
                    if (filename.includes('_x64.dmg')) onlineHashes['mac-intel'] = hash;
                    if (filename.includes('_aarch64.dmg')) onlineHashes['mac-silicon'] = hash;
                }
            });
            console.log('✅ Successfully mapped hashes from GitHub.');
        } else {
            console.log('ℹ️ No checksums.txt found on GitHub. Using placeholders.');
        }
    } catch (err) {
        console.warn('⚠️ GitHub API limit or connection error. Skipping auto-hash.');
    }

    const manifest = {
        version: version,
        pub_date: new Date().toISOString(),
        platforms: {
            "linux-x86_64": {
                "appimage": `${GITHUB_RELEASES}/Entropy_${version}_amd64.AppImage`,
                "appimage_sha256": onlineHashes['linux-appimage'] || "...",
                "deb": `${GITHUB_RELEASES}/entropy_${version}_amd64.deb`,
                "deb_sha256": onlineHashes['linux-deb'] || "...",
                "rpm": `${GITHUB_RELEASES}/Entropy-${version}-1.x86_64.rpm`,
                "rpm_sha256": onlineHashes['linux-rpm'] || "..."
            },
            "windows-x86_64": {
                "nsis": `${GITHUB_RELEASES}/Entropy_${version}_x64-setup.exe`,
                "msi": `${GITHUB_RELEASES}/Entropy_${version}_x64_en-US.msi`,
                "sha256": onlineHashes['windows-nsis'] || "..."
            },
            "darwin-x86_64": {
                "dmg": `${GITHUB_RELEASES}/Entropy_${version}_x64.dmg`,
                "sha256": onlineHashes['mac-intel'] || "..."
            },
            "darwin-aarch64": {
                "dmg": `${GITHUB_RELEASES}/Entropy_${version}_aarch64.dmg`,
                "sha256": onlineHashes['mac-silicon'] || "..."
            }
        }
    };

    const manifestPath = path.join(LANDING_SITE_PATH, 'update.json');
    writeFileSync(manifestPath, JSON.stringify(manifest, null, 2));

    const downloadHtmlPath = path.join(LANDING_SITE_PATH, 'download.html');
    let htmlContent = readFileSync(downloadHtmlPath, 'utf-8');

    const linkMap = {
        'win-download': manifest.platforms['windows-x86_64'].nsis,
        'mac-silicon': manifest.platforms['darwin-aarch64'].dmg,
        'mac-intel': manifest.platforms['darwin-x86_64'].dmg,
        'linux-appimage': manifest.platforms['linux-x86_64'].appimage,
        'linux-deb': manifest.platforms['linux-x86_64'].deb,
        'linux-rpm': manifest.platforms['linux-x86_64'].rpm
    };

    Object.entries(linkMap).forEach(([id, url]) => {
        const tagRegex = new RegExp(`(<a[^>]*id="${id}"[^>]*>)`, 'g');
        htmlContent = htmlContent.replace(tagRegex, (tag) => {
            return tag.replace(/href="[^"]*"/, `href="${url}"`);
        });
    });

    htmlContent = htmlContent.replace(/(<div class="announcement-banner">)([^<]*)(<\/div>)/, 
        `$1ALPHA TESTING PHASE v${version}$3`);
        
    htmlContent = htmlContent.replace(/(<div class="version-tag">)([^<]*)(<\/div>)/g, 
        `$1build v${version}$3`);

    writeFileSync(downloadHtmlPath, htmlContent);
    console.log(`✅ Build Complete. Site is synced with v${version} release.`);
}

publish().catch(console.error);
