<script lang="ts">
  import { userStore } from '../lib/stores/user';
  import { updateMyProfile, toggleBlock, updatePrivacy, registerGlobalNickname } from '../lib/actions/contacts';
  import { burnAccount } from '../lib/actions/auth';
  import { invoke } from '@tauri-apps/api/core';
  import { addToast, showConfirm, showPrompt } from '../lib/stores/ui';
  import { network } from '../lib/network';
  import QRCode from 'qrcode';
  import {
    LucidePlus, LucideCamera, LucideUser, LucideLock, LucideCheckCheck, 
    LucideBan, LucideEyeOff, LucideShieldAlert, LucideGlobe, LucideTrash2, 
    LucideSun, LucideMoon
  } from 'lucide-svelte';
  import Avatar from './Avatar.svelte';

  let { onClose } = $props<{ onClose: () => void }>();

  let settingsTab = $state<'profile' | 'privacy' | 'blocked'>('profile');
  let isRegisteringNickname = $state(false);
  let qrCodeUrl = $state<string>("");
  let copied = $state(false);

  $effect(() => {
      if ($userStore.identityHash) {
          QRCode.toDataURL($userStore.identityHash, {
              width: 400,
              margin: 2,
              color: { dark: '#000000', light: '#ffffff' }
          }).then(url => qrCodeUrl = url).catch(console.error);
      }
  });

  const copyHash = async () => {
      if ($userStore.identityHash) {
          try {
              await navigator.clipboard.writeText($userStore.identityHash);
          } catch(e) {}
          copied = true;
          setTimeout(() => copied = false, 2000);
      }
  };

  const exportVault = async () => {
    addToast("Starting export...", 'info'); 
    try {
        if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
            const { save } = await import('@tauri-apps/plugin-dialog');
            const path = await save({
                defaultPath: `entropy_backup_${Date.now()}.entropy`,
                filters: [{ name: 'Entropy Backup', extensions: ['entropy', 'zip'] }]
            });
            if (path) {
                await invoke('export_database', { targetPath: path });
                addToast("Backup exported successfully!", 'success');
            } else {
                addToast("Export cancelled.", 'info');
            }
        } else {
            addToast("Export not supported in web mode.", 'warning');
        }
    } catch (e) {
        addToast("Export failed: " + e, 'error');
    }
  };

  const importVault = async () => {
    if (!await showConfirm("WARNING: Importing a backup will OVERWRITE all current data. This cannot be undone. Continue?", "Restore Backup")) return;
    try {
        if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
            const { open } = await import('@tauri-apps/plugin-dialog');
            const path = await open({
                multiple: false,
                filters: [{ name: 'Entropy Backup', extensions: ['entropy', 'zip'] }]
            });
            if (path) {
                await invoke('import_database', { srcPath: path });
                addToast("Backup restored! The app will now reload.", 'success');
                setTimeout(() => window.location.reload(), 2000);
            }
        }
    } catch (e) {
        addToast("Import failed: " + e, 'error');
    }
  };


  const handleRegisterNickname = async () => {
      const nick = await showPrompt("Register a global nickname (min 3 chars):", $userStore.globalNickname || "", "Global Nickname");
      if (nick && nick.length >= 3) {
          isRegisteringNickname = true;
          try {
              const res = await registerGlobalNickname(nick);
              if (res?.success) addToast("Nickname registered successfully!", 'success');
              else addToast("Registration failed: " + (res?.error || "Unknown"), 'error');
          } finally { isRegisteringNickname = false; }
      } else if (nick) addToast("Nickname too short", 'error');
  };

</script>

<div class="absolute inset-0 bg-entropy-bg z-[60] flex flex-col animate-in slide-in-from-bottom duration-300">
    <div class="p-4 flex justify-between items-center bg-entropy-surface">
        <h2 class="font-bold text-entropy-text-primary">Settings</h2>
        <button onclick={onClose} class="text-entropy-text-secondary hover:text-entropy-text-primary font-bold">Done</button>
    </div>
    
    <div class="flex text-[10px] font-black uppercase tracking-widest text-entropy-text-dim">
        <button onclick={() => settingsTab = 'profile'} class="flex-1 py-3 {settingsTab === 'profile' ? 'text-entropy-primary border-b-2 border-entropy-primary' : ''}">Profile</button>
        <button onclick={() => settingsTab = 'privacy'} class="flex-1 py-3 {settingsTab === 'privacy' ? 'text-entropy-primary border-b-2 border-entropy-primary' : ''}">Privacy</button>
        <button onclick={() => settingsTab = 'blocked'} class="flex-1 py-3 {settingsTab === 'blocked' ? 'text-entropy-primary border-b-2 border-entropy-primary' : ''}">Blocked</button>
    </div>

    <div class="p-6 space-y-8 flex-1 overflow-y-auto custom-scrollbar">
        {#if settingsTab === 'profile'}
            <div class="flex flex-col items-center space-y-4">
                <Avatar hash={$userStore.identityHash || ''} alias={$userStore.globalNickname || 'Anonymous'} size="w-24 h-24" textSize="text-3xl" rounded="rounded-3xl" />
                <div class="text-center space-y-1">
                    <div class="text-xl font-bold text-entropy-text-primary flex items-center justify-center space-x-2">
                        <span>{$userStore.globalNickname || 'Anonymous'}</span>
                    </div>
                    <div class="text-xs font-bold text-entropy-text-dim uppercase tracking-widest">Active Identity</div>
                </div>

                <div class="w-full flex flex-col space-y-2">
                    <button 
                        disabled={isRegisteringNickname || $userStore.connectionStatus !== 'connected'}
                        onclick={handleRegisterNickname}
                        class="w-full py-3 bg-entropy-primary text-white rounded-xl text-sm font-bold shadow-lg hover:bg-entropy-primary-dim transition-all active:scale-95 flex flex-col items-center justify-center space-y-1 overflow-hidden disabled:opacity-50 disabled:grayscale"
                    >
                        {#if isRegisteringNickname}
                            <div class="flex items-center space-x-2 animate-pulse"><div class="w-3 h-3 border-2 border-white/20 border-t-white rounded-full animate-spin"></div><span>Authenticating Identity...</span></div>
                            <div class="text-[10px] font-black uppercase tracking-[0.2em] opacity-60">Identity Verification</div>
                        {:else}
                            <div class="flex items-center space-x-2"><img src="/logo.png" alt="logo" class="w-6 h-6 object-contain invert opacity-40" /><span>{$userStore.globalNickname ? 'Change Global Nickname' : 'Register Global Nickname'}</span></div>
                        {/if}
                    </button>

                    <div class="flex space-x-2">
                        <button onclick={exportVault} class="flex-1 py-3 bg-entropy-surface-light text-entropy-text-secondary rounded-xl text-xs font-bold hover:bg-entropy-surface transition">Export Backup</button>
                        <button onclick={importVault} class="flex-1 py-3 bg-entropy-surface-light text-entropy-text-secondary rounded-xl text-xs font-bold hover:bg-entropy-surface transition">Import Backup</button>
                    </div>
                </div>
            </div>

            <div class="bg-entropy-surface-light p-4 rounded-2xl space-y-3">
                <div class="text-[10px] font-bold text-entropy-primary uppercase tracking-widest">Global Identity Hash</div>
                <div class="break-all font-mono text-[10px] text-entropy-text-primary bg-entropy-bg/50 p-2 rounded select-all leading-tight">{$userStore.identityHash || 'Generating...'}</div>
                <button onclick={copyHash} class="w-full py-3 bg-entropy-surface text-entropy-primary rounded-xl text-sm font-bold shadow-sm hover:bg-entropy-surface-light transition">{copied ? 'Copied!' : 'Copy Hash Address'}</button>
                
                <div class="flex justify-center mt-2">
                    <div class="bg-white p-2 rounded-xl shadow-sm relative overflow-hidden group/qr">
                        {#if qrCodeUrl}
                            <img src={qrCodeUrl} alt="QR Identity" class="w-32 h-32 blur-[2px] group-hover/qr:blur-0 transition-all duration-300" />
                        {:else}
                            <div class="w-32 h-32 bg-gray-100 flex items-center justify-center"><div class="w-6 h-6 border-2 border-entropy-primary border-t-transparent rounded-full animate-spin"></div></div>
                        {/if}
                        <div class="absolute inset-0 bg-white/40 flex items-center justify-center opacity-100 group-hover/qr:opacity-0 transition-opacity"><LucideLock size={24} class="text-blue-600" /></div>
                    </div>
                </div>
            </div>
        {:else if settingsTab === 'privacy'}
                <div class="space-y-6">
                    <div class="space-y-1">
                        <h3 class="font-bold text-entropy-text-primary flex items-center space-x-2"><LucideCheckCheck size={18} class="text-entropy-primary" /><span>Read Receipts</span></h3>
                        <p class="text-xs text-entropy-text-secondary leading-relaxed">If turned off, you won't send or receive Read Receipts (blue checks). Personal privacy first.</p>
                        <div class="flex justify-end pt-2">
                            <button onclick={() => updatePrivacy({ readReceipts: !$userStore.privacySettings.readReceipts })} class="w-12 h-6 rounded-full transition-colors relative {$userStore.privacySettings.readReceipts ? 'bg-entropy-primary' : 'bg-entropy-surface-light'}" aria-label="Toggle Read Receipts">
                                <div class="absolute top-1 w-4 h-4 bg-white rounded-full transition-all {$userStore.privacySettings.readReceipts ? 'left-7' : 'left-1'}"></div>
                            </button>
                        </div>
                    </div>

                    <div class="space-y-1">
                        <h3 class="font-bold text-entropy-text-primary flex items-center space-x-2"><LucideEyeOff size={18} class="text-entropy-primary" /><span>Typing Indicators</span></h3>
                        <p class="text-xs text-entropy-text-secondary leading-relaxed">Share if you are currently typing. Stealth mode hides this from peers.</p>
                        <div class="flex bg-entropy-surface-light p-1 rounded-xl mt-3">
                            <button onclick={() => updatePrivacy({ typingStatus: 'everyone' })} class="flex-1 py-2 text-[10px] font-bold rounded-lg transition {$userStore.privacySettings.typingStatus === 'everyone' ? 'bg-entropy-surface shadow-sm text-entropy-primary' : 'text-entropy-text-dim'}">EVERYONE</button>
                            <button onclick={() => updatePrivacy({ typingStatus: 'nobody' })} class="flex-1 py-2 text-[10px] font-bold rounded-lg transition {$userStore.privacySettings.typingStatus === 'nobody' ? 'bg-entropy-surface shadow-sm text-entropy-primary' : 'text-entropy-text-dim'}">NOBODY</button>
                        </div>
                    </div>
                    <div class="space-y-1">
                        <h3 class="font-bold text-entropy-text-primary flex items-center space-x-2"><LucideGlobe size={18} class="text-entropy-primary" /><span>Network Routing</span></h3>
                        <p class="text-xs text-entropy-text-secondary leading-relaxed">Route your traffic to hide your IP address. (Tor requires a local Tor instance on port 9050).</p>
                         <div class="flex bg-entropy-surface-light p-1 rounded-xl mt-3">
                            <button onclick={() => { updatePrivacy({ routingMode: 'direct' }); network.reconnect(); }} class="flex-1 py-1.5 text-[9px] font-bold rounded-lg transition {$userStore.privacySettings.routingMode === 'direct' ? 'bg-entropy-surface shadow-sm text-entropy-primary' : 'text-entropy-text-dim'}">DIRECT</button>
                            <button onclick={() => { updatePrivacy({ routingMode: 'tor' }); network.reconnect(); }} class="flex-1 py-1.5 text-[9px] font-bold rounded-lg transition {$userStore.privacySettings.routingMode === 'tor' ? 'bg-entropy-surface shadow-sm text-entropy-primary' : 'text-entropy-text-dim'}">TOR</button>
                            <button onclick={async () => {
                                const url = await showPrompt("Enter SOCKS5 Proxy URL:", $userStore.privacySettings.proxyUrl || "", "Custom Proxy");
                                if (url) { updatePrivacy({ routingMode: 'custom', proxyUrl: url }); network.reconnect(); }
                            }} class="flex-1 py-1.5 text-[9px] font-bold rounded-lg transition {$userStore.privacySettings.routingMode === 'custom' ? 'bg-entropy-surface shadow-sm text-entropy-primary' : 'text-entropy-text-dim'}">CUSTOM</button>
                        </div>
                    </div>

                    <div class="space-y-1">
                        <h3 class="font-bold text-entropy-text-primary flex items-center space-x-2">
                            {#if ($userStore.privacySettings.theme || 'dark') === 'dark'}<LucideMoon size={18} class="text-entropy-primary" />{:else}<LucideSun size={18} class="text-entropy-primary" />{/if}
                            <span>Appearance</span>
                        </h3>
                        <div class="flex bg-entropy-surface-light p-1 rounded-xl mt-3">
                            <button onclick={() => updatePrivacy({ theme: 'light' })} class="flex-1 py-2 text-[10px] font-bold rounded-lg transition {($userStore.privacySettings.theme || 'dark') === 'light' ? 'bg-entropy-surface shadow-sm text-entropy-primary' : 'text-entropy-text-dim'}">LIGHT</button>
                            <button onclick={() => updatePrivacy({ theme: 'dark' })} class="flex-1 py-2 text-[10px] font-bold rounded-lg transition {($userStore.privacySettings.theme || 'dark') === 'dark' ? 'bg-entropy-surface shadow-sm text-entropy-primary' : 'text-entropy-text-dim'}">DARK</button>
                        </div>
                    </div>

                    <div class="p-4 bg-entropy-primary/5 rounded-2xl flex items-start space-x-3">
                        <img src="/logo.png" alt="logo" class="w-8 h-8 object-contain shrink-0 opacity-40 ml-[-4px]" />
                        <div><div class="text-[11px] font-bold text-entropy-text-primary uppercase tracking-widest mb-1">E2E Integrity</div><p class="text-[10px] text-entropy-text-secondary leading-snug">All privacy signals are encrypted.</p></div>
                    </div>

                    <div class="pt-4 space-y-3">
                         <div class="text-[10px] font-bold text-red-500 uppercase tracking-widest mb-3 flex items-center space-x-1"><LucideShieldAlert size={12} /><span>Danger Zone</span></div>

                        <div class="p-3 bg-entropy-surface-light rounded-2xl space-y-2">
                            <div class="text-[10px] font-bold text-red-500 uppercase tracking-wider">Panic Mode</div>
                            <p class="text-[10px] text-entropy-text-secondary leading-snug">Set a fake password that, when entered at login, silently destroys all data.</p>
                            <button 
                                onclick={async () => {
                                    const p1 = await showPrompt("Set a PANIC password (entering this at login WIPES account):", "", "Panic Password");
                                    if (!p1) return;
                                    const p2 = await showPrompt("Confirm PANIC password:", "", "Confirm Panic");
                                    if (p1 !== p2) { addToast("Passwords mismatch.", 'error'); return; }
                                    if (await showConfirm(`Are you sure?`, "Confirm Panic")) {
                                        try { await invoke('set_panic_password', { password: p1 }); addToast("Panic set", 'success'); } catch (e) { addToast("Error", 'error'); }
                                    }
                                }}
                                class="w-full py-2 bg-red-500/10 text-red-500 rounded-lg text-xs font-bold hover:bg-red-500 hover:text-white transition-all transform active:scale-[0.98]"
                            >Set Panic Password</button>
                        </div>

                         <button 
                            onclick={() => burnAccount($userStore.relayUrl)}
                            class="w-full py-3 bg-red-500/10 text-red-500 rounded-xl text-xs font-bold hover:bg-red-500 hover:text-white transition-all transform active:scale-[0.98] flex items-center justify-center space-x-2"
                         ><LucideTrash2 size={14} /><span>Nuke Account (Forensic Burn)</span></button>
                    </div>
                </div>
        {:else}
            <div class="space-y-4">
                <h3 class="text-xs font-bold text-entropy-text-dim uppercase tracking-widest">Blocked Identity Hashes</h3>
                {#if $userStore.blockedHashes.length === 0}
                    <div class="text-center py-12 space-y-2 opacity-30"><LucideBan size={40} class="mx-auto" /><p class="text-sm font-medium">No blocked contacts</p></div>
                {:else}
                    <div class="space-y-2">
                        {#each $userStore.blockedHashes as h}
                            <div class="flex items-center justify-between p-3 bg-entropy-surface-light rounded-xl">
                                <span class="text-[10px] font-mono font-bold text-entropy-text-primary">{h.slice(0, 32)}...</span>
                                <button onclick={() => toggleBlock(h)} class="text-[10px] font-black text-red-500 uppercase hover:underline">Unblock</button>
                            </div>
                        {/each}
                    </div>
                {/if}
            </div>
        {/if}
    </div>
</div>
