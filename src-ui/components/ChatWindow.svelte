
<script lang="ts">
  import { userStore } from '../lib/stores/user';
  import { 
    sendMessage, sendFile, sendVoiceNote, 
    sendTypingStatus, setLocalNickname, toggleStar, 
    setReplyingTo, leaveGroup, addToGroup,
    bulkDelete, bulkStar, toggleBlock, toggleVerification 
  } from '../lib/store';
  import { signalManager } from '../lib/signal_manager';
  import { 
    LucideSend, LucideMoreVertical, LucideMic, LucidePaperclip, 
    LucideX, LucideSearch, 
    LucideCheck, LucideCheckCheck, LucideEdit2, LucideUsers,
    LucideStar, LucideReply, LucideClock, LucideBellOff, LucideTrash2,
    LucideExternalLink, LucideImage, LucideLink, LucideFile, LucideInfo,
    LucideCopy, LucideCheck as LucideCheckIcon, LucideShare2, LucideBan,
    LucideShieldCheck, LucideShieldAlert, LucideSquare, LucideTrash
  } from 'lucide-svelte';
  import AttachmentRenderer from './AttachmentRenderer.svelte';
  import VoiceNotePlayer from './VoiceNotePlayer.svelte';
  import MediaThumbnail from './MediaThumbnail.svelte';
  import { onMount, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { playingVoiceNoteId } from '../lib/stores/audio';
  import { addToast, showConfirm, showPrompt } from '../lib/stores/ui';
  import { LucideX as LucideXLarge } from 'lucide-svelte';
  import type { Message } from '../lib/types';
  
  let { showStarredMessages = false, onCloseStarred }: { showStarredMessages?: boolean; onCloseStarred?: () => void } = $props();
  
  /**
   * Primary messaging interface for end-to-end encrypted conversations.
   * Manages real-time input orchestration, native media recording, and message state rendering.
   */
  let messageInput = $state("");
  let fileInput = $state<HTMLInputElement | null>(null);
  let messageSearchQuery = $state("");
  let showMessageSearch = $state(false);
  let scrollContainer = $state<HTMLElement | null>(null);
  let showOptions = $state(false);
  let showGallery = $state(false);

  let selectionMode = $state(false);
  let selectedIds = $state<string[]>([]);
  
  let isRecording = $state(false);
  let recordedBlob = $state<Blob | null>(null);
  let previewUrl = $state<string | null>(null);
  let recordingSeconds = $state(0);
  let recordingInterval: any = null;
  let messageInputEl = $state<HTMLTextAreaElement | null>(null);
  let visualizerCanvas = $state<HTMLCanvasElement | null>(null);
  let volumeUnlisten: (() => void) | null = null;
  let currentVolume = $state(0);
  let viewingImage = $state<string | null>(null);
  let safetyNumber = $state<{ digits: string; isVerified: boolean } | null>(null);
  let loadingSafetyNumber = $state(false);

  async function loadSafetyNumber() {
    if (!activeChat || activeChat.isGroup) {
        safetyNumber = null;
        return;
    }
    loadingSafetyNumber = true;
    try {
        const result = await signalManager.getFingerprint(activeChat.peerHash);
        safetyNumber = result;
    } catch (e) {
        console.error("Error loading safety number:", e);
    } finally {
        loadingSafetyNumber = false;
    }
  }

  $effect(() => {
    if (showGallery && activeChat && !activeChat.isGroup) {
        loadSafetyNumber();
    }
  });

  $effect(() => {
    if (messageInput !== undefined && messageInputEl) {
        messageInputEl.style.height = 'auto';
        messageInputEl.style.height = Math.min(messageInputEl.scrollHeight, 200) + 'px';
    }
  });

  let activeChat = $derived($userStore.activeChatHash ? $userStore.chats[$userStore.activeChatHash] : null);
  let replyingTo = $derived($userStore.replyingTo);
  
  const scrollToBottom = async () => {
      await tick();
      if (scrollContainer && !selectionMode) scrollContainer.scrollTop = scrollContainer.scrollHeight;
  };

  $effect(() => {
    if (activeChat?.messages) {
      scrollToBottom();
    }
  });

  const handleSend = () => {
    if (!messageInput.trim() || !activeChat) return;
    const dest = activeChat.peerHash;
    sendMessage(dest, messageInput);
    messageInput = "";
    isLocallyTyping = false;
    sendTypingStatus(dest, false).catch(() => {});
  };
  
  const handleKeydown = (e: KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        handleSend();
    }
  };

  const onFileSelect = (e: Event) => {
    const files = (e.target as HTMLInputElement).files;
    if (files && files.length > 0 && activeChat) sendFile(activeChat.peerHash, files[0]);
  };

  const startRecording = async () => {
    try {
        await invoke('start_native_recording');
        
        isRecording = true;
        recordingSeconds = 0;
        recordedBlob = null;
        if (previewUrl) URL.revokeObjectURL(previewUrl);
        previewUrl = null;

        if (volumeUnlisten) volumeUnlisten();
        volumeUnlisten = await listen<number>('recording-volume', (event) => {
            currentVolume = event.payload;
            drawNativeWaveform();
        });

        recordingInterval = setInterval(() => { recordingSeconds++; }, 1000);
    } catch (e: any) { 
        console.error("Recording error:", e); 
        addToast("Microphone error: " + e, 'error');
    }
  };

  const drawNativeWaveform = () => {
      if (!visualizerCanvas) return;
      const ctx = visualizerCanvas.getContext('2d');
      if (!ctx) return;
      
      const width = visualizerCanvas.width;
      const height = visualizerCanvas.height;

      const bars = 50;
      const barWidth = width / bars;

      const imageData = ctx.getImageData(barWidth, 0, width - barWidth, height);
      ctx.clearRect(0, 0, width, height);
      ctx.putImageData(imageData, 0, 0);

      const barHeight = Math.max(4, currentVolume * height * 5);
      ctx.fillStyle = '#ef4444';
      ctx.beginPath();
      ctx.roundRect(width - barWidth + 1, (height - barHeight) / 2, barWidth - 3, barHeight, 1.5);
      ctx.fill();
  };

  const stopRecording = async () => {
      if (isRecording) {
          try {
              const bytes = await invoke<number[]>('stop_native_recording');
              recordedBlob = new Blob([new Uint8Array(bytes)], { type: 'audio/wav' });
              previewUrl = URL.createObjectURL(recordedBlob);
              
              isRecording = false;
              if (recordingInterval) clearInterval(recordingInterval);
              if (volumeUnlisten) {
                  volumeUnlisten();
                  volumeUnlisten = null;
              }
          } catch (e) {
              console.error("Stop recording error:", e);
          }
      }
  };

  const cancelRecording = async () => {
      if (isRecording) {
          await invoke('stop_native_recording');
      }
      isRecording = false;
      if (recordingInterval) clearInterval(recordingInterval);
      if (volumeUnlisten) {
          volumeUnlisten();
          volumeUnlisten = null;
      }
      recordedBlob = null;
      if (previewUrl) URL.revokeObjectURL(previewUrl);
      previewUrl = null;
      recordingSeconds = 0;
  };

  const sendRecordedVoiceNote = () => {
      if (recordedBlob && activeChat) {
          sendVoiceNote(activeChat.peerHash, recordedBlob);
          cancelRecording();
      }
  };

  const formatRecordingTime = (s: number) => {
      const mins = Math.floor(s / 60);
      const secs = s % 60;
      return `${mins}:${secs.toString().padStart(2, '0')}`;
  };

  const handleSetLocalNickname = async () => {
      if (!activeChat) return;
      try {
          const val = await showPrompt("Set a local nickname for this contact:", activeChat.localNickname || activeChat.peerAlias || "", "Local Nickname");
          if (val !== null) {
              await setLocalNickname(activeChat.peerHash, val.trim() || null);
              addToast("Nickname updated", 'success');
          }
      } catch (e) {
          console.error("Failed to set nickname:", e);
          addToast("Failed to set nickname", 'error');
      }
  };

  const scrollToMessage = (id: string) => {
      const el = document.getElementById(`msg-${id}`);
      if (el) el.scrollIntoView({ behavior: 'smooth', block: 'center' });
  };
  
  const toggleSelect = (id: string) => {
      if (!selectionMode) { selectionMode = true; selectedIds = [id]; return; }
      if (selectedIds.includes(id)) {
          selectedIds = selectedIds.filter(x => x !== id);
          if (selectedIds.length === 0) selectionMode = false;
      } else {
          selectedIds = [...selectedIds, id];
      }
  };

  const cancelSelection = () => { selectionMode = false; selectedIds = []; };
  
  const handleBulkStar = () => { if (activeChat) { bulkStar(activeChat.peerHash, selectedIds); cancelSelection(); } };
  const handleBulkDelete = async () => { 
    if (activeChat && await showConfirm(`Delete ${selectedIds.length} messages?`, "Delete Messages")) { 
        bulkDelete(activeChat.peerHash, selectedIds); cancelSelection(); 
    } 
  };
  
  let mediaMessages = $derived(activeChat ? activeChat.messages.filter((m: any) => m.attachment && (m.type === 'file' || m.type === 'voice_note')) : []);
  let linkMessages = $derived(activeChat ? activeChat.messages.filter((m: any) => m.linkPreview?.url) : []);
  
  let allStarredMessages = $derived(Object.values($userStore.chats).flatMap(chat => 
    chat.messages.filter(m => m.isStarred).map(m => ({
        ...m,
        peerHash: chat.peerHash,
        peerAlias: chat.localNickname || chat.peerAlias || chat.peerHash.slice(0, 8)
    }))
  ).sort((a, b) => b.timestamp - a.timestamp));
  
  const selectChat = (hash: string) => {
    userStore.update(s => ({ ...s, activeChatHash: hash }));
    if (onCloseStarred) onCloseStarred();
  };

  let inviteCopied = $state(false);
  const copyInvite = () => {
      if (!activeChat) return;
      const link = `entropy://${activeChat.peerHash}`;
      navigator.clipboard.writeText(link);
      inviteCopied = true;
      setTimeout(() => inviteCopied = false, 2000);
  };
  
  let typingTimeout: any;
  let isLocallyTyping = false;
  let lastTypingSent = 0;
  let lastTypingPeer: string | null = null;

  $effect(() => {
      const currentInput = messageInput;
      const currentPeer = activeChat?.peerHash;
      
      if (lastTypingPeer && lastTypingPeer !== currentPeer) {
          if (isLocallyTyping) {
              sendTypingStatus(lastTypingPeer, false).catch(() => {});
              isLocallyTyping = false;
          }
      }
      lastTypingPeer = currentPeer;

      if (currentInput.length > 0 && activeChat && !activeChat.isGroup) {
          const now = Date.now();
          // Send if first time or every 4s to keep recipient timer alive
          if (!isLocallyTyping || (now - lastTypingSent > 4000)) {
              isLocallyTyping = true;
              lastTypingSent = now;
              sendTypingStatus(activeChat.peerHash, true).catch(() => {});
          }
      } else if (currentInput.length === 0 && isLocallyTyping && activeChat && !activeChat.isGroup) {
          isLocallyTyping = false;
          sendTypingStatus(activeChat.peerHash, false).catch(() => {});
      }
      
      if (typingTimeout) clearTimeout(typingTimeout);
      if (activeChat && !activeChat.isGroup) {
          const typingPeer = activeChat.peerHash;
          typingTimeout = setTimeout(() => {
              if (isLocallyTyping) {
                  isLocallyTyping = false;
                  sendTypingStatus(typingPeer, false).catch(() => {});
              }
          }, 2000);
      }

      return () => {
          if (typingTimeout) clearTimeout(typingTimeout);
      };
  });
</script>

{#if showStarredMessages}
    <div class="h-full w-full flex flex-col bg-entropy-bg">
        <div class="p-4 flex justify-between items-center bg-entropy-surface/50 backdrop-blur-md border-b border-entropy-border/5">
            <div class="flex items-center space-x-3">
                <LucideStar size={24} class="text-yellow-500" />
                <div>
                    <h2 class="font-bold text-entropy-text-primary text-lg">Starred Messages</h2>
                    <p class="text-[10px] text-entropy-text-dim">All your starred messages across all conversations</p>
                </div>
            </div>
            <button onclick={onCloseStarred} class="p-2 hover:bg-entropy-surface-light rounded-full text-entropy-text-dim hover:text-entropy-text-primary transition" title="Close">
                <LucideX size={20} />
            </button>
        </div>

        <div class="flex-1 overflow-y-auto custom-scrollbar p-6">
            {#if allStarredMessages.length === 0}
                <div class="flex flex-col items-center justify-center h-full text-entropy-text-dim space-y-4 opacity-50">
                    <LucideStar size={64} />
                    <div class="space-y-1 text-center">
                        <p class="text-sm font-bold uppercase tracking-widest">No Starred Messages</p>
                        <p class="text-[10px]">Star messages to save them for later.</p>
                    </div>
                </div>
            {:else}
                <div class="space-y-3 max-w-3xl mx-auto">
                    {#each allStarredMessages as msg (msg.id)}
                        <button 
                            onclick={() => selectChat(msg.peerHash)}
                            class="w-full text-left p-5 bg-entropy-surface/50 hover:bg-entropy-surface rounded-2xl transition-all group relative overflow-hidden"
                        >
                            <div class="flex justify-between items-start mb-3">
                                 <div class="flex items-center space-x-2">
                                    <LucideStar size={14} class="text-yellow-500 fill-yellow-500" />
                                    <span class="text-xs font-black uppercase text-entropy-primary tracking-tight">{msg.peerAlias}</span>
                                 </div>
                                 <span class="text-[10px] font-bold text-entropy-text-dim">{new Date(msg.timestamp).toLocaleDateString()} {new Date(msg.timestamp).toLocaleTimeString([], {hour: '2-digit', minute:'2-digit'})}</span>
                            </div>
                            <p class="text-sm text-entropy-text-primary leading-relaxed">{msg.content}</p>
                            <div class="absolute bottom-2 right-2 opacity-0 group-hover:opacity-100 transition-opacity">
                                <div class="bg-entropy-primary text-white text-[9px] font-black px-2 py-1 rounded">VIEW IN CHAT</div>
                            </div>
                        </button>
                    {/each}
                </div>
            {/if}
        </div>
    </div>
{:else if !activeChat}
    <div class="h-full w-full flex items-center justify-center bg-entropy-bg flex-col text-center p-8">
        <div class="w-20 h-20 bg-entropy-surface rounded-3xl flex items-center justify-center mb-6 shadow-xl animate-bounce-subtle">
            <img src="/logo.png" alt="logo" class="w-16 h-16 object-contain" />
        </div>
        <h2 class="text-2xl font-bold text-entropy-text-primary mb-2 tracking-tight">Entropy</h2>
        <p class="text-entropy-text-dim max-w-sm font-medium text-xs leading-relaxed opacity-60">Select a conversation to start messaging.<br>Encrypted P2P Secure Session</p>
    </div>
{:else}
    <div class="h-full w-full flex bg-entropy-bg relative overflow-hidden">
        <div class="flex-1 flex flex-col relative h-full min-w-0">
            <div class="bg-entropy-surface/95 backdrop-blur-md p-3 px-4 flex justify-between items-center shadow-sm z-30">
                <div class="flex items-center space-x-3 overflow-hidden cursor-pointer">
                    <button 
                        class="w-10 h-10 rounded-xl bg-gradient-to-tr {activeChat.isGroup ? 'from-purple-500 to-indigo-600' : 'from-blue-400 to-blue-600'} shrink-0 flex items-center justify-center text-white font-bold shadow-sm relative overflow-hidden active:scale-95 transition-transform"
                        onclick={() => activeChat.pfp && (viewingImage = activeChat.pfp)}
                    >
                        {#if activeChat.pfp}
                            <img src={activeChat.pfp} alt="" class="w-full h-full object-cover" />
                        {:else}
                            {activeChat.peerAlias ? activeChat.peerAlias[0].toUpperCase() : '?'}
                        {/if}
                    </button>
                    <div class="min-w-0">
                        <div class="flex items-center space-x-2">
                            <div class="font-bold text-entropy-text-primary leading-tight truncate">
                                {activeChat.localNickname || activeChat.peerAlias || activeChat.peerHash.slice(0, 12)}
                            </div>
                            {#if activeChat.isVerified}
                                <LucideShieldCheck size={14} class="text-entropy-accent" />
                            {/if}
                            {#if activeChat.isMuted}<LucideBellOff size={11} class="text-entropy-text-dim" />{/if}
                        </div>
                        <div class="text-[11px] font-medium h-3 truncate {activeChat.isTyping ? 'text-entropy-accent animate-pulse' : 'text-entropy-text-dim'}">
                            {activeChat.isTyping ? "typing..." : (activeChat.isOnline ? "Online" : "Offline")}
                        </div>
                    </div>
                </div>

                <div class="flex items-center space-x-1">
                    {#if selectionMode}
                        <div class="flex items-center bg-entropy-primary/10 px-3 py-1 rounded-full mr-2">
                            <span class="text-xs font-bold text-entropy-primary mr-4">{selectedIds.length} selected</span>
                            <button onclick={handleBulkStar} class="p-2 text-entropy-primary hover:bg-entropy-primary/20 rounded-full transition"><LucideStar size={18} /></button>
                            <button onclick={handleBulkDelete} class="p-2 text-red-600 hover:bg-red-100/20 rounded-full transition"><LucideTrash2 size={18} /></button>
                            <button onclick={cancelSelection} class="p-2 text-entropy-text-dim hover:bg-entropy-surface-light rounded-full transition ml-1"><LucideX size={18} /></button>
                        </div>
                    {:else}
                        <button onclick={() => showMessageSearch = !showMessageSearch} class="p-2 {showMessageSearch ? 'text-entropy-primary bg-entropy-primary/10' : 'text-entropy-text-dim'} hover:text-entropy-primary hover:bg-entropy-surface-light rounded-full transition"><LucideSearch size={20} /></button>
                        <div class="relative">
                            <button onclick={() => showOptions = !showOptions} class="p-2 text-entropy-text-dim hover:text-entropy-primary hover:bg-entropy-surface-light rounded-full transition"><LucideMoreVertical size={20} /></button>
                            {#if showOptions}
                                <div class="absolute right-0 mt-2 w-56 bg-entropy-surface rounded-xl shadow-2xl py-2 z-[100] animate-in fade-in zoom-in-95 duration-150" onclick={() => showOptions = false} onkeypress={(e) => e.key === 'Enter' && (showOptions = false)} role="button" tabindex="0">
                                    <button onclick={() => {showGallery = true; showOptions = false;}} class="w-full text-left px-4 py-2 text-sm text-entropy-text-secondary hover:bg-entropy-surface-light flex items-center space-x-3"><LucideInfo size={16} /> <span>Contact Info</span></button>
                                    <button onclick={() => {handleSetLocalNickname(); showOptions = false;}} class="w-full text-left px-4 py-2 text-sm text-entropy-text-secondary hover:bg-entropy-surface-light flex items-center space-x-3"><LucideEdit2 size={16} /> <span>Set Nickname</span></button>
                                    <button onclick={() => {selectionMode = true; showOptions = false;}} class="w-full text-left px-4 py-2 text-sm text-entropy-text-secondary hover:bg-entropy-surface-light flex items-center space-x-3"><LucideCheckIcon size={16} /> <span>Select Messages</span></button>
                                    <div class="h-px bg-entropy-border my-1"></div>
                                    {#if activeChat.isGroup}
                                        <button onclick={async () => { if (await showConfirm("Are you sure you want to leave this group?", "Leave Group")) { leaveGroup(activeChat!.peerHash); showOptions = false; } }} class="w-full text-left px-4 py-2 text-sm text-red-600 hover:bg-red-500/10 flex items-center space-x-3">
                                            <LucideTrash2 size={16} /> <span>Leave Group</span>
                                        </button>
                                    {:else}
                                        <button onclick={() => {toggleBlock(activeChat!.peerHash); showOptions = false;}} class="w-full text-left px-4 py-2 text-sm text-red-600 hover:bg-red-500/10 flex items-center space-x-3">
                                            <LucideBan size={16} /> <span>{$userStore.blockedHashes.includes(activeChat!.peerHash) ? 'Unblock Contact' : 'Block Contact'}</span>
                                        </button>
                                    {/if}
                                </div>
                            {/if}
                        </div>
                    {/if}
                </div>
            </div>
            
            {#if $playingVoiceNoteId}
                <div class="bg-blue-600 text-white p-1.5 px-4 flex items-center justify-between animate-in slide-in-from-top duration-300 z-20">
                    <div class="flex items-center space-x-2">
                        <div class="flex space-x-0.5">
                            <div class="w-1 h-3 bg-white/40 animate-pulse delay-75"></div>
                            <div class="w-1 h-3 bg-white/40 animate-pulse delay-150"></div>
                            <div class="w-1 h-3 bg-white/40 animate-pulse delay-300"></div>
                        </div>
                        <span class="text-[10px] font-bold tracking-wide">Listening to voice note</span>
                    </div>
                    <button onclick={() => playingVoiceNoteId.set(null)} class="text-[10px] font-bold hover:underline bg-white/10 px-2 py-0.5 rounded-md">Stop</button>
                </div>
            {/if}

            {#if showMessageSearch}
                <div class="bg-entropy-surface/95 backdrop-blur-md p-2 px-4 animate-in slide-in-from-top duration-200 z-20">
                    <div class="relative">
                        <LucideSearch size={14} class="absolute left-3 top-1/2 -translate-y-1/2 text-entropy-text-dim" />
                        <input 
                            type="text" 
                            bind:value={messageSearchQuery}
                            placeholder="Search in this conversation..." 
                            class="w-full pl-9 pr-10 py-2 bg-entropy-surface-light border-none rounded-xl text-xs focus:ring-1 focus:ring-entropy-primary transition whitespace-pre text-entropy-text-primary placeholder:text-entropy-text-dim"
                        />
                        {#if messageSearchQuery}
                            <button onclick={() => messageSearchQuery = ""} class="absolute right-3 top-1/2 -translate-y-1/2 text-entropy-text-dim hover:text-entropy-text-primary">
                                <LucideX size={14} />
                            </button>
                        {/if}
                    </div>
                </div>
            {/if}
            
            <div bind:this={scrollContainer} class="flex-1 overflow-y-auto p-4 space-y-3 custom-scrollbar chat-bg">

                {#each activeChat.messages.filter(m => !messageSearchQuery || m.content.toLowerCase().includes(messageSearchQuery.toLowerCase())) as msg (msg.id)}
                    {#if msg.type === 'system'}
                        <div class="flex justify-center my-4">
                            <div class="bg-entropy-surface-light px-4 py-1.5 rounded-full border border-entropy-border/5 flex items-center space-x-2 shadow-sm">
                                <LucideInfo size={12} class="text-entropy-primary" />
                                <span class="text-[11px] font-bold text-entropy-text-dim uppercase tracking-widest">{msg.content}</span>
                            </div>
                        </div>
                    {:else}
                    <div id="msg-{msg.id}" class="flex {msg.isMine ? 'justify-end' : 'justify-start'} group items-center relative z-10">
                        {#if selectionMode}
                            <div class="mr-4 order-first">
                                <button 
                                    onclick={() => toggleSelect(msg.id)}
                                    class="w-5 h-5 rounded-full border-2 transition-all flex items-center justify-center {selectedIds.includes(msg.id) ? 'bg-entropy-primary border-entropy-primary' : 'border-entropy-border/10 bg-entropy-surface'}"
                                >
                                    {#if selectedIds.includes(msg.id)}<LucideCheckIcon size={12} class="text-white" />{/if}
                                </button>
                            </div>
                        {/if}

                        <div class="flex flex-col {msg.isMine ? 'items-end' : 'items-start'} max-w-[65%] relative z-10">
                            <div 
                                class="relative rounded-2xl shadow-sm transition-all duration-200 overflow-hidden
                                    {msg.type === 'voice_note' ? 'p-1.5 px-2' : 'p-2.5 px-4'}
                                    {msg.isMine ? 'bg-entropy-primary text-white rounded-tr-none' : 'bg-entropy-surface-light text-entropy-text-primary rounded-tl-none'}
                                    {msg.isStarred ? 'ring-2 ring-entropy-accent/30' : ''}
                                    {selectedIds.includes(msg.id) ? 'opacity-50 scale-95' : ''}
                                "
                                style="word-break: break-all;"
                                ondblclick={() => toggleSelect(msg.id)}
                                role="button"
                                tabindex="0"
                            >
                                {#if msg.replyTo}
                                    <div onclick={() => scrollToMessage(msg.replyTo!.id)} onkeypress={(e) => e.key === 'Enter' && scrollToMessage(msg.replyTo!.id)} role="button" tabindex="0" class="bg-black/10 dark:bg-white/10 p-2 rounded-lg border-l-4 border-entropy-primary mb-2 cursor-pointer hover:bg-black/20 dark:hover:bg-white/20 transition overflow-hidden">
                                        <div class="text-[10px] font-bold {msg.isMine ? 'text-white' : 'text-entropy-primary'} truncate">{msg.replyTo.senderAlias || 'Peer'}</div>
                                        <div class="{msg.isMine ? 'text-white/80' : 'text-entropy-text-secondary'} text-xs truncate opacity-90">{msg.replyTo.content}</div>
                                    </div>
                                {/if}

                                {#if activeChat.isGroup && !msg.isMine}
                                    <div class="text-[10px] font-bold text-entropy-primary mb-1 opacity-80">{msg.senderAlias || msg.senderHash.slice(0, 12)}</div>
                                {/if}

                                {#if (msg.type === 'voice_note' || msg.type === 'file') && msg.attachment}
                                    <AttachmentRenderer {msg} chatId={activeChat.peerHash} />
                                {:else}
                                    <div class="text-[14px] leading-relaxed whitespace-pre-wrap break-all overflow-hidden">{msg.content}</div>
                                {/if}

                                {#if msg.linkPreview}
                                    <a href={msg.linkPreview.url} target="_blank" rel="noopener noreferrer" class="block mt-2 bg-black/10 dark:bg-white/10 rounded-xl overflow-hidden hover:bg-black/20 dark:hover:bg-white/20 transition no-underline">
                                        <div class="p-3">
                                            <div class="text-[10px] font-bold {msg.isMine ? 'text-white/60' : 'text-entropy-accent'} mb-1 flex items-center space-x-1 uppercase tracking-wide">
                                                <span>{msg.linkPreview.siteName}</span>
                                                <LucideExternalLink size={10} />
                                            </div>
                                            <div class="text-[13px] font-bold {msg.isMine ? 'text-white' : 'text-entropy-text-primary'} truncate mb-1">{msg.linkPreview.title}</div>
                                            <div class="text-[11px] {msg.isMine ? 'text-white/70' : 'text-entropy-text-secondary'} line-clamp-2 leading-snug">{msg.linkPreview.url}</div>
                                        </div>
                                    </a>
                                {/if}

                                <div class="flex items-center justify-end space-x-1 mt-1.5">
                                    {#if msg.isStarred}<LucideStar size={10} class="{msg.isMine ? 'text-white fill-white' : 'text-entropy-accent fill-entropy-accent'}" />{/if}
                                    <span class="text-[9px] font-bold {msg.isMine ? 'text-white/60' : 'text-entropy-text-secondary'}">{new Date(msg.timestamp).toLocaleTimeString([], {hour: '2-digit', minute:'2-digit'})}</span>
                                    {#if msg.isMine}
                                        {#if msg.status === 'read'}<LucideCheckCheck size={12} class="text-white/90" />
                                        {:else if msg.status === 'delivered'}<LucideCheckCheck size={12} class="text-white/60" />
                                        {:else}<LucideCheck size={12} class="text-white/60" />{/if}
                                    {/if}
                                </div>
                            </div>

                            {#if !selectionMode}
                                <div class="absolute {msg.isMine ? '-left-8' : '-right-8'} top-0 bottom-0 opacity-0 group-hover:opacity-100 flex flex-col items-center justify-center space-y-1 transition-all duration-200 z-10">
                                    <button onclick={() => setReplyingTo(msg)} class="p-1.5 hover:bg-entropy-surface-light bg-entropy-surface/80 backdrop-blur-sm rounded-full text-entropy-text-dim hover:text-entropy-primary shadow-sm transition active:scale-90" title="Reply"><LucideReply size={14} /></button>
                                    <button onclick={() => toggleStar(activeChat.peerHash, msg.id)} class="p-1.5 hover:bg-entropy-surface-light bg-entropy-surface/80 backdrop-blur-sm rounded-full text-entropy-text-dim hover:text-yellow-500 shadow-sm transition active:scale-90" title="Star"><LucideStar size={14} class={msg.isStarred ? 'fill-yellow-500 text-yellow-500' : ''} /></button>
                                </div>
                            {/if}
                        </div>
                    </div>
                    {/if}
                {/each}
            </div>
            
            {#if replyingTo}
                <div class="px-4 py-2 bg-entropy-surface/95 backdrop-blur-md flex items-center animate-in slide-in-from-bottom duration-200">
                    <div class="flex-1 bg-entropy-surface-light rounded-xl p-2 border-l-4 border-entropy-primary flex items-center justify-between">
                        <div class="min-w-0">
                            <div class="text-[10px] font-black text-entropy-primary truncate">{replyingTo.isMine ? 'You' : (replyingTo.senderAlias || 'Peer')}</div>
                            <div class="text-xs text-entropy-text-secondary truncate">{replyingTo.content}</div>
                        </div>
                        <button onclick={() => setReplyingTo(null)} class="p-1.5 hover:bg-entropy-surface rounded-full text-entropy-text-dim"><LucideX size={16} /></button>
                    </div>
                </div>
            {/if}

            {#if $userStore.blockedHashes.includes(activeChat.peerHash)}
                <div class="p-6 bg-entropy-surface/50 backdrop-blur-sm flex flex-col items-center justify-center space-y-3">
                    <div class="flex items-center space-x-2 text-red-500 font-bold text-xs uppercase tracking-wide">
                        <LucideBan size={14} />
                        <span>Contact blocked</span>
                    </div>
                    <button 
                        onclick={() => toggleBlock(activeChat.peerHash)}
                        class="px-6 py-2 bg-entropy-primary text-white text-xs font-bold rounded-xl shadow-lg hover:bg-entropy-primary-dim transition active:scale-[0.98]"
                    >
                        Unblock
                    </button>
                    <p class="text-[11px] text-entropy-text-dim max-w-xs text-center leading-relaxed">Blocked contacts cannot call or message you.</p>
                </div>
            {:else}
                <div class="p-3 bg-entropy-bg flex items-center space-x-2 z-10 min-h-[64px] glass">
                    <input type="file" bind:this={fileInput} onchange={onFileSelect} class="hidden" />
                    
                    {#if isRecording}
                        <div class="flex-1 flex items-center justify-between bg-white/[0.03] backdrop-blur-md px-5 py-2 rounded-[1.5rem] border border-red-500/20 shadow-xl shadow-red-500/5 animate-in slide-in-from-bottom-2 duration-300 h-[52px]">
                            <div class="flex items-center space-x-4 flex-1 h-full">
                                <div class="relative flex items-center justify-center">
                                    <div class="w-3 h-3 bg-red-500 rounded-full animate-ping absolute opacity-40"></div>
                                    <div class="w-3 h-3 bg-red-500 rounded-full relative shadow-[0_0_12px_rgba(239,68,68,0.8)]"></div>
                                </div>
                                <div class="flex flex-col">
                                    <span class="text-[9px] font-black text-red-500 uppercase tracking-[0.2em] leading-none mb-0.5">Capturing</span>
                                    <span class="text-xs font-mono font-black text-entropy-text-primary tabular-nums">{formatRecordingTime(recordingSeconds)}</span>
                                </div>
                                <div class="flex-1 h-8 flex items-center bg-white/5 rounded-xl px-4 border border-white/5">
                                    <canvas bind:this={visualizerCanvas} width="200" height="32" class="w-full h-full opacity-80"></canvas>
                                </div>
                            </div>
                            <div class="flex items-center space-x-2 pl-3">
                                <button onclick={cancelRecording} class="p-2.5 text-entropy-text-dim hover:text-red-500 hover:bg-red-500/10 rounded-xl transition-all active:scale-90" title="Discard"><LucideTrash size={20} /></button>
                                <button onclick={stopRecording} class="w-10 h-10 bg-red-500 text-white rounded-xl shadow-[0_8px_16px_rgba(239,68,68,0.3)] hover:bg-red-600 transition-all active:scale-95 flex items-center justify-center group" title="Stop Recording">
                                    <LucideSquare size={16} fill="white" class="group-hover:scale-110 transition-transform" />
                                </button>
                            </div>
                        </div>
                    {:else if previewUrl}
                        <div class="flex-1 flex items-center space-x-3 bg-white/[0.03] backdrop-blur-md px-4 py-2.5 rounded-[1.5rem] border border-white/10 shadow-2xl animate-in zoom-in-95 duration-300">
                            <button onclick={cancelRecording} class="w-10 h-10 flex items-center justify-center text-entropy-text-dim hover:text-red-500 hover:bg-red-500/10 rounded-xl transition-all" title="Discard"><LucideTrash size={20} /></button>
                            <div class="flex-1">
                                <VoiceNotePlayer src={previewUrl} id="preview" isMine={true} />
                            </div>
                            <button onclick={sendRecordedVoiceNote} class="w-11 h-11 bg-entropy-primary text-white rounded-xl shadow-[0_8px_24px_rgba(139,92,246,0.3)] hover:bg-entropy-primary-dim active:scale-95 transition-all flex items-center justify-center group">
                                <LucideSend size={22} class="translate-x-0.5 group-hover:translate-x-1 group-hover:-translate-y-0.5 transition-transform" />
                            </button>
                        </div>
                    {:else}
                        <button onclick={() => fileInput?.click()} class="p-3 text-entropy-text-dim hover:bg-entropy-surface-light rounded-full transition"><LucidePaperclip size={24} /></button>
                        <textarea 
                            id="message-input"
                            bind:this={messageInputEl}
                            bind:value={messageInput}
                            onkeydown={handleKeydown}
                            rows="1"
                            class="flex-1 p-3 rounded-2xl border-none focus:ring-1 focus:ring-entropy-primary bg-entropy-surface-light text-entropy-text-primary resize-none max-h-[200px] overflow-y-auto custom-scrollbar placeholder:text-entropy-text-dim"
                            placeholder="Type a message" 
                        ></textarea>
                        {#if !messageInput.trim()}
                            <button onclick={startRecording} class="p-3 text-entropy-text-dim hover:bg-entropy-surface-light rounded-full shadow-sm"><LucideMic size={24} /></button>
                        {:else}
                            <button onclick={handleSend} class="p-3 bg-entropy-primary text-white rounded-full hover:bg-entropy-primary-dim shadow-lg active:scale-95 transition-transform"><LucideSend size={24} /></button>
                        {/if}
                    {/if}
                </div>
            {/if}
        </div>
        
        {#if showGallery}
            <div class="w-80 bg-entropy-bg flex flex-col animate-in slide-in-from-right duration-300 z-[40]">
                <div class="p-4 flex justify-between items-center bg-entropy-surface">
                    <h2 class="font-bold text-entropy-text-primary flex items-center space-x-2"><LucideInfo size={18} /><span>Info & Media</span></h2>
                    <button onclick={() => showGallery = false} class="text-entropy-text-dim hover:text-red-500 transition"><LucideX size={20} /></button>
                </div>
                
                <div class="p-6 flex-1 overflow-y-auto custom-scrollbar space-y-8">
                    <div class="flex flex-col items-center space-y-4">
                        <div class="w-24 h-24 rounded-3xl bg-entropy-surface flex items-center justify-center text-entropy-primary text-3xl font-bold shadow-xl">
                            {#if activeChat.pfp}<img src={activeChat.pfp} alt="" class="w-full h-full object-cover rounded-3xl" />{:else}{(activeChat.localNickname || activeChat.peerAlias || '?')[0].toUpperCase()}{/if}
                        </div>
                        <div class="text-center">
                            <div class="flex items-center justify-center space-x-2">
                                <h3 class="text-xl font-bold text-entropy-text-primary">{activeChat.localNickname || activeChat.peerAlias || 'Peer'}</h3>
                                {#if !activeChat.isGroup && activeChat.isVerified}
                                    <LucideShieldCheck size={18} class="text-entropy-accent" />
                                {/if}
                            </div>
                            {#if activeChat.localNickname && activeChat.peerAlias}
                                <p class="text-[10px] font-bold text-entropy-primary uppercase mb-1 tracking-wide">Alias: {activeChat.peerAlias}</p>
                            {/if}
                            <p class="text-[11px] font-mono text-entropy-text-secondary break-all opacity-80">{activeChat.peerHash}</p>
                        </div>
                    </div>

                    {#if !activeChat.isGroup}
                        <div class="space-y-4 pt-2">
                             <div class="flex items-center justify-between">
                                <h4 class="text-[10px] font-black text-entropy-text-dim uppercase tracking-[0.1em]">Identity Verification</h4>
                                 {#if activeChat.isVerified}
                                    <div class="flex items-center space-x-1 text-entropy-accent animate-in fade-in zoom-in duration-300">
                                        <LucideShieldCheck size={12} />
                                        <span class="text-[9px] font-black uppercase">Verified</span>
                                    </div>
                                {:else}
                                    <div class="flex items-center space-x-1 text-red-500/80">
                                        <LucideShieldAlert size={12} />
                                        <span class="text-[9px] font-black uppercase">Unverified</span>
                                    </div>
                                {/if}
                             </div>

                             <div class="bg-entropy-surface-light p-4 rounded-2xl border border-white/5 space-y-3">
                                {#if loadingSafetyNumber}
                                    <div class="flex flex-col items-center py-4 space-y-2">
                                        <LucideLoader size={20} class="animate-spin text-entropy-primary" />
                                        <span class="text-[10px] text-entropy-text-dim uppercase font-bold tracking-widest">Generating Fingerprint...</span>
                                    </div>
                                {:else if safetyNumber}
                                    <div class="grid grid-cols-2 gap-x-4 gap-y-2 font-mono text-[11px] text-entropy-text-primary text-center opacity-90 leading-relaxed bg-black/5 dark:bg-white/5 p-3 rounded-xl border border-white/5">
                                        {#each safetyNumber.digits.split('\n') as line}
                                            {#each line.split(' ') as part}
                                                <div class="tracking-widest">{part}</div>
                                            {/each}
                                        {/each}
                                    </div>

                                    <div class="pt-2">
                                        <button 
                                            onclick={async () => {
                                                const verified = !activeChat!.isVerified;
                                                await toggleVerification(activeChat!.peerHash, verified);
                                                // Sync local safetyNumber state to prevent flicker before store update propagates
                                                if (safetyNumber) safetyNumber.isVerified = verified;
                                                addToast(verified ? "Session Verified" : "Verification Removed", verified ? 'success' : 'info');
                                            }}
                                            class="w-full py-2.5 rounded-xl text-[10px] font-black uppercase tracking-widest transition-all active:scale-[0.98] shadow-lg
                                            {activeChat.isVerified 
                                                ? 'bg-red-500/10 text-red-500 hover:bg-red-500/20' 
                                                : 'bg-entropy-accent text-white hover:bg-entropy-accent/90 shadow-entropy-accent/20'}"
                                        >
                                            {activeChat.isVerified ? 'Remove Verification' : 'Verify Identity'}
                                        </button>
                                        <p class="text-[9px] text-entropy-text-dim text-center mt-3 leading-relaxed px-2">
                                            Verify the safety number above with this contact via another secure channel.
                                        </p>
                                    </div>
                                {:else}
                                    <div class="text-[10px] text-center text-red-500/80 font-bold py-2">Encryption session not established.</div>
                                {/if}
                             </div>
                        </div>
                    {/if}
                    
                    {#if activeChat.isGroup}
                        <div class="space-y-2">
                            <h4 class="text-xs font-bold text-entropy-text-dim uppercase tracking-widest">Group Invite Link</h4>
                            <div class="bg-entropy-surface-light p-3 rounded-xl flex items-center justify-between">
                                <span class="text-[11px] font-mono text-entropy-primary truncate">entropy://{activeChat.peerHash.slice(0, 32)}...</span>
                                <button onclick={copyInvite} class="p-2 hover:bg-entropy-primary/10 rounded-lg text-entropy-primary transition">
                                    {#if inviteCopied}<LucideCheckIcon size={16} />{:else}<LucideCopy size={16} />{/if}
                                </button>
                            </div>
                        </div>

                        <div class="space-y-2">
                             <div class="flex justify-between items-center">
                                <h4 class="text-xs font-bold text-entropy-text-dim uppercase tracking-widest">Members ({activeChat.members?.length || 0})</h4>
                                <button onclick={async () => {
                                    let input = await showPrompt("Enter Peer Hash, Nickname, or link:", "", "Add Member");
                                    if (input) {
                                        input = input.trim().replace(/^entropy:\/\//, '');
                                        if (input.length === 64) {
                                            addToGroup(activeChat!.peerHash, [input]);
                                            addToast("Invitation sent!", 'success');
                                        } else {
                                            addToast("Please provide a full 64-char ID for now.", 'info');
                                        }
                                    }
                                }} class="text-[10px] font-black text-entropy-primary hover:underline uppercase">Add Member</button>
                             </div>
                             <div class="space-y-1 max-h-40 overflow-y-auto custom-scrollbar pr-1">
                                {#each activeChat.members || [] as member}
                                    <div class="flex items-center space-x-2 bg-entropy-surface-light p-2 rounded-lg">
                                        <div class="w-5 h-5 rounded-md bg-entropy-primary/20 flex items-center justify-center text-[8px] font-bold text-entropy-primary">
                                            {member.slice(0, 2).toUpperCase()}
                                        </div>
                                        <span class="text-[10px] font-mono text-entropy-text-secondary truncate flex-1">{member.slice(0, 16)}...</span>
                                        {#if member === $userStore.identityHash}
                                            <span class="text-[8px] font-black bg-entropy-primary/10 text-entropy-primary px-1 rounded">YOU</span>
                                        {/if}
                                    </div>
                                {/each}
                             </div>
                        </div>
                    {/if}

                    <div class="space-y-4">
                        <div class="flex justify-between items-center">
                            <h4 class="text-xs font-bold text-entropy-text-dim uppercase tracking-wider">Shared Media</h4>
                            <LucideImage size={14} class="text-entropy-text-dim" />
                        </div>
                        {#if mediaMessages.length === 0}
                            <div class="bg-entropy-surface-light rounded-2xl h-32 flex items-center justify-center text-xs text-entropy-text-dim">No media shared yet</div>
                        {:else}
                            <div class="grid grid-cols-3 gap-2">
                                {#each mediaMessages.slice(0, 9) as m}
                                    <MediaThumbnail msg={m} onclick={() => scrollToMessage(m.id)} />
                                {/each}
                            </div>
                        {/if}
                    </div>

                    <div class="space-y-4">
                        <div class="flex justify-between items-center">
                            <h4 class="text-xs font-bold text-entropy-text-dim uppercase tracking-wider">Shared Links</h4>
                            <LucideLink size={14} class="text-entropy-text-dim" />
                        </div>
                        {#if linkMessages.length === 0}
                            <div class="bg-entropy-surface-light rounded-2xl h-24 flex items-center justify-center text-xs text-entropy-text-dim">No links shared yet</div>
                        {:else}
                            <div class="space-y-2">
                                 {#each linkMessages.slice(0, 5) as m}
                                    <a href={m.linkPreview!.url} target="_blank" rel="noopener noreferrer" class="block p-3 bg-entropy-surface-light hover:bg-entropy-surface rounded-xl transition duration-200 group/link">
                                        <div class="flex items-start space-x-2">
                                            <LucideExternalLink size={12} class="text-entropy-accent shrink-0 mt-0.5 group-hover/link:text-entropy-primary transition" />
                                            <div class="flex-1 min-w-0">
                                                <div class="text-[11px] font-bold text-entropy-text-primary truncate">{m.linkPreview!.title || m.linkPreview!.url}</div>
                                                <div class="text-[10px] text-entropy-text-secondary truncate mt-0.5">{m.linkPreview!.url}</div>
                                                {#if m.linkPreview!.description}
                                                    <div class="text-[9px] text-entropy-text-dim line-clamp-2 mt-1 leading-relaxed">{m.linkPreview!.description}</div>
                                                {/if}
                                            </div>
                                        </div>
                                    </a>
                                 {/each}
                            </div>
                        {/if}
                    </div>

                    {#if activeChat.isGroup}
                        <div class="pt-4 border-t border-entropy-border/10">
                            <button 
                                onclick={async () => { if (await showConfirm("Are you sure you want to leave this group? All history will be deleted.", "Leave Group")) { leaveGroup(activeChat!.peerHash); showGallery = false; } }}
                                class="w-full flex items-center justify-center space-x-2 p-3 rounded-xl bg-red-500/10 text-red-500 hover:bg-red-500/20 font-bold text-xs transition active:scale-[0.98]"
                            >
                                <LucideTrash2 size={16} />
                                <span>Leave Group</span>
                            </button>
                        </div>
                    {/if}
                </div>
            </div>
        {/if}
    </div>
{/if}

<style>
    .custom-scrollbar::-webkit-scrollbar { width: 5px; }
    .custom-scrollbar::-webkit-scrollbar-thumb { background: rgba(0,0,0,0.1); border-radius: 10px; }
    
    .chat-bg {
        position: relative;
        background-color: rgb(var(--entropy-bg));
        isolation: isolate;
    }
    
    .chat-bg::before {
        content: '';
        position: absolute;
        inset: 0;
        background-image: url("data:image/svg+xml,%3Csvg viewBox='0 0 200 200' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='noiseFilter'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='3' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23noiseFilter)'/%3E%3C/svg%3E");
        opacity: 0.25;
        pointer-events: none;
        z-index: -1;
    }
    
    :global(.theme-dark) .chat-bg::before {
        opacity: 1;
        background-image: 
            radial-gradient(circle, rgba(255, 255, 255, 0.05) 1px, transparent 1px);
        background-size: 20px 20px;
    }
    
    @keyframes bounce-subtle {
        0%, 100% { transform: translateY(0); }
        50% { transform: translateY(-10px); }
    }
    .animate-bounce-subtle {
        animation: bounce-subtle 3s ease-in-out infinite;
    }
</style>

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
