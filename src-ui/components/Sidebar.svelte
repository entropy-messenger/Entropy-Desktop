
<script lang="ts">
  import { userStore } from '../lib/stores/user';
  import { 
    startChat, createGroup, updateMyProfile, 
    togglePin, toggleArchive, toggleMute, toggleBlock, updatePrivacy,
    registerGlobalNickname, lookupNickname, burnAccount, refreshDecoys
  } from '../lib/store';
  import {
    LucidePlus, LucideSettings, LucideSearch,
    LucideCheck, LucideCheckCheck, LucideUsers, LucideX,
    LucideCamera, LucideUser, LucidePin, LucideArchive, LucideBellOff,
    LucideLock, LucideCheckCircle2, LucideBan, LucideEyeOff,
    LucideShieldAlert, LucideCpu, LucideGlobe, LucideTrash2,
    LucideWifiOff, LucideEye, LucideSun, LucideMoon, LucideStar
  } from 'lucide-svelte';
  import ImageEditor from './ImageEditor.svelte';
  import { playingVoiceNoteId } from '../lib/stores/audio';
  import { invoke } from '@tauri-apps/api/core';
  import { addToast, showConfirm, showPrompt } from '../lib/stores/ui';
  import { network } from '../lib/network';
  import QRCode from 'qrcode';

  /**
   * Primary navigation and configuration hub.
   * Orchestrates peer discovery, group lifecycle, and identity management.
   */

  const exportVault = async () => {
    addToast("Starting export...", 'info'); 
    try {
        if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
            const { save } = await import('@tauri-apps/plugin-dialog');
            const path = await save({
                defaultPath: `entropy_backup_${Date.now()}.entropy`,
                filters: [{
                    name: 'Entropy Backup',
                    extensions: ['entropy', 'zip']
                }]
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
        console.error("Export failed:", e);
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
  };

  let activeHash = $state<string | null>(null);
  let searchQuery = $state("");
  let showCreateGroup = $state(false);
  let groupName = $state("");
  let groupMembers = $state<string[]>([]);
  let memberInput = $state("");
  let pfpInput = $state<HTMLInputElement | null>(null);
  let editingImage = $state<string | null>(null);
  let viewingImage = $state<string | null>(null);
  let qrCodeUrl = $state<string>("");
  let filter = $state<'all' | 'archived'>('all');
  let isRegisteringNickname = $state(false);
  
  let { showStarredMessages = $bindable(false) } = $props();

  let starredMessages = $derived(Object.values($userStore.chats).flatMap(chat => 
    chat.messages.filter(m => m.isStarred).map(m => ({
        ...m,
        peerHash: chat.peerHash,
        peerAlias: chat.localNickname || chat.peerAlias || chat.peerHash.slice(0, 8)
    }))
  ).sort((a, b) => b.timestamp - a.timestamp));
  
  userStore.subscribe(store => {
    activeHash = store.activeChatHash;
  });

  const selectChat = (hash: string) => {
    userStore.update(s => {
        if (s.chats[hash]) {
            s.chats[hash] = { 
                ...s.chats[hash], 
                unreadCount: 0 
            };
        }
        return { ...s, activeChatHash: hash, chats: { ...s.chats } };
    });
    startChat(hash);
  };

  /**
   * Initiates a new peer-to-peer conversation by resolving a 64-character hash or a global nickname.
   */
  const createChatPrompt = async () => {
    let input = await showPrompt("Enter Peer ID Hash (64-char Hex) or Global Nickname:", "", "New Chat");
    if (!input) return;
    input = input.trim();

    if (input.length === 64 && /^[0-9a-fA-F]+$/.test(input)) {
        startChat(input);
    } else {
        const hash = await lookupNickname(input);
        if (hash) {
            startChat(hash, input);
        } else {
            addToast("Could not find user with that hash or nickname.", 'error');
        }
    }
  };

  $effect(() => {
    if ($userStore.identityHash) {
        const isLight = ($userStore.privacySettings.theme || 'dark') === 'light';
        QRCode.toDataURL($userStore.identityHash, {
            margin: 2,
            width: 300,
            color: {
                dark: isLight ? '#0f172a' : '#08080a',
                light: isLight ? '#ffffff' : '#ffffff'
            }
        }).then(url => {
            qrCodeUrl = url;
        }).catch(err => {
            console.error("QR generation failed:", err);
        });
    }
  });

  const handleCreateGroup = () => {
      if (!groupName.trim() || groupMembers.length === 0) return;
      createGroup(groupName, groupMembers);
      groupName = "";
      groupMembers = [];
      showCreateGroup = false;
  };

  const addMember = async () => {
      const input = memberInput.trim();
      if (!input) return;

      let targetHash = "";
      if (input.length === 64 && /^[0-9a-fA-F]+$/.test(input)) {
          targetHash = input;
      } else {
          const localMatch = Object.values($userStore.chats).find(c => c.localNickname?.toLowerCase() === input.toLowerCase());
          if (localMatch) {
              targetHash = localMatch.peerHash;
          } else {
              const globalHash = await lookupNickname(input);
              if (globalHash) {
                targetHash = globalHash;
                userStore.update(s => {
                    if (s.chats[targetHash]) s.chats[targetHash].peerAlias = input;
                    return s;
                });
            }
          }
      }

      if (targetHash && !groupMembers.includes(targetHash)) {
          groupMembers = [...groupMembers, targetHash];
          memberInput = "";
      } else if (!targetHash) {
          addToast("Could not resolve nickname or hash.", 'error');
      }
  };

  const toggleMember = (hash: string) => {
      if (groupMembers.includes(hash)) {
          groupMembers = groupMembers.filter(m => m !== hash);
      } else {
          groupMembers = [...groupMembers, hash];
      }
  };

  const removeMember = (m: string) => {
      groupMembers = groupMembers.filter(member => member !== m);
  };

  let showSettings = $state(false);
  let settingsTab = $state<'profile' | 'privacy' | 'blocked'>('profile');
  let copied = $state(false);
  
    const toggleSettings = () => { 
        showSettings = !showSettings; 
        settingsTab = 'profile';
        copied = false; 
    };

    const toggleTheme = () => {
        const current = $userStore.privacySettings.theme || 'dark';
        updatePrivacy({ theme: current === 'dark' ? 'light' : 'dark' });
    };

  const copyHash = () => {
    if ($userStore.identityHash) {
        navigator.clipboard.writeText($userStore.identityHash);
        copied = true;
        setTimeout(() => copied = false, 2000);
    }
  };

   const onPfpSelect = (e: Event) => {
      const file = (e.target as HTMLInputElement).files?.[0];
      if (file) {
          const reader = new FileReader();
          reader.onload = (ev) => {
              editingImage = ev.target?.result as string;
          };
          reader.readAsDataURL(file);
      }
  };

  const handleSavePfp = (cropped: string) => {
      updateMyProfile($userStore.myAlias || "Anonymous", cropped);
      editingImage = null;
      addToast("Profile picture updated!", 'success');
  };

  const handleUpdateAlias = async () => {
      const next = await showPrompt("Update your display name:", $userStore.myAlias || "", "Display Name");
      if (next !== null) {
          updateMyProfile(next.trim() || "Anonymous", $userStore.myPfp);
      }
  };

  let filteredChats = $derived(Object.values($userStore.chats).filter(chat => {
    const query = searchQuery.toLowerCase();
    const chatName = (chat.localNickname || chat.peerAlias || "").toLowerCase();
    const matchesName = chatName.includes(query) || chat.peerHash.toLowerCase().includes(query);
    const matchesMessages = chat.messages.some(m => m.content.toLowerCase().includes(query));
    
    if (filter === 'archived' && !chat.isArchived) return false;
    if (filter === 'all' && chat.isArchived) return false;

    return matchesName || matchesMessages;
  }).sort((a, b) => {
    if (a.isPinned && !b.isPinned) return -1;
    if (!a.isPinned && b.isPinned) return 1;
    
    const aTime = a.messages.length > 0 ? a.messages[a.messages.length - 1].timestamp : 0;
    const bTime = b.messages.length > 0 ? b.messages[b.messages.length - 1].timestamp : 0;
    return bTime - aTime;
  }));

  function formatLastSeen(ts?: number) {
      if (!ts) return "";
      const diff = Date.now() - ts;
      if (diff < 60000) return "just now";
      if (diff < 3600000) return `${Math.floor(diff/60000)}m ago`;
      return new Date(ts).toLocaleTimeString([], {hour: '2-digit', minute:'2-digit'});
  }
</script>

<div class="h-full w-80 bg-entropy-bg flex flex-col relative shrink-0">
  <div class="p-4 flex flex-col space-y-4 bg-entropy-surface/50">
    <div class="flex justify-end items-center px-1">
        <div class="flex items-center space-x-1">
            <button onclick={toggleTheme} class="p-2 hover:bg-entropy-surface-light rounded-full text-entropy-text-dim hover:text-entropy-primary transition" title="Toggle Theme">
                {#if ($userStore.privacySettings.theme || 'dark') === 'dark'}
                    <LucideSun size={18} />
                {:else}
                    <LucideMoon size={18} />
                {/if}
            </button>
            <button onclick={() => showStarredMessages = true} class="p-2 hover:bg-entropy-surface-light rounded-full text-entropy-text-dim hover:text-yellow-500 transition" title="Starred Messages">
                <LucideStar size={18} />
            </button>
            <button onclick={() => showCreateGroup = true} class="p-2 hover:bg-entropy-surface-light rounded-full text-entropy-text-secondary transition" title="New Group">
                <LucideUsers size={18} />
            </button>
            <button onclick={createChatPrompt} class="p-2 hover:bg-entropy-surface-light rounded-full text-entropy-primary transition" title="New Message">
                <LucidePlus size={20} />
            </button>
            <button onclick={toggleSettings} class="p-2 hover:bg-entropy-surface-light rounded-full text-entropy-text-dim transition">
                <LucideSettings size={18} />
            </button>
        </div>
    </div>

    <div class="flex bg-entropy-surface-light rounded-lg p-1 text-[10px] font-bold uppercase tracking-wider">
        <button onclick={() => filter = 'all'} class="flex-1 py-1 rounded-md transition {filter === 'all' ? 'bg-entropy-surface shadow-sm text-entropy-primary' : 'text-entropy-text-dim hover:text-entropy-text-secondary'}">All Chats</button>
        <button onclick={() => filter = 'archived'} class="flex-1 py-1 rounded-md transition {filter === 'archived' ? 'bg-entropy-surface shadow-sm text-entropy-primary' : 'text-entropy-text-dim hover:text-entropy-text-secondary'}">Archived</button>
    </div>

    <div class="relative">
        <LucideSearch size={14} class="absolute left-3 top-1/2 -translate-y-1/2 text-entropy-text-dim" />
        <input 
            type="text" 
            bind:value={searchQuery}
            placeholder="Search messages & contacts..." 
            class="w-full pl-9 pr-4 py-2 bg-entropy-surface-light focus:bg-entropy-surface border-none rounded-xl text-xs transition ring-1 ring-white/5"
        />
    </div>
    
    {#if $playingVoiceNoteId}
        <div class="px-4 py-2 bg-entropy-primary text-white flex items-center justify-between animate-in slide-in-from-left duration-300">
            <div class="flex items-center space-x-2">
                <div class="w-2 h-2 bg-white rounded-full animate-pulse"></div>
                <span class="text-[9px] font-black uppercase tracking-widest">Listening</span>
            </div>
            <button onclick={() => playingVoiceNoteId.set(null)} class="text-[8px] font-black uppercase tracking-tighter hover:underline bg-white/10 px-1.5 py-0.5 rounded">Stop</button>
        </div>
    {/if}
  </div>

  <div class="flex-1 overflow-y-auto custom-scrollbar">
    {#each filteredChats as chat (chat.peerHash)}
        <div 
            class="group/item p-4 hover:bg-entropy-surface/50 cursor-pointer transition relative {activeHash === chat.peerHash ? 'bg-entropy-primary/10 shadow-[inset_4px_0_0_0_#8b5cf6]' : ''}"
            onclick={() => selectChat(chat.peerHash)}
            onkeypress={(e) => e.key === 'Enter' && selectChat(chat.peerHash)}
            role="button"
            tabindex="0"
        >
                <div class="flex items-center space-x-3">
                    <div class="relative">
                        {#if chat.pfp}
                            <img src={chat.pfp} alt="" class="w-12 h-12 rounded-2xl object-cover shadow-sm" />
                        {:else}
                            <div class="w-12 h-12 rounded-2xl bg-gradient-to-br {chat.isGroup ? 'from-purple-500 to-indigo-600' : 'from-blue-400 to-blue-600'} flex items-center justify-center text-white font-bold text-lg shadow-sm">
                                {chat.peerAlias ? chat.peerAlias[0].toUpperCase() : '?'}
                            </div>
                        {/if}
                        {#if chat.isOnline && !chat.isGroup}
                            <div class="absolute -bottom-0.5 -right-0.5 w-3.5 h-3.5 bg-green-500 border-2 border-white rounded-full shadow-sm"></div>
                        {/if}
                    </div>
                    
                    <div class="flex-1 min-w-0">
                        <div class="flex justify-between items-baseline mb-0.5">
                            <div class="font-bold text-entropy-text-primary truncate flex items-center space-x-1">
                                {#if chat.isGroup}<LucideUsers size={12} class="text-entropy-primary" />{/if}
                                <span class="truncate">{chat.localNickname || chat.peerAlias || chat.peerHash.slice(0, 8)}</span>
                                {#if chat.isPinned}<LucidePin size={10} class="text-entropy-primary fill-entropy-primary" />{/if}
                                {#if chat.isMuted}<LucideBellOff size={10} class="text-entropy-text-dim" />{/if}
                            </div>
                            {#if chat.messages.length > 0}
                                 <div class="text-[10px] font-medium text-entropy-text-secondary shrink-0">
                                    {new Date(chat.messages[chat.messages.length - 1].timestamp).toLocaleTimeString([], {hour: '2-digit', minute:'2-digit'})}
                                 </div>
                            {/if}
                        </div>
                        
                        <div class="flex items-center justify-between mt-0.5">
                            <div class="text-[12px] truncate pr-2 flex-1 {chat.isTyping ? 'text-entropy-accent font-bold' : 'text-entropy-text-dim'}">
                                {#if chat.isTyping}
                                    <span>typing...</span>
                                {:else if chat.messages.length > 0}
                                    <div class="flex items-center space-x-1">
                                        {#if chat.messages[chat.messages.length - 1].isMine}
                                            {#if chat.messages[chat.messages.length - 1].status === 'read'}
                                                <LucideCheckCheck size={13} class="text-blue-600 dark:text-cyan-400" />
                                            {:else if chat.messages[chat.messages.length - 1].status === 'delivered'}
                                                <LucideCheckCheck size={13} class="text-entropy-text-secondary" />
                                            {:else}
                                                <LucideCheck size={13} class="text-entropy-text-secondary" />
                                            {/if}
                                        {/if}
                                        <span class="truncate">{chat.messages[chat.messages.length - 1].content}</span>
                                    </div>
                                {:else if !chat.isOnline && chat.lastSeen}
                                    <span class="text-[11px] opacity-70">last seen {formatLastSeen(chat.lastSeen)}</span>
                                {/if}
                            </div>
                            
                            {#if chat.unreadCount > 0}
                                <div class="bg-entropy-primary text-white text-[9px] font-bold px-1.5 py-0.5 rounded-full min-w-[17px] text-center shadow-sm">
                                    {chat.unreadCount}
                                </div>
                            {/if}
                            
                            <div class="hidden group-hover/item:flex items-center space-x-1 ml-2">
                                 <button onclick={(e) => {e.stopPropagation(); togglePin(chat.peerHash)}} class="p-1 hover:bg-white/10 rounded transition text-entropy-text-dim hover:text-entropy-primary" title="Pin/Unpin">
                                    <LucidePin size={12} class={chat.isPinned ? 'fill-entropy-primary text-entropy-primary' : ''} />
                                 </button>
                                 <button onclick={(e) => {e.stopPropagation(); toggleArchive(chat.peerHash)}} class="p-1 hover:bg-white/10 rounded transition text-entropy-text-dim hover:text-entropy-primary" title="Archive/Unarchive">
                                    <LucideArchive size={12} class={chat.isArchived ? 'fill-entropy-primary text-entropy-primary' : ''} />
                                 </button>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        {/each}
  </div>

  {#if showSettings}
    <div class="absolute inset-0 bg-entropy-bg z-[60] flex flex-col animate-in slide-in-from-bottom duration-300">
        <div class="p-4 flex justify-between items-center bg-entropy-surface">
            <h2 class="font-bold text-entropy-text-primary">Settings</h2>
            <button onclick={toggleSettings} class="text-entropy-text-secondary hover:text-entropy-text-primary font-bold">Done</button>
        </div>
        
        <div class="flex text-[10px] font-black uppercase tracking-widest text-entropy-text-dim">
            <button onclick={() => settingsTab = 'profile'} class="flex-1 py-3 {settingsTab === 'profile' ? 'text-entropy-primary border-b-2 border-entropy-primary' : ''}">Profile</button>
            <button onclick={() => settingsTab = 'privacy'} class="flex-1 py-3 {settingsTab === 'privacy' ? 'text-entropy-primary border-b-2 border-entropy-primary' : ''}">Privacy</button>
            <button onclick={() => settingsTab = 'blocked'} class="flex-1 py-3 {settingsTab === 'blocked' ? 'text-entropy-primary border-b-2 border-entropy-primary' : ''}">Blocked</button>
        </div>

        <div class="p-6 space-y-8 flex-1 overflow-y-auto custom-scrollbar">
            {#if settingsTab === 'profile'}
                <div class="flex flex-col items-center space-y-4">
                    <div class="relative group">
                        {#if $userStore.myPfp}
                            <button onclick={() => viewingImage = $userStore.myPfp} class="focus:outline-none" title="View Profile Picture">
                                <img src={$userStore.myPfp} alt="" class="w-24 h-24 rounded-3xl object-cover shadow-xl ring-4 ring-entropy-primary/10 hover:ring-entropy-primary/30 transition-all active:scale-95" />
                            </button>
                        {:else}
                            <div class="w-24 h-24 rounded-3xl bg-entropy-surface-light flex items-center justify-center text-entropy-primary shadow-xl">
                                <LucideUser size={48} />
                            </div>
                        {/if}
                        <button onclick={() => pfpInput?.click()} class="absolute -bottom-2 -right-2 p-2 bg-entropy-primary text-white rounded-xl shadow-lg hover:bg-entropy-primary-dim transition active:scale-95"><LucideCamera size={18} /></button>
                        <input type="file" bind:this={pfpInput} onchange={onPfpSelect} accept="image/*" class="hidden" />
                    </div>
                    <div class="text-center space-y-1">
                        <button onclick={handleUpdateAlias} class="text-xl font-bold text-entropy-text-primary hover:text-entropy-primary transition flex items-center justify-center space-x-2">
                            <span>{$userStore.myAlias || 'Set Name'}</span>
                            <LucidePlus size={16} class="opacity-50" />
                        </button>
                        <div class="text-xs font-bold text-entropy-text-dim uppercase tracking-widest">Active Identity</div>
                    </div>

                    <div class="w-full flex flex-col space-y-2">
                        <button 
                            disabled={isRegisteringNickname || $userStore.connectionStatus !== 'connected'}
                            onclick={async () => {
                                const nick = await showPrompt("Register a global nickname (min 3 chars):", $userStore.myAlias || "", "Global Nickname");
                                if (nick) {
                                    if (nick.length < 3) {
                                        addToast("Nickname too short (min 3 characters)", 'error');
                                        return;
                                    }
                                    isRegisteringNickname = true;
                                    try {
                                        const res = await registerGlobalNickname(nick);
                                        if (res && res.success) {
                                            addToast("Nickname registered successfully!", 'success');
                                        } else {
                                            addToast("Registration failed: " + (res?.error || "Unknown"), 'error');
                                        }
                                    } finally {
                                        isRegisteringNickname = false;
                                    }
                                }
                            }}
                            class="w-full py-3 bg-entropy-primary text-white rounded-xl text-sm font-bold shadow-lg hover:bg-entropy-primary-dim transition-all active:scale-95 flex flex-col items-center justify-center space-y-1 overflow-hidden disabled:opacity-50 disabled:grayscale"
                        >
                            {#if isRegisteringNickname}
                                <div class="flex items-center space-x-2 animate-pulse">
                                    <div class="w-3 h-3 border-2 border-white/20 border-t-white rounded-full animate-spin"></div>
                                    <span>Authenticating Identity...</span>
                                </div>
                                <div class="text-[8px] font-black uppercase tracking-[0.2em] opacity-60">
                                    Solving Cryptographic Puzzle
                                </div>
                                <div class="text-[7px] font-bold opacity-40 px-4 text-center leading-tight">
                                    Shorter names take longer to compute. Please wait.
                                </div>
                            {:else}
                                <div class="flex items-center space-x-2">
                                    <img src="/logo.png" alt="logo" class="w-6 h-6 object-contain invert opacity-40" />
                                    <span>Register Global Nickname</span>
                                </div>
                            {/if}
                        </button>

                        <div class="flex space-x-2">
                            <button 
                                type="button"
                                onclick={(e) => { 
                                    exportVault(); 
                                }}
                                class="flex-1 py-3 bg-entropy-surface-light text-entropy-text-secondary rounded-xl text-xs font-bold hover:bg-entropy-surface transition"
                            >
                                Export Backup
                            </button>
                            <button 
                                type="button"
                                onclick={(e) => { 
                                    importVault(); 
                                }}
                                class="flex-1 py-3 bg-entropy-surface-light text-entropy-text-secondary rounded-xl text-xs font-bold hover:bg-entropy-surface transition"
                            >
                                Import Backup
                            </button>
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
                                <img 
                                    src={qrCodeUrl} 
                                    alt="QR Identity" 
                                    class="w-32 h-32 blur-[2px] group-hover/qr:blur-0 transition-all duration-300"
                                />
                            {:else}
                                <div class="w-32 h-32 bg-gray-100 flex items-center justify-center">
                                    <div class="w-6 h-6 border-2 border-entropy-primary border-t-transparent rounded-full animate-spin"></div>
                                </div>
                            {/if}
                            <div class="absolute inset-0 bg-white/40 flex items-center justify-center opacity-100 group-hover/qr:opacity-0 transition-opacity">
                                <LucideLock size={24} class="text-blue-600" />
                            </div>
                        </div>
                    </div>
                </div>
            {:else if settingsTab === 'privacy'}
                    <div class="space-y-6">
                        <div class="space-y-1">
                            <h3 class="font-bold text-entropy-text-primary flex items-center space-x-2">
                                <LucideCheckCheck size={18} class="text-entropy-primary" />
                                <span>Read Receipts</span>
                            </h3>
                            <p class="text-xs text-entropy-text-secondary leading-relaxed">If turned off, you won't send or receive Read Receipts (blue checks). Personal privacy first.</p>
                            <div class="flex justify-end pt-2">
                                <button onclick={() => updatePrivacy({ readReceipts: !$userStore.privacySettings.readReceipts })} class="w-12 h-6 rounded-full transition-colors relative {$userStore.privacySettings.readReceipts ? 'bg-entropy-primary' : 'bg-entropy-surface-light'}" aria-label="Toggle Read Receipts">
                                    <div class="absolute top-1 w-4 h-4 bg-white rounded-full transition-all {$userStore.privacySettings.readReceipts ? 'left-7' : 'left-1'}"></div>
                                </button>
                            </div>
                        </div>

                        <div class="space-y-1">
                            <h3 class="font-bold text-entropy-text-primary flex items-center space-x-2">
                                <LucideEyeOff size={18} class="text-entropy-primary" />
                                <span>Last Seen & Online</span>
                            </h3>
                            <p class="text-xs text-entropy-text-secondary leading-relaxed">Control who can see when you were last online.</p>
                            <div class="flex bg-entropy-surface-light p-1 rounded-xl mt-3">
                                <button onclick={() => updatePrivacy({ lastSeen: 'everyone' })} class="flex-1 py-2 text-[10px] font-bold rounded-lg transition {$userStore.privacySettings.lastSeen === 'everyone' ? 'bg-entropy-surface shadow-sm text-entropy-primary' : 'text-entropy-text-dim'}">EVERYONE</button>
                                <button onclick={() => updatePrivacy({ lastSeen: 'nobody' })} class="flex-1 py-2 text-[10px] font-bold rounded-lg transition {$userStore.privacySettings.lastSeen === 'nobody' ? 'bg-entropy-surface shadow-sm text-entropy-primary' : 'text-entropy-text-dim'}">NOBODY</button>
                            </div>
                        </div>
                        <div class="space-y-1">
                            <h3 class="font-bold text-entropy-text-primary flex items-center space-x-2">
                                <LucideGlobe size={18} class="text-entropy-primary" />
                                <span>Network Routing</span>
                            </h3>
                            <p class="text-xs text-entropy-text-secondary leading-relaxed">Route your traffic to hide your IP address. (Tor requires a local Tor instance on port 9050).</p>
                             <div class="flex bg-entropy-surface-light p-1 rounded-xl mt-3">
                                <button onclick={() => { updatePrivacy({ routingMode: 'direct' }); network.reconnect(); }} class="flex-1 py-1.5 text-[9px] font-bold rounded-lg transition {$userStore.privacySettings.routingMode === 'direct' ? 'bg-entropy-surface shadow-sm text-entropy-primary' : 'text-entropy-text-dim'}">DIRECT</button>
                                <button onclick={() => { updatePrivacy({ routingMode: 'tor' }); network.reconnect(); }} class="flex-1 py-1.5 text-[9px] font-bold rounded-lg transition {$userStore.privacySettings.routingMode === 'tor' ? 'bg-entropy-surface shadow-sm text-entropy-primary' : 'text-entropy-text-dim'}">TOR</button>
                                <button onclick={async () => {
                                    const url = await showPrompt("Enter SOCKS5 Proxy URL (e.g. socks5://127.0.0.1:1080):", $userStore.privacySettings.proxyUrl || "", "Custom Proxy");
                                    if (url) {
                                        updatePrivacy({ routingMode: 'custom', proxyUrl: url });
                                        network.reconnect();
                                    }
                                }} class="flex-1 py-1.5 text-[9px] font-bold rounded-lg transition {$userStore.privacySettings.routingMode === 'custom' ? 'bg-entropy-surface shadow-sm text-entropy-primary' : 'text-entropy-text-dim'}">CUSTOM</button>
                            </div>
                        </div>

                        <div class="space-y-1">
                            <h3 class="font-bold text-entropy-text-primary flex items-center space-x-2">
                                <LucideCpu size={18} class="text-entropy-accent" />
                                <span>Decoy Fetching</span>
                            </h3>
                            <p class="text-xs text-entropy-text-secondary leading-relaxed">Fetch multiple random keys when looking up a peer to hide your intent from the server.</p>
                            <div class="flex items-center justify-between pt-2">
                                <button 
                                    onclick={async (e) => {
                                        const btn = e.currentTarget;
                                        btn.disabled = true;
                                        const original = btn.innerText;
                                        btn.innerText = "REFRESHING...";
                                        await refreshDecoys($userStore.relayUrl);
                                        btn.innerText = "DONE!";
                                        setTimeout(() => { 
                                            btn.innerText = original; 
                                            btn.disabled = false;
                                        }, 1000);
                                    }}
                                    class="text-[9px] font-bold text-entropy-accent hover:text-entropy-accent/80 uppercase tracking-tighter"
                                >
                                    REFRESH POOL
                                </button>
                                <button onclick={() => updatePrivacy({ decoyMode: !$userStore.privacySettings.decoyMode })} class="w-12 h-6 rounded-full transition-colors relative {$userStore.privacySettings.decoyMode ? 'bg-entropy-accent' : 'bg-entropy-surface-light'}" aria-label="Toggle Decoy Mode">
                                    <div class="absolute top-1 w-4 h-4 bg-white rounded-full transition-all {$userStore.privacySettings.decoyMode ? 'left-7' : 'left-1'}"></div>
                                </button>
                            </div>
                        </div>

                        <div class="space-y-1">
                            <h3 class="font-bold text-entropy-text-primary flex items-center space-x-2">
                                {#if ($userStore.privacySettings.theme || 'dark') === 'dark'}
                                    <LucideMoon size={18} class="text-entropy-primary" />
                                {:else}
                                    <LucideSun size={18} class="text-entropy-primary" />
                                {/if}
                                <span>Appearance</span>
                            </h3>
                            <p class="text-xs text-entropy-text-secondary leading-relaxed">Switch between high-contrast dark and light modes.</p>
                            <div class="flex bg-entropy-surface-light p-1 rounded-xl mt-3">
                                <button 
                                    onclick={() => updatePrivacy({ theme: 'light' })} 
                                    class="flex-1 py-2 text-[10px] font-bold rounded-lg transition {($userStore.privacySettings.theme || 'dark') === 'light' ? 'bg-entropy-surface shadow-sm text-entropy-primary' : 'text-entropy-text-dim'}"
                                >
                                    LIGHT
                                </button>
                                <button 
                                    onclick={() => updatePrivacy({ theme: 'dark' })} 
                                    class="flex-1 py-2 text-[10px] font-bold rounded-lg transition {($userStore.privacySettings.theme || 'dark') === 'dark' ? 'bg-entropy-surface shadow-sm text-entropy-primary' : 'text-entropy-text-dim'}"
                                >
                                    DARK
                                </button>
                            </div>
                        </div>

                        <div class="p-4 bg-entropy-primary/5 rounded-2xl flex items-start space-x-3">
                            <img src="/logo.png" alt="logo" class="w-8 h-8 object-contain shrink-0 opacity-40 ml-[-4px]" />
                            <div>
                                <div class="text-[11px] font-bold text-entropy-text-primary uppercase tracking-widest mb-1">E2E Integrity</div>
                                <p class="text-[10px] text-entropy-text-secondary leading-snug">All privacy signals are encrypted.</p>
                            </div>
                        </div>

                        <div class="pt-4 space-y-3">
                             <div class="text-[10px] font-bold text-red-500 uppercase tracking-widest mb-3 flex items-center space-x-1">
                                <LucideShieldAlert size={12} />
                                <span>Danger Zone</span>
                             </div>

                            <!-- Panic Mode -->
                            <div class="p-3 bg-entropy-surface-light rounded-2xl space-y-2">
                                <div class="text-[10px] font-bold text-red-500 uppercase tracking-wider">Panic Mode</div>
                                <p class="text-[10px] text-entropy-text-secondary leading-snug">Set a fake password that, when entered at login, silently destroys all data.</p>
                                <button 
                                    onclick={async () => {
                                        const p1 = await showPrompt("Set a PANIC password (entering this at login will WIPE your account):", "", "Panic Password");
                                        if (!p1) return;
                                        const p2 = await showPrompt("Confirm PANIC password:", "", "Confirm Panic Password");
                                        if (p1 !== p2) { addToast("Passwords do not match.", 'error'); return; }
                                        if (await showConfirm(`Are you sure? Entering "${p1}" at login will permanently delete your database.`, "Confirm Panic Activation")) {
                                            try {
                                                await invoke('set_panic_password', { password: p1 });
                                                addToast("Panic password set active. Do NOT forget it.", 'success');
                                            } catch (e) {
                                                addToast("Error: " + e, 'error');
                                            }
                                        }
                                    }}
                                    class="w-full py-2 bg-red-500/10 text-red-500 rounded-lg text-xs font-bold hover:bg-red-500 hover:text-white transition-all transform active:scale-[0.98]"
                                >
                                    Set Panic Password
                                </button>
                            </div>

                             <button 
                                onclick={() => burnAccount($userStore.relayUrl)}
                                class="w-full py-3 bg-red-500/10 text-red-500 rounded-xl text-xs font-bold hover:bg-red-500 hover:text-white transition-all transform active:scale-[0.98] flex items-center justify-center space-x-2"
                             >
                                <LucideTrash2 size={14} />
                                <span>Nuke Account (Forensic Burn)</span>
                             </button>
                        </div>
                    </div>
                {:else}
                    <div class="space-y-4">
                        <h3 class="text-xs font-bold text-entropy-text-dim uppercase tracking-widest">Blocked Identity Hashes</h3>
                        {#if $userStore.blockedHashes.length === 0}
                            <div class="text-center py-12 space-y-2 opacity-30">
                                <LucideBan size={40} class="mx-auto" />
                                <p class="text-sm font-medium">No blocked contacts</p>
                            </div>
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
    {/if}

    {#if showCreateGroup}
        <div class="absolute inset-0 bg-entropy-bg z-[60] flex flex-col animate-in slide-in-from-right duration-300">
            <div class="p-4 flex justify-between items-center bg-entropy-surface">
                <h2 class="font-bold text-entropy-text-primary flex items-center space-x-2"><LucideUsers size={18} /><span>New Group</span></h2>
                <button onclick={() => showCreateGroup = false} class="text-entropy-text-secondary" aria-label="Close panel"><LucideX size={20} /></button>
            </div>
            <div class="p-6 flex-1 space-y-6 overflow-y-auto custom-scrollbar">
                <div class="space-y-2">
                    <label for="group-name-input" class="text-xs font-bold text-entropy-text-dim uppercase">Group Name</label>
                    <input id="group-name-input" bind:value={groupName} placeholder="Enter group name..." class="w-full p-3 bg-entropy-surface-light rounded-xl border-none focus:ring-2 focus:ring-entropy-primary/20" />
                </div>
                <div class="space-y-3">
                    <label for="member-input" class="text-xs font-bold text-entropy-text-dim uppercase">Add Members</label>
                    <div class="flex space-x-2">
                        <input id="member-input" bind:value={memberInput} placeholder="Hash or Nickname..." class="flex-1 p-3 bg-entropy-surface-light rounded-xl border-none text-xs" onkeydown={(e) => e.key === 'Enter' && addMember()} />
                        <button onclick={addMember} aria-label="Add Member" class="bg-entropy-primary text-white p-3 rounded-xl disabled:opacity-50" disabled={!memberInput}><LucidePlus size={20}/></button>
                    </div>
                <div class="space-y-4">
                    <div class="text-[10px] font-bold text-entropy-text-dim uppercase tracking-widest">Select from Contacts</div>
                    <div class="space-y-1 max-h-48 overflow-y-auto custom-scrollbar">
                        {#each Object.values($userStore.chats).filter(c => !c.isGroup) as contact}
                            <button 
                                onclick={() => toggleMember(contact.peerHash)}
                                class="w-full flex items-center justify-between p-2 rounded-xl border-2 {groupMembers.includes(contact.peerHash) ? 'border-entropy-primary bg-entropy-surface' : 'border-transparent bg-entropy-surface-light'} transition"
                            >
                                <div class="flex items-center space-x-3">
                                    <div class="w-8 h-8 rounded-full bg-entropy-surface flex items-center justify-center text-[10px] font-bold text-entropy-primary">
                                        {(contact.localNickname || contact.peerAlias || "?")[0].toUpperCase()}
                                    </div>
                                    <div class="text-left">
                                        <div class="text-xs font-bold text-entropy-text-primary">{contact.localNickname || contact.peerAlias || contact.peerHash.slice(0, 8)}</div>
                                        <div class="text-[9px] font-mono text-entropy-text-dim">{contact.peerHash.slice(0, 16)}...</div>
                                    </div>
                                </div>
                                {#if groupMembers.includes(contact.peerHash)}
                                    <LucideCheckCircle2 size={16} class="text-entropy-primary" />
                                {/if}
                            </button>
                        {/each}
                    </div>
                </div>
                <div class="space-y-2">
                    {#each groupMembers as m}<div class="flex items-center justify-between p-2 bg-entropy-surface rounded-lg text-[10px] font-mono text-entropy-text-primary"><span>{m.slice(0, 32)}...</span><button onclick={() => removeMember(m)}><LucideX size={14}/></button></div>{/each}
                </div>
            </div>
        </div>
        <div class="p-6"><button onclick={handleCreateGroup} disabled={!groupName || groupMembers.length === 0} class="w-full py-4 bg-entropy-primary text-white rounded-2xl font-bold shadow-lg active:scale-[0.98] transition">Create Group Chat</button></div>
    </div>
    {/if}

  <div class="mt-auto bg-entropy-surface/50">
    {#if $userStore.connectionStatus !== 'connected'}
        <div class="p-3 px-4 flex items-center space-x-3 animate-in fade-in slide-in-from-bottom-2 duration-300">
            {#if $userStore.connectionStatus === 'mining'}
                <div class="w-3 h-3 border-2 border-entropy-primary/20 border-t-entropy-primary rounded-full animate-spin"></div>
                <div class="flex flex-col min-w-0">
                    <span class="text-[9px] font-black uppercase text-entropy-primary tracking-tighter">Securing Link</span>
                    <span class="text-[8px] font-bold text-entropy-text-dim truncate tracking-tight">SOLVING PROOF-OF-WORK...</span>
                </div>
            {:else if $userStore.connectionStatus === 'connecting'}
                <div class="w-3 h-3 border-2 border-entropy-text-dim/20 border-t-entropy-text-dim rounded-full animate-spin"></div>
                <div class="flex flex-col min-w-0">
                    <span class="text-[9px] font-black uppercase text-entropy-text-secondary tracking-tighter">Connecting</span>
                    <span class="text-[8px] font-bold text-entropy-text-dim truncate tracking-tight">ESTABLISHING RELAY...</span>
                </div>
            {:else}
                <LucideWifiOff size={14} class="text-red-500 animate-pulse" />
                <div class="flex-1 flex flex-col min-w-0">
                    <span class="text-[9px] font-black uppercase text-red-500 tracking-tighter">Offline</span>
                    <span class="text-[8px] font-bold text-red-400/60 truncate tracking-tight">WAITING FOR NETWORK...</span>
                </div>
                <button onclick={() => window.location.reload()} class="text-[8px] font-black uppercase text-blue-600 hover:underline">Retry</button>
            {/if}
        </div>
    {:else}
        <div class="p-2 px-4 flex items-center justify-between opacity-40 hover:opacity-100 transition-opacity">
            <div class="flex items-center space-x-1.5">
                <div class="w-1.5 h-1.5 bg-green-500 rounded-full shadow-[0_0_4px_rgba(34,197,94,0.5)]"></div>
                <span class="text-[8px] font-black uppercase tracking-widest text-entropy-text-dim">Relay Connection Healthy</span>
            </div>
        </div>
    {/if}
  </div>
</div>

{#if editingImage}
    <ImageEditor 
        image={editingImage} 
        onSave={handleSavePfp} 
        onCancel={() => editingImage = null} 
    />
{/if}

{#if viewingImage}
    <div class="fixed inset-0 z-[100] flex items-center justify-center bg-black/90 backdrop-blur-md p-4 animate-in fade-in duration-300" onclick={() => viewingImage = null}>
        <div class="relative max-w-4xl max-h-[90vh] flex flex-col items-center">
            <button class="absolute -top-12 right-0 p-2 text-white/60 hover:text-white transition" onclick={() => viewingImage = null}>
                <LucideX size={32} />
            </button>
            <img src={viewingImage} alt="Preview" class="max-w-full max-h-full rounded-2xl shadow-2xl animate-in zoom-in-95 duration-300 pointer-events-none" />
        </div>
    </div>
{/if}

{#if viewingImage}
    <div class="fixed inset-0 z-[100] flex items-center justify-center bg-black/90 backdrop-blur-md p-4 animate-in fade-in duration-300" onclick={() => viewingImage = null}>
        <div class="relative max-w-4xl max-h-[90vh] flex flex-col items-center">
            <button class="absolute -top-12 right-0 p-2 text-white/60 hover:text-white transition" onclick={() => viewingImage = null}>
                <LucideX size={32} />
            </button>
            <img src={viewingImage} alt="Preview" class="max-w-full max-h-full rounded-2xl shadow-2xl animate-in zoom-in-95 duration-300 pointer-events-none" />
        </div>
    </div>
{/if}

<style>
    .custom-scrollbar::-webkit-scrollbar { width: 4px; }
    .custom-scrollbar::-webkit-scrollbar-thumb { background: rgba(0,0,0,0.1); border-radius: 10px; }
</style>
