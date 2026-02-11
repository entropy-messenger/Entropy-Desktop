
<script lang="ts">
  import { onMount } from 'svelte';
  import { userStore } from './lib/stores/user';
  import { createIdentity, initApp } from './lib/store';
  import { network } from './lib/network';
  import Sidebar from './components/Sidebar.svelte';
  import ChatWindow from './components/ChatWindow.svelte';
  import TitleBar from './components/TitleBar.svelte';
  import { LucideWifiOff, LucideShieldCheck, LucideLock, LucideFingerprint, LucideEye, LucideEyeOff } from 'lucide-svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { isPermissionGranted, requestPermission } from '@tauri-apps/plugin-notification';
  import { hasVault } from './lib/secure_storage';
  import { signalManager } from './lib/signal_manager';
  
  $effect(() => {
    const isDark = $userStore.privacySettings.theme === 'dark';
    if (isDark) {
      document.documentElement.classList.add('theme-dark');
    } else {
      document.documentElement.classList.remove('theme-dark');
    }
  });
  
  /**
   * Main application entry point.
   * Manages the high-level application state, identity bootstrapping, and global UI overlays.
   */
  import Toast from './components/Toast.svelte';
  import Modal from './components/Modal.svelte';
  import { addToast, showConfirm } from './lib/stores/ui';

  let password = $state("");
  let confirmPassword = $state("");
  let showPassword = $state(false);
  let isInitializing = $state(true);
  let hasExistingIdentity = $state(false);
  let showStarredMessages = $state(false);


  /**
   * Orchestrates OS-level notification permissions required for incoming message alerts.
   */
  const checkNotificationPermission = async () => {
    let permission = await isPermissionGranted();
    if (!permission) {
      permission = await requestPermission() === 'granted';
    }
  };

  onMount(async () => {
    if (window.__TAURI_INTERNALS__) {
        await new Promise(r => setTimeout(r, 100));
    }
    
    isInitializing = false;
    hasExistingIdentity = await hasVault();

    if (window.__TAURI_INTERNALS__) {
      try {
        await checkNotificationPermission();
      } catch (e) {
        console.error("Tauri permission failed:", e);
      }
    }
  });

  const handleLogin = async () => {
    if (!password) return;
    
    userStore.update(s => ({ ...s, authError: null }));
    
    isInitializing = true;
    try {
        await initApp(password);
    } catch (e: any) {
        console.error("Login component caught error:", e);
        userStore.update(s => ({ ...s, authError: e.toString() }));
    } finally {
        isInitializing = false;
    }
  };

  const handleCreate = async () => {
    if (!password) return;
    isInitializing = true;
    try {
        await createIdentity(password);
        network.connect();
    } catch (e: any) {
        addToast("Creation failed: " + (e.message || e), 'error');
    } finally {
        isInitializing = false;
    }
  };

  function handleContextMenu(e: MouseEvent) {
      if (import.meta.env.DEV) return;
      e.preventDefault();
  }

    function handleKeydown(e: KeyboardEvent) {
        
        if (
            e.key === 'F12' ||
            (e.ctrlKey && e.shiftKey && (e.key === 'I' || e.key === 'J' || e.key === 'C'))
        ) {
            // Let dev tools open in dev mode? 
            if (!import.meta.env.DEV) e.preventDefault();
        }

        if (e.key === 'Escape') {
            userStore.update(s => ({ ...s, activeChatHash: null }));
        }
    }

    /**
     * Executes a destructive account wipe across both local storage and the native database.
     */
    async function handleNuclearReset() {
        if (!await showConfirm("This will PERMANENTLY delete your vault and all messages. Are you sure?", "Nuclear Reset")) return;
        try {
            localStorage.clear();
            addToast("Vault wiped. Restarting Entropy...", 'info');
            // Allow toast to render
            await new Promise(r => setTimeout(r, 2000));
            await invoke('nuclear_reset');
        } catch (e) {
            console.error("Reset failed:", e);
            addToast("Reset failed: " + e, 'error');
        }
    }

    /**
     * Serializes the current vault to a file on the host filesystem.
     */
    async function handleExportVault() {
         try {
            if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
                const { save } = await import('@tauri-apps/plugin-dialog');
                const path = await save({
                    defaultPath: `entropy_backup_${Date.now()}.entropy`,
                    filters: [{
                        name: 'Entropy Backup',
                        extensions: ['entropy']
                    }]
                });

                if (path) {
                    await invoke('export_database', { targetPath: path });
                    addToast("Backup exported successfully!", 'success');
                }
            } else {
                addToast("Export not supported in web mode.", 'warning');
            }
        } catch (e) {
            console.error("Export failed:", e);
            addToast("Export failed: " + e, 'error');
        }
    }

    /**
     * Restores the application state from an external backup file.
     */
    async function handleImportVault() {
        if (!await showConfirm("WARNING: Importing a backup will OVERWRITE any current data on this device. Continue?", "Restore Backup")) return;

        try {
            if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
                const { open } = await import('@tauri-apps/plugin-dialog');
                const path = await open({
                    multiple: false,
                    filters: [{
                        name: 'Entropy Backup',
                        extensions: ['entropy', 'zip']
                    }]
                });

                if (path) {
                    await invoke('import_database', { srcPath: path });
                    addToast("Backup restored! The app will now reload.", 'success');
                    setTimeout(() => window.location.reload(), 2000);
                }
            } else {
                addToast("Import not supported in web mode.", 'warning');
            }
        } catch (e) {
            console.error("Import failed:", e);
            addToast("Import failed: " + e, 'error');
        }
    }
</script>

<svelte:window oncontextmenu={handleContextMenu} onkeydown={handleKeydown} />

<main class="h-screen w-screen bg-entropy-bg overflow-hidden flex flex-col font-sans antialiased text-entropy-text-primary select-none">
    
    {#if !$userStore.identityHash}
        
        <div class="flex-1 flex items-center justify-center bg-entropy-bg relative overflow-hidden">
            
            <div class="absolute inset-0 pointer-events-none">
                <div class="absolute top-[-20%] left-[-10%] w-[60%] h-[60%] bg-entropy-primary/10 blur-[150px] rounded-full animate-pulse"></div>
                <div class="absolute bottom-[-20%] right-[-10%] w-[60%] h-[60%] bg-entropy-accent/10 blur-[150px] rounded-full animate-pulse" style="animation-delay: 2s;"></div>
                <div class="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[30%] h-[30%] bg-entropy-primary/5 blur-[100px] rounded-full"></div>
            </div>

            <div class="bg-entropy-surface/80 backdrop-blur-xl rounded-[3rem] shadow-[0_40px_100px_-20px_rgba(0,0,0,0.5)] w-[440px] text-center overflow-hidden animate-in zoom-in-95 duration-700 relative z-10">
                <TitleBar />
                <div class="p-12 space-y-10">
                    <div class="relative inline-block">
                        <div class="w-20 h-20 bg-entropy-surface rounded-2xl shadow-xl flex items-center justify-center mx-auto transform -rotate-6 transition-all duration-700 hover:rotate-0 hover:scale-105 group">
                            <img src="/logo.png" alt="Entropy" class="w-16 h-16 object-contain transition-transform duration-500 group-hover:scale-110" />
                        </div>
                        <div class="absolute -top-1.5 -right-1.5 bg-entropy-primary text-white p-1.5 rounded-xl shadow-lg">
                            <LucideShieldCheck size={14} />
                        </div>
                    </div>
                    
                    <div class="space-y-3">
                        <h1 class="text-4xl font-black text-entropy-text-primary tracking-tight">Entropy</h1>
                        <p class="text-entropy-text-secondary text-sm leading-relaxed max-w-[280px] mx-auto font-medium opacity-80">
                            {hasExistingIdentity ? 'Welcome back.' : 'Secure messaging. No sign-up required.'}
                        </p>
                    </div>
                    
                    <div class="space-y-6 text-left">
                        <div class="space-y-2.5">
                            <div class="flex justify-between items-center px-1">
                                <label for="vault-password" class="text-[11px] font-bold text-entropy-text-dim uppercase tracking-wider pl-1">Login</label>
                                {#if hasExistingIdentity}
                                    <span class="text-[10px] font-bold text-entropy-primary uppercase tracking-wider">Identity Found</span>
                                {/if}
                            </div>
                            
                            {#if $userStore.authError}
                                <div class="p-4 bg-red-500/10 rounded-2xl text-[11px] font-bold text-red-500 animate-in fade-in slide-in-from-top-2 flex items-center space-x-2">
                                    <div class="w-1.5 h-1.5 bg-red-500 rounded-full animate-pulse"></div>
                                    <span>{$userStore.authError}</span>
                                </div>
                            {/if}

                            <div class="relative group">
                                <div class="absolute left-5 top-1/2 -translate-y-1/2 text-entropy-text-dim group-focus-within:text-entropy-primary transition-colors">
                                    <LucideLock size={18} />
                                </div>
                                <input 
                                    id="vault-password"
                                    type={showPassword ? 'text' : 'password'}
                                    bind:value={password}
                                    placeholder="Enter password..." 
                                    class="w-full pl-14 pr-14 py-5 bg-entropy-surface-light/50 rounded-[1.5rem] focus:bg-entropy-surface focus:ring-4 focus:ring-entropy-primary/5 transition-all text-lg font-mono tracking-wider outline-none text-entropy-text-primary"
                                    onkeydown={(e) => e.key === 'Enter' && (hasExistingIdentity ? handleLogin() : null)}
                                />
                                <button 
                                    type="button"
                                    onclick={() => showPassword = !showPassword}
                                    class="absolute right-5 top-1/2 -translate-y-1/2 text-entropy-text-dim hover:text-entropy-primary transition-colors"
                                    aria-label="Toggle password visibility"
                                >
                                    {#if showPassword}
                                        <LucideEyeOff size={18} />
                                    {:else}
                                        <LucideEye size={18} />
                                    {/if}
                                </button>
                            </div>
                            
                            {#if !hasExistingIdentity}
                                <div class="relative group">
                                    <div class="absolute left-5 top-1/2 -translate-y-1/2 text-entropy-text-dim group-focus-within:text-entropy-primary transition-colors">
                                        <LucideLock size={18} />
                                    </div>
                                    <input 
                                        id="confirm-password"
                                        type={showPassword ? 'text' : 'password'}
                                        bind:value={confirmPassword}
                                        placeholder="Confirm password..." 
                                        class="w-full pl-14 pr-14 py-5 bg-entropy-surface-light/50 rounded-[1.5rem] focus:bg-entropy-surface focus:ring-4 focus:ring-entropy-primary/5 transition-all text-lg font-mono tracking-wider outline-none text-entropy-text-primary"
                                        onkeydown={(e) => e.key === 'Enter' && handleCreate()}
                                    />
                                    <button 
                                        type="button"
                                        onclick={() => showPassword = !showPassword}
                                        class="absolute right-5 top-1/2 -translate-y-1/2 text-entropy-text-dim hover:text-entropy-primary transition-colors"
                                        aria-label="Toggle password visibility"
                                    >
                                        {#if showPassword}
                                            <LucideEyeOff size={18} />
                                        {:else}
                                            <LucideEye size={18} />
                                        {/if}
                                    </button>
                                </div>
                                
                                {#if password && confirmPassword && password !== confirmPassword}
                                    <div class="p-3 bg-yellow-500/10 rounded-xl text-[10px] font-bold text-yellow-600 animate-in fade-in slide-in-from-top-2">
                                        Passwords don't match
                                    </div>
                                {/if}
                            {/if}
                        </div>

                        {#if hasExistingIdentity}
                            <button 
                                class="w-full py-5 bg-entropy-primary text-white rounded-[1.5rem] font-bold text-sm uppercase tracking-wider hover:bg-entropy-primary-dim transition-all shadow-xl shadow-entropy-primary/10 active:scale-[0.98] disabled:opacity-50 flex items-center justify-center space-x-3 overflow-hidden group"
                                onclick={handleLogin}
                                disabled={isInitializing || !password}
                                aria-label="Unlock Identity"
                            >
                                {#if isInitializing}
                                    <div class="w-5 h-5 border-2 border-white/30 border-t-white rounded-full animate-spin"></div>
                                    <span>Unlocking...</span>
                                {:else}
                                    <LucideFingerprint size={20} class="group-hover:scale-110 transition-transform" />
                                    <span>Enter</span>
                                {/if}
                            </button>
                        {:else}
                            <button 
                                class="w-full py-5 bg-white text-entropy-bg rounded-[1.5rem] font-bold text-sm uppercase tracking-wider hover:bg-white/90 transition-all shadow-2xl active:scale-[0.98] disabled:opacity-50 flex items-center justify-center space-x-3 overflow-hidden group"
                                onclick={handleCreate}
                                disabled={isInitializing || !password || !confirmPassword || password !== confirmPassword}
                                aria-label="Create Identity"
                            >
                                {#if isInitializing}
                                    <div class="w-5 h-5 border-2 border-entropy-bg/30 border-t-entropy-bg rounded-full animate-spin"></div>
                                    <span>Creating...</span>
                                {:else}
                                    <LucideShieldCheck size={20} class="group-hover:scale-110 transition-transform" />
                                    <span>Create Identity</span>
                                {/if}
                            </button>
                        {/if}
                    </div>

                    <div class="pt-2">
                        
                        <div class="flex items-center justify-center space-x-6">
                            {#if hasExistingIdentity}
                                <button 
                                    onclick={handleExportVault}
                                    class="text-[10px] font-bold text-entropy-primary uppercase tracking-wider hover:text-entropy-primary-dim transition-colors"
                                >
                                    Backup
                                </button>
                            {/if}
                            <button 
                                onclick={handleImportVault}
                                    class="text-[10px] font-bold text-entropy-accent uppercase tracking-wider hover:text-entropy-accent/80 transition-colors"
                            >
                                Restore
                            </button>
                            {#if import.meta.env.DEV || $userStore.authError}
                                <button 
                                    onclick={handleNuclearReset}
                                    class="text-[10px] font-bold text-red-500 uppercase tracking-wider hover:text-red-600 transition-colors"
                                >
                                    Wipe
                                </button>
                            {/if}
                        </div>
                    </div>
                </div>
            </div>
        </div>
    {:else}
        
        <TitleBar />
        <div class="flex flex-row flex-1 overflow-hidden bg-entropy-bg">
            <Sidebar bind:showStarredMessages />
            <div class="flex-1 relative flex flex-col min-w-0">
                <ChatWindow {showStarredMessages} onCloseStarred={() => showStarredMessages = false} />
            
            </div>
        </div>
    {/if}

    <Toast />
    <Modal />
</main>

<style>
    :global(body) {
        background-color: transparent;
        margin: 0;
        padding: 0;
        user-select: none;
        cursor: default;
        -webkit-user-select: none;
        overflow: hidden;
    }

    @keyframes progress {
        0% { width: 0%; left: 0%; }
        50% { width: 100%; left: 0%; }
        100% { width: 0%; left: 100%; }
    }

    .animate-progress {
        animation: progress 2s infinite ease-in-out;
    }
</style>
