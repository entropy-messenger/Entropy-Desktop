<script lang="ts">
  import { userStore } from '../lib/stores/user';
  import { 
    startChat, togglePin, toggleArchive, lookupNickname, updatePrivacy, resolveIdentity
  } from '../lib/actions/contacts';
  import { deleteChat, jumpToMessage, jumpToPresent } from '../lib/actions/chat';
  import { tick } from 'svelte';
  import { leaveGroup } from '../lib/actions/groups';
  import {
    LucidePlus, LucideSettings, LucideSearch,
    LucideCheck, LucideCheckCheck, LucideClock, LucideUsers,
    LucidePin, LucideArchive, LucideWifiOff, LucideSun, LucideMoon, LucideStar, LucideTrash2
  } from 'lucide-svelte';
  import MessageContent from './MessageContent.svelte';
  import { playingVoiceNoteId } from '../lib/stores/audio';
  import { invoke } from '@tauri-apps/api/core';
  import { addToast, showConfirm, showPrompt } from '../lib/stores/ui';
  import { signalManager } from '../lib/signal_manager';

  import SettingsPanel from './SettingsPanel.svelte';
  import CreateGroupOverlay from './CreateGroupOverlay.svelte';
  import Avatar from './Avatar.svelte';

  let searchQuery = $state("");
  let showCreateGroup = $state(false);

  const pendingResolutions = new Set<string>();
  $effect(() => {
     const unknownHashes = Object.values($userStore.chats)
         .filter(chat => chat.isGroup && chat.lastSenderHash && !$userStore.nicknames[chat.lastSenderHash])
         .map(chat => chat.lastSenderHash as string);

     if (unknownHashes.length === 0) return;

     unknownHashes.forEach(hash => {
         if (pendingResolutions.has(hash)) return;
         pendingResolutions.add(hash);
         
         // Stagger requests to avoid relay rate-limiting and UI thread saturation
         setTimeout(async () => {
             try {
                await resolveIdentity(hash);
             } finally {
                pendingResolutions.delete(hash);
             }
         }, 500 + (Math.random() * 2000));
     });
   });
  let filter = $state<'all' | 'archived'>('all');
  
  let { 
      showStarredMessages = $bindable(false),
      updateAvailable = null,
      onUpdateClick = () => {}
  } = $props();
  
  import { messageStore } from '../lib/stores/user';

  let starredMessages = $derived(Object.entries($messageStore).flatMap(([peerHash, msgs]) => 
    msgs.filter(m => m.isStarred).map(m => ({
        ...m,
        peerHash,
        displayNickname: $userStore.nicknames[peerHash] || peerHash.slice(0, 8)
    }))
  ).sort((a, b) => b.timestamp - a.timestamp));
  
  let activeHash = $derived($userStore.activeChatHash);

  const selectChat = (hash: string) => {
    userStore.update(s => {
        if (s.chats[hash]) { s.chats[hash] = { ...s.chats[hash], unreadCount: 0 }; }
        return { ...s, activeChatHash: hash, chats: { ...s.chats } };
    });
    invoke('db_reset_unread_count', { address: hash }).catch(() => {});
    showStarredMessages = false;
    jumpToPresent(hash);
  };

  const jumpToChat = async (hash: string, msgId: string) => {
    userStore.update(s => {
        if (s.chats[hash]) { s.chats[hash] = { ...s.chats[hash], unreadCount: 0 }; }
        return { ...s, activeChatHash: hash, chats: { ...s.chats } };
    });
    showStarredMessages = false;
    await jumpToMessage(hash, msgId);
    await tick();
    const el = document.getElementById(`msg-${msgId}`);
    if (el) el.scrollIntoView({ behavior: 'smooth', block: 'center' });
  };

  const createChatPrompt = async () => {
    let input = await showPrompt("Enter Peer ID Hash or Global Nickname:", "", "New Chat");
    if (!input) return;

    if (input.length === 64 && /^[0-9a-fA-F]+$/.test(input)) {
        startChat(input);
    } else {
        const hash = await lookupNickname(input);
        if (hash) { startChat(hash, input); } else { addToast("Could not find user.", 'error'); }
    }
  };

  let showSettings = $state(false);
  const toggleSettings = () => showSettings = !showSettings;
  
  const toggleTheme = () => {
      const current = $userStore.privacySettings.theme || 'dark';
      updatePrivacy({ theme: current === 'dark' ? 'light' : 'dark' });
  };

  let searchMessages = $state<any[]>([]);
  let isSearching = $state(false);

  $effect(() => {
    const query = searchQuery.trim();
    if (query.length > 2) {
        isSearching = true;
        const timer = setTimeout(async () => {
            try {
                const results = await invoke<any[]>('db_search_messages', { query });
                searchMessages = results.map(m => ({
                    ...m,
                    peerNickname: $userStore.nicknames[m.chatAddress] || m.chatAddress.slice(0, 8)
                }));
            } finally { isSearching = false; }
        }, 300);
        return () => clearTimeout(timer);
    } else {
        searchMessages = [];
        isSearching = false;
    }
  });

  let filteredChats = $derived(Object.values($userStore.chats).filter(chat => {
    const query = searchQuery.toLowerCase();
    const chatName = ($userStore.nicknames[chat.peerHash] || "").toLowerCase();
    const matchesName = chatName.includes(query) || chat.peerHash.toLowerCase().includes(query);
    if (filter === 'archived' && !chat.isArchived) return false;
    if (filter === 'all' && chat.isArchived) return false;
    return matchesName;
  }).sort((a, b) => {
    if (a.isPinned && !b.isPinned) return -1;
    if (!a.isPinned && b.isPinned) return 1;
    const aTime = a.lastTimestamp || 0;
    const bTime = b.lastTimestamp || 0;
    return bTime - aTime;
  }));

  const canSeeTyping = $derived($userStore.privacySettings.typingStatus !== 'nobody');
  const canSeeReceipts = $derived($userStore.privacySettings.readReceipts);

    const resolveHashesInText = (text: string) => {
        if (!text) return text;
        const hashRegex = /\b([a-fA-F0-9]{64})\b/g;
        return text.replace(hashRegex, (match) => {
            const nick = $userStore.nicknames[match.toLowerCase()];
            return nick ? nick : match.slice(0, 8);
        });
    };
</script>

<div class="h-full w-80 bg-entropy-bg flex flex-col relative shrink-0">
  <div class="p-4 flex flex-col space-y-4 bg-entropy-surface/50">
    <div class="flex justify-end items-center px-1">
        <div class="flex items-center space-x-1">
            <button onclick={toggleTheme} class="p-2 hover:bg-entropy-surface-light rounded-full text-entropy-text-dim hover:text-entropy-primary transition" title="Toggle Theme">
                {#if ($userStore.privacySettings.theme || 'dark') === 'dark'}<LucideSun size={18} />{:else}<LucideMoon size={18} />{/if}
            </button>
            <button onclick={() => showStarredMessages = true} class="p-2 hover:bg-entropy-surface-light rounded-full {showStarredMessages ? 'text-yellow-500 bg-yellow-500/10' : 'text-entropy-text-dim hover:text-yellow-500'} transition" title="Starred Messages"><LucideStar size={18} /></button>
            <button onclick={() => showCreateGroup = true} class="p-2 hover:bg-entropy-surface-light rounded-full text-entropy-text-secondary transition" title="New Group"><LucideUsers size={18} /></button>
            <button onclick={createChatPrompt} class="p-2 hover:bg-entropy-surface-light rounded-full text-entropy-primary transition" title="New Message"><LucidePlus size={20} /></button>
            <button onclick={toggleSettings} class="p-2 hover:bg-entropy-surface-light rounded-full text-entropy-text-dim transition"><LucideSettings size={18} /></button>
        </div>
    </div>

    <div class="flex bg-entropy-surface-light rounded-lg p-1 text-[10px] font-bold uppercase tracking-wider">
        <button onclick={() => filter = 'all'} class="flex-1 py-1 rounded-md transition {filter === 'all' ? 'bg-entropy-surface shadow-sm text-entropy-primary' : 'text-entropy-text-dim hover:text-entropy-text-secondary'}">All Chats</button>
        <button onclick={() => filter = 'archived'} class="flex-1 py-1 rounded-md transition {filter === 'archived' ? 'bg-entropy-surface shadow-sm text-entropy-primary' : 'text-entropy-text-dim hover:text-entropy-text-secondary'}">Archived</button>
    </div>

    <div class="relative">
        <LucideSearch size={14} class="absolute left-3 top-1/2 -translate-y-1/2 text-entropy-text-dim" />
        <input type="text" bind:value={searchQuery} placeholder="Search messages & contacts..." class="w-full pl-9 pr-4 py-2 bg-entropy-surface-light focus:bg-entropy-surface border-none rounded-xl text-xs transition ring-1 ring-white/5" />
    </div>
    
  </div>

  <div class="flex-1 overflow-y-auto custom-scrollbar">
    {#if searchQuery.trim().length > 2}
        <div class="px-4 py-2 bg-entropy-surface-light/50 border-b border-entropy-border/5">
            <div class="flex items-center justify-between">
                <span class="text-[10px] font-black uppercase tracking-[0.2em] text-entropy-primary">Search Results</span>
                {#if isSearching}<div class="w-3 h-3 border-2 border-entropy-primary border-t-transparent rounded-full animate-spin"></div>{/if}
            </div>
        </div>
        
        {#if searchMessages.length === 0 && !isSearching}
             <div class="p-8 text-center space-y-2"><div class="text-entropy-text-dim text-xs font-medium">No messages found for "{searchQuery}"</div></div>
        {/if}

        {#each searchMessages as msg}
            <div class="p-4 hover:bg-entropy-surface/50 cursor-pointer border-b border-entropy-border/5 transition group/search" onclick={() => jumpToChat(msg.chatAddress, msg.id)} onkeypress={(e) => e.key === 'Enter' && jumpToChat(msg.chatAddress, msg.id)} role="button" tabindex="0">
                <div class="flex items-center space-x-3 mb-3">
                    <Avatar hash={msg.chatAddress} alias={msg.peerNickname} size="w-6 h-6" textSize="text-[10px]" rounded="rounded-md" />
                    <div class="flex justify-between items-baseline flex-1 min-w-0">
                        <span class="text-xs font-black text-entropy-text-primary tracking-tight truncate pr-2">{msg.peerNickname}</span>
                        <span class="text-[9px] font-bold text-entropy-text-dim text-right shrink-0">{new Date(msg.timestamp).toLocaleDateString()}</span>
                    </div>
                </div>
                <div class="pointer-events-none">
                    <MessageContent {msg} isMine={msg.isMine} chatAddress={msg.chatAddress} compactMode={true} />
                </div>
            </div>
        {/each}

        {#if filteredChats.length > 0}
            <div class="px-4 py-2 bg-entropy-surface-light/50 border-y border-entropy-border/5 mt-4">
                <span class="text-[10px] font-black uppercase tracking-[0.2em] text-entropy-text-dim">Matched Contacts</span>
            </div>
        {/if}
    {/if}

    {#each filteredChats as chat (chat.peerHash)}
        <div class="group/item mx-2 my-0.5 p-3 rounded-2xl hover:bg-entropy-surface/50 cursor-pointer transition relative {activeHash === chat.peerHash ? 'bg-entropy-primary/10 ring-1 ring-entropy-primary/20' : ''}" onclick={() => selectChat(chat.peerHash)} onkeypress={(e) => e.key === 'Enter' && selectChat(chat.peerHash)} role="button" tabindex="0">
                <div class="flex items-center space-x-3">
                    <Avatar hash={chat.peerHash} alias={$userStore.nicknames[chat.peerHash]} />
                    
                    <div class="flex-1 min-w-0">
                        <div class="flex justify-between items-baseline mb-0.5">
                            <div class="font-bold text-entropy-text-primary truncate flex items-center space-x-1">
                                {#if chat.isGroup}<LucideUsers size={12} class="text-entropy-primary" />{/if}
                                <span class="truncate">
                                    {chat.isGroup 
                                        ? (chat.localNickname || chat.peerHash.slice(0, 8)) 
                                        : ($userStore.nicknames[chat.peerHash] || chat.peerHash.slice(0, 8))}
                                </span>
                                {#if chat.isPinned}<LucidePin size={10} class="text-entropy-primary fill-entropy-primary" />{/if}
                            </div>
                            {#if chat.lastTimestamp}
                                 <div class="text-[10px] font-medium text-entropy-text-secondary shrink-0 opacity-60">
                                    {new Date(chat.lastTimestamp).toLocaleTimeString([], {hour: '2-digit', minute:'2-digit'})}
                                 </div>
                            {/if}
                        </div>
                        <div class="flex items-center justify-between mt-0.5">
                            <div class="text-[12px] truncate pr-2 flex-1 {chat.isTyping && canSeeTyping ? 'text-entropy-accent font-bold' : 'text-entropy-text-dim'}">
                                {#if chat.isTyping && canSeeTyping}
                                    <span class="animate-pulse">typing...</span>
                                {:else if chat.lastMsg}
                                    <div class="flex items-center space-x-1">
                                        {#if chat.lastIsMine}
                                            {#if chat.lastStatus === 'sending' || chat.lastStatus === 'pending'}<LucideClock size={13} class="text-entropy-text-secondary animate-pulse" />
                                            {:else if chat.lastStatus === 'read' && canSeeReceipts}<LucideCheckCheck size={13} class="text-cyan-300" />
                                            {:else if chat.lastStatus === 'read' || chat.lastStatus === 'delivered'}<LucideCheckCheck size={13} class="text-entropy-text-secondary opacity-70" />
                                            {:else}<LucideCheck size={13} class="text-entropy-text-secondary opacity-70" />{/if}
                                        {/if}
                                        <span class="truncate opacity-70">
                                            {#if chat.isGroup && !chat.lastIsMine}<span class="text-entropy-primary font-bold">{$userStore.nicknames[chat.lastSenderHash || ''] || chat.lastSenderHash?.slice(0, 6) || ''}:</span>{/if}
                                            {resolveHashesInText(chat.lastMsg)}
                                        </span>
                                    </div>
                                {/if}
                            </div>
                            
                            {#if chat.unreadCount > 0}
                                <div class="bg-entropy-primary text-white text-[9px] font-bold px-1.5 py-0.5 rounded-full min-w-[17px] text-center shadow-sm">{chat.unreadCount}</div>
                            {/if}
                            
                            <div class="hidden group-hover/item:flex items-center space-x-1 ml-2">
                                 <button onclick={(e) => {e.stopPropagation(); togglePin(chat.peerHash)}} class="p-1 hover:bg-white/10 rounded transition text-entropy-text-dim hover:text-entropy-primary" title="Pin/Unpin"><LucidePin size={12} class={chat.isPinned ? 'fill-entropy-primary text-entropy-primary' : ''} /></button>
                                 <button onclick={(e) => {e.stopPropagation(); toggleArchive(chat.peerHash)}} class="p-1 hover:bg-white/10 rounded transition text-entropy-text-dim hover:text-entropy-primary" title="Archive/Unarchive"><LucideArchive size={12} class={chat.isArchived ? 'fill-entropy-primary text-entropy-primary' : ''} /></button>
                                 <button 
                                    onclick={async (e) => { 
                                        e.stopPropagation(); 
                                        const msg = "Are you sure you want to delete this conversation's history?";
                                        const title = "Delete Chat History";
                                        if (await showConfirm(msg, title)) {
                                            deleteChat(chat.peerHash);
                                        }
                                    }} 
                                    class="p-1 hover:bg-red-500/10 rounded transition text-entropy-text-dim hover:text-red-500" 
                                     title="Delete Chat History"
                                 >
                                    <LucideTrash2 size={12} />
                                 </button>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        {/each}
  </div>

  {#if showSettings}
      <SettingsPanel onClose={() => showSettings = false} />
  {/if}

  {#if showCreateGroup}
      <CreateGroupOverlay onClose={() => showCreateGroup = false} />
  {/if}

  {#if $playingVoiceNoteId}
    <div class="mx-3 mb-2 bg-blue-600 text-white rounded-xl p-2 px-3 flex items-center justify-between animate-in slide-in-from-bottom-2 duration-300 shadow-lg shadow-blue-600/20">
        <div class="flex items-center space-x-2">
            <div class="flex space-x-0.5">
                <div class="w-1 h-2.5 bg-white/40 animate-pulse delay-75"></div>
                <div class="w-1 h-2.5 bg-white/40 animate-pulse delay-150"></div>
                <div class="w-1 h-2.5 bg-white/40 animate-pulse delay-300"></div>
            </div>
            <span class="text-[9px] font-black uppercase tracking-widest">Playing Voice Note</span>
        </div>
        <button onclick={() => playingVoiceNoteId.set(null)} class="text-[8px] font-black uppercase tracking-tighter hover:underline bg-white/10 px-2 py-1 rounded-md">Stop</button>
    </div>
  {/if}

  <div class="mt-auto bg-entropy-surface/50">
    {#if updateAvailable}
        <button 
            onclick={onUpdateClick}
            class="w-[calc(100%-1.5rem)] mx-3 my-2 bg-entropy-primary/20 text-entropy-primary rounded-xl p-2 px-3 flex items-center justify-between animate-in slide-in-from-bottom-2 duration-300 border border-entropy-primary/30 group"
        >
            <div class="flex items-center space-x-2">
                <div class="w-2 h-2 bg-entropy-primary rounded-full animate-pulse"></div>
                <span class="text-[9px] font-black uppercase tracking-widest leading-none">Update v{updateAvailable}</span>
            </div>
            <span class="text-[8px] font-black uppercase tracking-tighter opacity-70 group-hover:opacity-100 transition-opacity">Get it</span>
        </button>
    {/if}

    {#if $userStore.connectionStatus !== 'connected'}
        <div class="p-3 px-4 flex items-center space-x-3 animate-in fade-in slide-in-from-bottom-2 duration-300">
            {#if $userStore.connectionStatus === 'mining'}
                <div class="w-3 h-3 border-2 border-entropy-primary/20 border-t-entropy-primary rounded-full animate-spin"></div>
                <div class="flex flex-col min-w-0"><span class="text-[9px] font-black uppercase text-entropy-primary tracking-tighter">Securing Identity</span><span class="text-[8px] font-bold text-entropy-text-dim truncate tracking-tight">AUTHENTICATING...</span></div>
            {:else if $userStore.connectionStatus === 'connecting'}
                <div class="w-3 h-3 border-2 border-entropy-text-dim/20 border-t-entropy-text-dim rounded-full animate-spin"></div>
                <div class="flex flex-col min-w-0"><span class="text-[9px] font-black uppercase text-entropy-text-secondary tracking-tighter">Connecting</span><span class="text-[8px] font-bold text-entropy-text-dim truncate tracking-tight">ESTABLISHING CONNECTION...</span></div>
            {:else if $userStore.connectionStatus === 'reconnecting'}
                <div class="w-3 h-3 border-2 border-yellow-500/20 border-t-yellow-500 rounded-full animate-spin"></div>
                <div class="flex flex-col min-w-0"><span class="text-[9px] font-black uppercase text-yellow-500 tracking-tighter">Reconnecting</span><span class="text-[8px] font-bold text-entropy-text-dim truncate tracking-tight">RETRYING IN {$userStore.reconnectTimer || 0}s...</span></div>
            {:else if $userStore.connectionStatus === 'jailed'}
                <LucideWifiOff size={14} class="text-red-600 animate-pulse" />
                <div class="flex flex-col min-w-0"><span class="text-[9px] font-black uppercase text-red-600 tracking-tighter">Connection Jailed</span><span class="text-[8px] font-bold text-red-400/60 truncate tracking-tight">TEMPORARILY SUSPENDED</span></div>
            {:else}
                <LucideWifiOff size={14} class="text-red-500 animate-pulse" />
                <div class="flex-1 flex flex-col min-w-0"><span class="text-[9px] font-black uppercase text-red-500 tracking-tighter">Offline</span><span class="text-[8px] font-bold text-red-400/60 truncate tracking-tight">WAITING FOR NETWORK...</span></div>
                <button onclick={() => window.location.reload()} class="text-[8px] font-black uppercase text-blue-600 hover:underline">Retry</button>
            {/if}
        </div>
    {:else}
        <div class="p-2 px-4 flex items-center justify-between opacity-40 hover:opacity-100 transition-opacity">
            <div class="flex items-center space-x-1.5">
                {#if $userStore.isSynced}
                    <div class="w-1.5 h-1.5 bg-green-500 rounded-full shadow-[0_0_4px_rgba(34,197,94,0.5)]"></div>
                    <span class="text-[8px] font-black uppercase tracking-widest text-entropy-text-dim">Relay Connection Healthy</span>
                {:else}
                    <div class="w-1.5 h-1.5 bg-yellow-500 rounded-full shadow-[0_0_4px_rgba(234,179,8,0.5)]"></div>
                    <span class="text-[8px] font-black uppercase tracking-widest text-yellow-500/80">Signal Sync Degraded</span>
                    <button onclick={() => signalManager.ensureKeysUploaded()} class="ml-1 text-[8px] font-black uppercase text-blue-500 hover:underline">Retry</button>
                {/if}
            </div>
        </div>
    {/if}
  </div>
</div>

<style>
    .custom-scrollbar::-webkit-scrollbar { width: 4px; }
    .custom-scrollbar::-webkit-scrollbar-thumb { background: rgba(0,0,0,0.1); border-radius: 10px; }
</style>
