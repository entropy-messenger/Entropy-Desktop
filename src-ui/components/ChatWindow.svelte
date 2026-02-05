<script lang="ts">
  import { userStore } from '../lib/stores/user';
  import { 
    sendMessage, sendFile, sendVoiceNote, 
    sendTypingStatus, setLocalNickname, toggleStar, 
    setDisappearingTimer, setReplyingTo,
    bulkDelete, bulkStar, toggleBlock, toggleVerification 
  } from '../lib/store';
  import { signalManager } from '../lib/signal_manager';
  import { 
    LucideX, LucideSearch, 
    LucideClock, LucideBellOff, LucideTrash2,
    LucideExternalLink, LucideImage, LucideLink, LucideFile, LucideInfo,
    LucideCopy, LucideCheck as LucideCheckIcon, LucideShare2, LucideBan,
    LucideShieldCheck, LucideShieldAlert
  } from 'lucide-svelte';
  
  import ChatHeader from './chat/ChatHeader.svelte';
  import MessageList from './chat/MessageList.svelte';
  import MessageInput from './chat/MessageInput.svelte';
  import AttachmentRenderer from './AttachmentRenderer.svelte';
  
  let messageSearchQuery = $state("");
  let showMessageSearch = $state(false);
  let showOptions = $state(false);
  let showGallery = $state(false);
  let selectionMode = $state(false);
  let selectedIds = $state<string[]>([]);
  let isRecording = $state(false);
  let mediaRecorder = $state<MediaRecorder | null>(null);
  let audioChunks = $state<Blob[]>([]);
  let recordedBlob = $state<Blob | null>(null);
  let lastActiveHash = ""; // Non-reactive tracker to avoid recursive effect loops

  let activeChat = $derived($userStore.activeChatHash ? $userStore.chats[$userStore.activeChatHash] : null);
  let replyingTo = $derived($userStore.replyingTo);
  let safetyNumber = $state("");
  
  import { untrack } from 'svelte';

  $effect(() => {
    // Only react to changes in the active chat hash
    const chatHash = $userStore.activeChatHash || "";
    
    untrack(() => {
      if (chatHash !== lastActiveHash) {
          console.log("Chat switched to", chatHash, "- resetting VN state");
          lastActiveHash = chatHash;
          
          if (isRecording) {
              try { mediaRecorder?.stop(); } catch (e) {}
              isRecording = false;
          }
          recordedBlob = null;
          audioChunks = [];
      }
    });
  });


  const handleSend = (text: string) => {
    if (activeChat) sendMessage(activeChat.peerHash, text);
  };
  
  const onFileSelect = (file: File) => {
    if (activeChat) sendFile(activeChat.peerHash, file);
  };

  const toggleRecording = async () => {
    if (isRecording) {
        mediaRecorder?.stop();
        isRecording = false;
    } else {
        try {
            const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
            
            // Prefer webm, then ogg, then default
            const mimeType = ['audio/webm', 'audio/ogg', 'audio/mp4'].find(t => MediaRecorder.isTypeSupported(t)) || '';
            console.log("Starting recorder with MIME type:", mimeType || "default");
            
            mediaRecorder = new MediaRecorder(stream, mimeType ? { mimeType } : {});
            audioChunks = [];
            
            mediaRecorder.ondataavailable = (e) => { 
                if (e.data.size > 0) {
                    audioChunks.push(e.data);
                } 
            };
            
            mediaRecorder.onstop = () => {
                const actualMime = mediaRecorder?.mimeType || 'audio/webm';
                recordedBlob = new Blob(audioChunks, { type: actualMime });
                console.log("Recording stopped. Blob size:", recordedBlob.size, "MIME:", actualMime);
                
                stream.getTracks().forEach(track => track.stop());
            };
            
            mediaRecorder.start();
            isRecording = true;
        } catch (e: any) { 
            console.error("Recording error:", e); 
            if (e.name === 'NotAllowedError') {
                alert("Microphone permission denied. Please enable it in your system settings.");
            } else {
                alert(`Could not start recording: ${e.message || e}`);
            }
        }
    }
  };


  const discardRecording = () => {
    recordedBlob = null;
    audioChunks = [];
  };

  const sendRecording = (duration?: number) => {
    if (activeChat && recordedBlob && recordedBlob.size > 0) {
        sendVoiceNote(activeChat.peerHash, recordedBlob, duration);
        discardRecording();
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
  const handleBulkDelete = () => { 
    if (activeChat && confirm(`Delete ${selectedIds.length} messages?`)) { 
        bulkDelete(activeChat.peerHash, selectedIds); cancelSelection(); 
    } 
  };

  let mediaMessages = $derived(activeChat ? activeChat.messages.filter((m: any) => m.type === 'file' || m.type === 'voice_note') : []);
  let linkMessages = $derived(activeChat ? activeChat.messages.filter((m: any) => m.linkPreview) : []);

  let inviteCopied = $state(false);
  const copyInvite = () => {
      if (!activeChat) return;
      const link = `entropy://${activeChat.peerHash}`;
      navigator.clipboard.writeText(link);
      inviteCopied = true;
      setTimeout(() => inviteCopied = false, 2000);
  };
</script>

{#if !activeChat}
    <div class="h-full w-full flex items-center justify-center bg-[#f0f2f5] flex-col text-center p-8">
        <div class="w-20 h-20 bg-white rounded-3xl flex items-center justify-center mb-6 shadow-xl border border-black/5 animate-bounce-subtle">
            <img src="/logo.png" alt="logo" class="w-16 h-16 object-contain" />
        </div>
        <h2 class="text-2xl font-black text-gray-900 mb-2 tracking-tighter uppercase">Entropy</h2>
        <p class="text-gray-500 max-w-sm font-bold text-[10px] leading-relaxed opacity-40 uppercase tracking-widest pl-1">Select a conversation to start messaging.<br>All signals are end-to-end encrypted.</p>
    </div>
{:else}
    <div class="h-full w-full flex bg-[#efeae2] relative overflow-hidden">
        
        <div class="flex-1 flex flex-col relative h-full min-w-0">
            
            <ChatHeader 
                {activeChat} 
                onToggleGallery={() => showGallery = !showGallery}
                onToggleSearch={() => showMessageSearch = !showMessageSearch}
                onShowOptions={() => showOptions = !showOptions}
            />

            {#if showMessageSearch}
                <div class="bg-white p-3 px-4 border-b border-gray-200 flex items-center space-x-3 shadow-sm z-20 animate-in slide-in-from-top duration-200">
                    <div class="flex-1 relative">
                        <input 
                            bind:value={messageSearchQuery}
                            placeholder="Search in this chat..."
                            class="w-full bg-gray-100 border-none rounded-xl py-2 px-10 text-xs focus:ring-2 focus:ring-indigo-500/20"
                        />
                        <LucideSearch size={14} class="absolute left-4 top-1/2 -translate-y-1/2 text-gray-400" />
                    </div>
                    <button onclick={() => { showMessageSearch = false; messageSearchQuery = ""; }} class="p-2 text-gray-400 hover:text-indigo-600 transition">
                        <LucideX size={18} />
                    </button>
                </div>
            {/if}

            <MessageList 
                messages={messageSearchQuery ? activeChat.messages.filter(m => (m.content || "").toLowerCase().includes(messageSearchQuery.toLowerCase())) : activeChat.messages}
                {activeChat}
                {selectionMode}
                {selectedIds}
                onToggleSelect={toggleSelect}
                onScrollToMessage={scrollToMessage}
                onSetReplyingTo={(msg) => setReplyingTo(msg)}
            />

            {#if selectionMode}
                <div class="absolute bottom-6 left-1/2 -translate-x-1/2 bg-white/90 backdrop-blur-md px-6 py-3 rounded-2xl shadow-2xl border border-indigo-100 flex items-center space-x-8 z-40 animate-in zoom-in-95 duration-200">
                    <div class="text-xs font-black uppercase tracking-widest text-indigo-600 border-r border-indigo-100 pr-8">{selectedIds.length} Selected</div>
                    <button onclick={handleBulkStar} class="flex flex-col items-center space-y-1 text-amber-500 hover:scale-110 transition active:scale-95">
                        <div class="p-2 bg-amber-50 rounded-xl"><LucideClock size={18} /></div>
                        <span class="text-[9px] font-bold uppercase">Star</span>
                    </button>
                    <button onclick={handleBulkDelete} class="flex flex-col items-center space-y-1 text-red-500 hover:scale-110 transition active:scale-95">
                        <div class="p-2 bg-red-50 rounded-xl"><LucideTrash2 size={18} /></div>
                        <span class="text-[9px] font-bold uppercase">Delete</span>
                    </button>
                    <button onclick={cancelSelection} class="p-2 text-gray-400 hover:text-gray-900 transition"><LucideX size={20} /></button>
                </div>
            {/if}

            <MessageInput 
                peerHash={activeChat.peerHash}
                {replyingTo}
                onCancelReply={() => setReplyingTo(null)}
                onSend={handleSend}
                {onFileSelect}
                onToggleRecording={toggleRecording}
                {isRecording}
                {recordedBlob}
                onDiscardRecording={discardRecording}
                onSendRecording={sendRecording}
            />

        </div>

        {#if showGallery}
            <div class="w-[380px] bg-white border-l border-gray-200 flex flex-col h-full z-40 shadow-2xl animate-in slide-in-from-right duration-300">
                <div class="p-6 border-b border-gray-100 flex items-center justify-between bg-white sticky top-0 z-10">
                    <h3 class="font-black text-xs uppercase tracking-[0.2em] text-gray-400">Contact Info</h3>
                    <button onclick={() => showGallery = false} class="p-2 text-gray-400 hover:text-gray-800 transition hover:bg-gray-100 rounded-xl"><LucideX size={20} /></button>
                </div>

                <div class="flex-1 overflow-y-auto custom-scrollbar">
                    <div class="p-8 flex flex-col items-center text-center space-y-4">
                        <div class="w-32 h-32 rounded-[2.5rem] bg-gradient-to-tr from-indigo-500 to-purple-600 shadow-2xl flex items-center justify-center text-white text-4xl font-black relative overflow-hidden ring-4 ring-indigo-50">
                            {#if activeChat.pfp}
                                <img src={activeChat.pfp} alt="" class="w-full h-full object-cover" />
                            {:else}
                                <span>{(activeChat.localNickname || activeChat.peerAlias || "?")[0].toUpperCase()}</span>
                            {/if}
                        </div>
                        <div class="space-y-1">
                            <h2 class="text-xl font-black text-gray-900 flex items-center justify-center space-x-2">
                                <span>{activeChat.localNickname || activeChat.peerAlias}</span>
                                {#if activeChat.isVerified}
                                    <LucideShieldCheck size={20} class="text-emerald-500" />
                                {/if}
                            </h2>
                            <p class="text-[10px] font-bold uppercase tracking-widest text-gray-400">Public Protocol Hash</p>
                        </div>
                    </div>

                    <div class="px-6 space-y-2 mb-8">
                        <div class="bg-gray-50 p-4 rounded-2xl border border-gray-100 break-all select-all font-mono text-[10px] leading-relaxed text-gray-500 relative group">
                            {activeChat.peerHash}
                            <button 
                                onclick={copyInvite} 
                                class="absolute top-2 right-2 p-1.5 bg-white shadow-sm rounded-lg opacity-0 group-hover:opacity-100 transition"
                            >
                                <LucideCopy size={12} />
                            </button>
                            {#if inviteCopied}
                                <span class="absolute -top-8 right-0 bg-black text-white text-[10px] px-2 py-1 rounded">Copied!</span>
                            {/if}
                        </div>
                    </div>

                </div>
            </div>
        {/if}
    </div>
{/if}

<style>
    .custom-scrollbar::-webkit-scrollbar { width: 4px; }
    .custom-scrollbar::-webkit-scrollbar-track { background: transparent; }
    .custom-scrollbar::-webkit-scrollbar-thumb { background: rgba(0,0,0,0.05); border-radius: 10px; }
</style>
