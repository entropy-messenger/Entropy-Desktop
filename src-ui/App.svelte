<script lang="ts">
  import { onMount } from 'svelte';
  import { userStore } from './lib/stores/user';
  import { createIdentity, initApp } from './lib/actions/auth';
  import { hasVault } from './lib/persistence';
  import { network } from './lib/network';
  import Sidebar from './components/Sidebar.svelte';
  import ChatWindow from './components/ChatWindow.svelte';
  import TitleBar from './components/TitleBar.svelte';
  import Onboarding from './components/Onboarding.svelte';
  import { LucideShieldCheck, LucideLock, LucideUnlock, LucideEye, LucideEyeOff } from 'lucide-svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { isPermissionGranted, requestPermission } from '@tauri-apps/plugin-notification';
  import { signalManager } from './lib/signal_manager';
  import Toast from './components/Toast.svelte';
  import Modal from './components/Modal.svelte';
  import Lightbox from './components/Lightbox.svelte';
  import { addToast, showConfirm, contextMenu } from './lib/stores/ui';
  import { LucideDownload } from 'lucide-svelte';
  import { exportVault, importVault, resetDatabase as resetAccountAction } from './lib/actions/vault';
  import { getVersion } from '@tauri-apps/api/app';

  let updateAvailable = $state<string | null>(null);

  let currentUpdate = $state<any>(null);
  let updatePercent = $state(0);
  let isMobile = $state(false);

  const updateDimensions = () => {
    isMobile = window.innerWidth < 1024;
  };

  const checkNativeUpdate = async () => {
    if (!(window as any).__TAURI_INTERNALS__) return;
    try {
      const { check } = await import('@tauri-apps/plugin-updater');
      const update = await check();
      
      if (update && update.available) {
        updateAvailable = update.version;
        currentUpdate = update;
      }
    } catch (e) {
    }
  };

  const handleUpdateClick = async () => {
      if (!currentUpdate || isUpdating) return;
      if (await showConfirm(`Download and install Entropy v${currentUpdate.version} now?`, "System Update")) {
          isUpdating = true;
          try {
              let downloaded = 0;
              let total = 0;
              let lastPercent = -1;

              await currentUpdate.downloadAndInstall((event: any) => {
                  switch (event.event) {
                      case 'Started':
                          total = event.data.contentLength || 0;
                          break;
                      case 'Progress':
                          downloaded += event.data.chunkLength;
                          if (total > 0) {
                              updatePercent = Math.round((downloaded / total) * 100);
                          }
                          break;
                      case 'Finished':
                          addToast("Update complete! Application will now restart.", "success");
                          break;
                  }
              });
          } catch (e) {
              isUpdating = false;
              addToast("Update failed: " + e, "error");
          }
      }
  };

  
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
  let password = $state("");
  let confirmPassword = $state("");
  let showPassword = $state(false);
  let isInitializing = $state(true);
  let hasExistingIdentity = $state(false);
  let isUpdating = $state(false);
  let showStarredMessages = $state(false);
  let showOnboarding = $state(false);


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
    if ((window as any).__TAURI_INTERNALS__) {
        await new Promise(r => setTimeout(r, 100));
        
        checkNativeUpdate();
    }
    
    isInitializing = false;
    hasExistingIdentity = await hasVault();

    if ((window as any).__TAURI_INTERNALS__) {
      try {
        await checkNotificationPermission();
      } catch (e) {
      }
    }

    updateDimensions();
    window.addEventListener('resize', updateDimensions);
    return () => window.removeEventListener('resize', updateDimensions);
  });

  const handleLogin = async () => {
    if (!password) return;
    
    userStore.update(s => ({ ...s, authError: null }));
    
    if (password.length < 4) {
        userStore.update(s => ({ ...s, authError: "Password must be at least 4 digits." }));
        return;
    }
    isInitializing = true;
    try {
        await initApp(password);
    } catch (e: any) {
        userStore.update(s => ({ ...s, authError: e.toString() }));
    } finally {
        isInitializing = false;
    }
  };

  const handleCreate = async () => {
    if (password.length < 4) {
        addToast("Password must be at least 4 digits", 'error');
        return;
    }
    isInitializing = true;
    try {
        await createIdentity(password);
        network.connect();
        showOnboarding = true;
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
            // Prevent opening dev tools in production mode
            if (!import.meta.env.DEV) e.preventDefault();
        }

        if (e.key === 'Escape') {
            userStore.update(s => ({ ...s, activeChatHash: null }));
        }
    }

    async function handleResetAccount() {
        if (!await showConfirm("This will PERMANENTLY delete your vault and all messages. Are you sure?", "Reset Identity")) return;
        await resetAccountAction();
    }

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
                    await exportVault(path);
                }
            } else {
                addToast("Export not supported in web mode.", 'warning');
            }
        } catch (e) {
        }
    }

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
                    await importVault(path);
                }
            } else {
                addToast("Import not supported in web mode.", 'warning');
            }
        } catch (e) {
        }
    }
</script>

<svelte:window oncontextmenu={handleContextMenu} onkeydown={handleKeydown} />

<main class="h-screen w-screen bg-entropy-bg overflow-hidden flex flex-col font-sans antialiased text-entropy-text-primary select-none">
    {#if !isMobile}
        <TitleBar />
    {/if}
    
    {#if !$userStore.identityHash}
        
        <div class="flex-1 flex items-center justify-center bg-entropy-bg relative overflow-hidden">
            
            <div class="absolute inset-0 pointer-events-none">
                <div class="absolute top-[-20%] left-[-10%] w-[60%] h-[60%] bg-entropy-primary/10 blur-[150px] rounded-full animate-pulse"></div>
                <div class="absolute bottom-[-20%] right-[-10%] w-[60%] h-[60%] bg-entropy-accent/10 blur-[150px] rounded-full animate-pulse" style="animation-delay: 2s;"></div>
            </div>

            <div class="max-w-6xl w-full mx-auto px-6 lg:px-12 py-8 lg:py-12 flex flex-col items-center justify-center animate-in fade-in duration-700 relative z-10 min-h-screen lg:min-h-[600px]">
                <div class="w-full grid grid-cols-1 lg:grid-cols-2 gap-12 lg:gap-24 items-center">
                    <!-- Right/Branding Section -->
                    <div class="space-y-8 lg:space-y-12 text-center lg:text-left order-1">
                        <div class="flex flex-col items-center lg:items-start space-y-6 lg:space-y-8">
                            <div class="w-20 h-20 lg:w-24 lg:h-24 bg-entropy-surface rounded-2xl shadow-2xl flex items-center justify-center transform -rotate-6 transition-all duration-700 hover:rotate-0 hover:scale-105 group border border-white/10">
                                <img src="/logo.png" alt="Entropy" class="w-12 h-12 lg:w-16 lg:h-16 object-contain" />
                            </div>
                            <div class="space-y-3 lg:space-y-4">
                                <h1 class="text-5xl lg:text-7xl font-black text-white tracking-tighter leading-none">Entropy</h1>
                                <p class="text-entropy-text-secondary text-lg lg:text-xl font-medium max-w-md mx-auto lg:mx-0 leading-relaxed opacity-80">
                                    {hasExistingIdentity ? 'Your secure gateway to sovereign communication.' : 'Ultimate privacy. No accounts. No metadata. Total sovereignty.'}
                                </p>
                            </div>
                        </div>

                        {#if !hasExistingIdentity && !isMobile}
                            <div class="grid grid-cols-1 md:grid-cols-2 gap-6 pt-8">
                                <div class="p-6 bg-white/[0.05] border border-white/10 rounded-2xl space-y-3">
                                     <div class="flex items-center space-x-3 text-entropy-accent">
                                         <LucideLock size={16} strokeWidth={3} />
                                         <span class="text-[10px] font-black uppercase tracking-[0.2em]">Zero Recovery</span>
                                     </div>
                                     <p class="text-[11px] text-white/60 leading-relaxed font-medium">
                                         This password <span class="text-entropy-accent font-black">cannot be reset</span>. If lost, your messages and identity are gone forever.
                                     </p>
                                 </div>
                                <div class="p-6 bg-white/[0.05] border border-white/10 rounded-2xl space-y-3">
                                    <div class="flex items-center space-x-3 text-entropy-primary">
                                        <LucideLock size={16} strokeWidth={3} />
                                        <span class="text-[10px] font-black uppercase tracking-[0.2em]">Local Vault</span>
                                    </div>
                                    <p class="text-[11px] text-white/60 leading-relaxed font-medium">
                                        All keys and messages are stored only in your <span class="text-entropy-primary font-black">Encrypted Database</span>.
                                    </p>
                                </div>
                            </div>
                        {/if}

                        <div class="hidden lg:block pt-12">
                            <div class="flex items-center space-x-12">
                                <button onclick={handleImportVault} class="text-[10px] font-black text-white/40 uppercase tracking-[0.3em] hover:text-white transition-colors">Restore Backup</button>
                            </div>
                        </div>
                    </div>

                    <!-- Left/Form Section -->
                    <div class="w-full space-y-8 order-2">
                        <div class="space-y-6">
                            <div class="flex justify-between items-center px-4">
                                <label for="vault-password" class="text-[11px] font-black text-white/50 uppercase tracking-[0.3em]">
                                    {hasExistingIdentity ? 'Authenticate' : 'Encrypt Identity Database'}
                                </label>
                            </div>

                            {#if $userStore.authError}
                                <div class="p-6 bg-red-500/20 rounded-2xl text-[12px] font-bold text-red-500 animate-in slide-in-from-top-4 duration-500 border border-red-500/30 flex items-center space-x-4">
                                    <div class="w-2.5 h-2.5 bg-red-500 rounded-full animate-pulse"></div>
                                    <span>{$userStore.authError}</span>
                                </div>
                            {/if}

                            <div class="space-y-4">
                                <div class="relative group">
                                    <div class="absolute left-6 lg:left-8 top-1/2 -translate-y-1/2 text-white/40 group-focus-within:text-entropy-primary transition-all scale-100 lg:scale-110">
                                        <LucideLock size={20} />
                                    </div>
                                    <input 
                                        id="vault-password"
                                        type={showPassword ? 'text' : 'password'}
                                        bind:value={password}
                                        placeholder={hasExistingIdentity ? "Enter database password" : "Define database password"} 
                                        class="w-full pl-14 lg:pl-20 pr-14 lg:pr-20 py-5 lg:py-6 bg-white/[0.06] border border-white/10 rounded-2xl focus:bg-white/[0.1] focus:border-entropy-primary/50 transition-all text-lg lg:text-xl font-mono tracking-[0.25em] outline-none text-white placeholder:text-white/20 placeholder:font-sans placeholder:tracking-normal placeholder:text-sm"
                                        onkeydown={(e) => e.key === 'Enter' && (hasExistingIdentity ? handleLogin() : null)}
                                    />
                                    <button 
                                        type="button"
                                        onclick={() => showPassword = !showPassword}
                                        class="absolute right-8 top-1/2 -translate-y-1/2 text-white/30 hover:text-white transition-colors"
                                    >
                                        {#if showPassword}
                                            <LucideEyeOff size={22} />
                                        {:else}
                                            <LucideEye size={22} />
                                        {/if}
                                    </button>
                                </div>

                                {#if !hasExistingIdentity}
                                    <div class="relative group animate-in slide-in-from-top-6 duration-700">
                                        <div class="absolute left-6 lg:left-8 top-1/2 -translate-y-1/2 text-white/40 group-focus-within:text-entropy-primary transition-all scale-100 lg:scale-110">
                                            <LucideLock size={20} />
                                        </div>
                                        <input 
                                            id="confirm-password"
                                            type={showPassword ? 'text' : 'password'}
                                            bind:value={confirmPassword}
                                            placeholder="Confirm database password" 
                                            class="w-full pl-14 lg:pl-20 pr-14 lg:pr-20 py-5 lg:py-6 bg-white/[0.06] border border-white/10 rounded-2xl focus:bg-white/[0.1] focus:border-entropy-primary/50 transition-all text-lg lg:text-xl font-mono tracking-[0.25em] outline-none text-white placeholder:text-white/20 placeholder:font-sans placeholder:tracking-normal placeholder:text-sm border-white/5"
                                            onkeydown={(e) => e.key === 'Enter' && handleCreate()}
                                        />
                                    </div>

                                    {#if password && confirmPassword && password !== confirmPassword}
                                        <div class="px-8 py-3 bg-red-500/20 rounded-xl text-[10px] font-black text-red-500 uppercase tracking-widest flex items-center space-x-3 border border-red-500/30">
                                            <div class="w-1.5 h-1.5 bg-red-500 rounded-full"></div>
                                            <span>Mismatch</span>
                                        </div>
                                    {/if}
                                {/if}
                            </div>
                        </div>

                        <div class="pt-4 flex flex-col space-y-6">
                            {#if hasExistingIdentity}
                                <button 
                                    class="w-full py-6 bg-white text-entropy-bg rounded-2xl font-black text-xs uppercase tracking-[0.4em] hover:bg-gray-100 transition-all shadow-xl active:scale-[0.98] disabled:opacity-40 flex items-center justify-center space-x-4 group"
                                    onclick={handleLogin}
                                    disabled={isInitializing || !password || password.length < 4}
                                >
                                    {#if isInitializing}
                                        <div class="w-5 h-5 border-[3px] border-entropy-bg/30 border-t-entropy-bg rounded-full animate-spin"></div>
                                        <span>Authenticated...</span>
                                    {:else}
                                        <LucideUnlock size={20} class="group-hover:scale-110 transition-transform" />
                                        <span>Unlock Database</span>
                                    {/if}
                                </button>
                            {:else}
                                <button 
                                    class="w-full py-6 bg-white text-entropy-bg rounded-2xl font-black text-xs uppercase tracking-[0.4em] hover:bg-gray-100 transition-all shadow-xl active:scale-[0.98] disabled:opacity-40 flex items-center justify-center space-x-4 group"
                                    onclick={handleCreate}
                                    disabled={isInitializing || !password || !confirmPassword || password !== confirmPassword || password.length < 4}
                                >
                                    {#if isInitializing}
                                        <div class="w-5 h-5 border-[3px] border-entropy-bg/30 border-t-entropy-bg rounded-full animate-spin"></div>
                                        <span>Mining Genesis...</span>
                                    {:else}
                                        <LucideLock size={20} class="group-hover:scale-110 transition-transform" />
                                        <span>Encrypt Local Database</span>
                                    {/if}
                                </button>
                            {/if}

                            <div class="flex items-center justify-center lg:hidden space-x-8 pt-4">
                                <button onclick={handleImportVault} class="text-[10px] font-black text-white/40 uppercase tracking-[0.2em] hover:text-white transition-colors">Restore</button>
                                <button onclick={handleResetAccount} class="text-[10px] font-black text-red-500/50 uppercase tracking-[0.2em] hover:text-red-500 transition-colors">Reset</button>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    {:else}
        <div class="flex-1 overflow-hidden bg-entropy-bg {isMobile ? 'relative' : 'flex'}">
            {#if isMobile}
                {#if !$userStore.activeChatHash}
                    <Sidebar 
                        bind:showStarredMessages
                        onUpdateClick={handleUpdateClick} 
                        {updateAvailable}
                        {isUpdating}
                        {updatePercent}
                        {isMobile}
                    />
                {:else}
                    <ChatWindow bind:showStarredMessages onCloseStarred={() => showStarredMessages = false} {isMobile} />
                {/if}
            {:else}
                <Sidebar 
                    bind:showStarredMessages
                    onUpdateClick={handleUpdateClick} 
                    {updateAvailable}
                    {isUpdating}
                    {updatePercent}
                    {isMobile}
                />
                <div class="flex-1 relative flex flex-col min-w-0">
                    <ChatWindow bind:showStarredMessages onCloseStarred={() => showStarredMessages = false} {isMobile} />
                </div>
            {/if}
        </div>
    {/if}

    <Toast />
    <Modal />
    <Lightbox />
    {#if showOnboarding}
        <Onboarding {isMobile} onComplete={() => showOnboarding = false} />
    {/if}

    {#if $contextMenu && $contextMenu.visible}
        <div
            class="fixed inset-0 z-[9998]"
            onclick={() => contextMenu.set(null)}
            oncontextmenu={(e) => { e.preventDefault(); contextMenu.set(null); }}
        ></div>
        <div
            class="fixed z-[9999] min-w-[170px] bg-entropy-surface/95 backdrop-blur-xl border border-white/10 rounded-xl shadow-2xl overflow-hidden py-1 animate-in fade-in zoom-in-95 duration-100"
            style="left: {$contextMenu.x}px; top: {$contextMenu.y}px;"
            onclick={(e) => e.stopPropagation()}
        >
            <div class="px-3 py-1.5 border-b border-white/5 mb-1 bg-white/5">
                <p class="text-[9px] font-black text-entropy-text-dim uppercase tracking-[0.2em] truncate">File Options</p>
            </div>
            <button
                onclick={() => { $contextMenu?.onSave(); contextMenu.set(null); }}
                class="w-full flex items-center space-x-3 px-3 py-2.5 text-left text-[12px] font-bold text-entropy-text-primary hover:bg-entropy-primary hover:text-white transition-all group"
            >
                <LucideDownload size={15} class="text-entropy-primary group-hover:text-white transition-colors" />
                <span>{$contextMenu?.label || 'Save to Device'}</span>
            </button>
        </div>
    {/if}
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


</style>
