<script lang="ts">
  import { userStore, messageStore } from '../lib/stores/user';
  import { toggleStar, setReplyingTo, toggleBlock } from '../lib/store';
  import { loadMoreMessages } from '../lib/actions/message_utils';
  import { LucideSearch, LucideX, LucideInfo, LucideLoader, LucideChevronDown, LucideBan } from 'lucide-svelte';
  
  import MessageBubble from './MessageBubble.svelte';
  import StarredMessages from './StarredMessages.svelte';
  import ContactProfile from './ContactProfile.svelte';
  import ChatHeader from './ChatHeader.svelte';
  import MessageInputArea from './MessageInputArea.svelte';
  
  import { tick } from 'svelte';
  import { playingVoiceNoteId } from '../lib/stores/audio';
  
  let { showStarredMessages = false, onCloseStarred }: { showStarredMessages?: boolean; onCloseStarred?: () => void } = $props();
  
  let messageSearchQuery = $state("");
  let showMessageSearch = $state(false);
  let scrollContainer = $state<HTMLElement | null>(null);
  let showGallery = $state(false);

  let selectionMode = $state(false);
  let selectedIds = $state<string[]>([]);
  let viewingImage = $state<string | null>(null);
  let isLoadingMore = $state(false);
  let lastScrollHeight = 0;
  let showScrollToBottom = $state(false);

  let activeChat = $derived($userStore.activeChatHash ? $userStore.chats[$userStore.activeChatHash] : null);
  let activeMessages = $derived($userStore.activeChatHash ? ($messageStore[$userStore.activeChatHash] || []) : []);
  
  let virtualizedMessages = $derived(activeMessages.slice(-300));

  const scrollToBottom = async () => {
      await tick();
      if (scrollContainer && !selectionMode) {
          scrollContainer.scrollTo({ top: scrollContainer.scrollHeight, behavior: 'instant' });
      }
  };

  $effect(() => {
    if (activeMessages.length > 0) {
      if (!isLoadingMore) {
          scrollToBottom();
      }
    }
  });

  const handleScroll = async (e: Event) => {
      const target = e.target as HTMLElement;
      if (!activeChat || isLoadingMore || !activeChat.hasMore) return;

      if (target.scrollTop < 50) {
          isLoadingMore = true;
          lastScrollHeight = target.scrollHeight;
          
          const loadedCount = await loadMoreMessages(activeChat.peerHash);
          
          if (loadedCount && loadedCount > 0) {
              await tick();
              target.scrollTop = target.scrollHeight - lastScrollHeight;
          }
          
          isLoadingMore = false;
      }

      // show scroll to bottom if more than 300px from bottom
      const threshold = 300;
      const distanceFromBottom = target.scrollHeight - target.scrollTop - target.clientHeight;
      showScrollToBottom = distanceFromBottom > threshold;
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
</script>

{#if showStarredMessages}
    <StarredMessages onClose={() => onCloseStarred && onCloseStarred()} onSelectChat={(hash, msgId) => {
        userStore.update(s => ({ ...s, activeChatHash: hash }));
        if (onCloseStarred) onCloseStarred();
        setTimeout(() => scrollToMessage(msgId), 150);
    }}/>
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
        <div class="flex-1 flex flex-col relative h-full min-w-0 chat-window-container">
            
            <ChatHeader 
                {activeChat}
                {selectionMode}
                bind:selectedIds
                {showMessageSearch}
                onViewImage={() => activeChat && activeChat.pfp && (viewingImage = activeChat.pfp)}
                onCancelSelection={() => {selectionMode = false; selectedIds = [];}}
                onToggleSearch={() => showMessageSearch = !showMessageSearch}
                onShowGallery={() => showGallery = true}
                onSelectionModeChange={(mode) => {selectionMode = mode; if (mode) selectedIds = [];}}
            />
            
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
            
            <div 
                bind:this={scrollContainer} 
                onscroll={handleScroll}
                class="flex-1 overflow-y-auto p-4 space-y-3 custom-scrollbar bg-transparent relative z-10"
                style="overflow-anchor: auto;"
            >
                {#if isLoadingMore}
                    <div class="flex justify-center py-2 animate-pulse">
                        <div class="flex items-center space-x-2 bg-entropy-surface-light px-3 py-1 rounded-full border border-entropy-border/10">
                            <LucideLoader size={12} class="animate-spin text-entropy-primary" />
                            <span class="text-[10px] font-bold text-entropy-text-dim uppercase tracking-widest">Fetching History...</span>
                        </div>
                    </div>
                {/if}

                {#each virtualizedMessages.filter(m => !messageSearchQuery || m.content.toLowerCase().includes(messageSearchQuery.toLowerCase())) as msg (msg.id)}
                    {#if msg.type === 'system'}
                        <div class="flex justify-center my-4">
                            <div class="bg-entropy-surface-light px-4 py-1.5 rounded-full border border-entropy-border/5 flex items-center space-x-2 shadow-sm">
                                <LucideInfo size={12} class="text-entropy-primary" />
                                <span class="text-[11px] font-bold text-entropy-text-dim uppercase tracking-widest">{msg.content}</span>
                            </div>
                        </div>
                    {:else if activeChat}
                        <MessageBubble 
                            {msg} 
                            {activeChat} 
                            {selectionMode} 
                            {selectedIds} 
                            toggleSelect={toggleSelect}
                            scrollToMessage={scrollToMessage}
                            setReplyingTo={setReplyingTo}
                            toggleStar={toggleStar}
                        />
                    {/if}
                {/each}

                {#if activeChat.isTyping}
                    <div class="flex items-center space-x-2 p-2 px-4 py-2.5 bg-entropy-surface-light/80 backdrop-blur-md rounded-2xl w-fit animate-in fade-in slide-in-from-bottom-2 duration-300 ml-4 mb-4 border border-entropy-border/5 shadow-sm">
                        <div class="flex space-x-1">
                            <div class="w-1.5 h-1.5 bg-entropy-primary rounded-full animate-bounce [animation-delay:-0.3s]"></div>
                            <div class="w-1.5 h-1.5 bg-entropy-primary rounded-full animate-bounce [animation-delay:-0.15s]"></div>
                            <div class="w-1.5 h-1.5 bg-entropy-primary rounded-full animate-bounce"></div>
                        </div>
                    </div>
                {/if}
            </div>
            
            {#if showScrollToBottom}
                <button 
                    onclick={scrollToBottom}
                    class="absolute bottom-24 right-6 p-3 bg-entropy-surface/90 backdrop-blur-xl border border-entropy-border/10 rounded-full shadow-2xl text-entropy-primary hover:scale-110 active:scale-95 transition-all z-20 group"
                    aria-label="Scroll to bottom"
                >
                    <LucideChevronDown size={24} strokeWidth={2.5} />
                    {#if activeChat.unreadCount > 0}
                        <span class="absolute -top-1 -right-1 bg-red-500 text-white text-[10px] font-black w-5 h-5 flex items-center justify-center rounded-full border-2 border-entropy-bg animate-bounce">
                            {activeChat.unreadCount}
                        </span>
                    {/if}
                </button>
            {/if}
            <MessageInputArea {activeChat} />
        </div>
        
        {#if showGallery}
            <ContactProfile 
                {activeChat} 
                onClose={() => showGallery = false} 
                onScrollToMessage={scrollToMessage} 
            />
        {/if}
    </div>
{/if}

<style>
    .custom-scrollbar::-webkit-scrollbar { width: 5px; }
    .custom-scrollbar::-webkit-scrollbar-thumb { background: rgba(0,0,0,0.1); border-radius: 10px; }
    
    .chat-window-container {
        position: relative;
        background-color: rgb(var(--entropy-bg));
        isolation: isolate;
        overflow: hidden;
    }
    
    .chat-window-container::before {
        content: '';
        position: absolute;
        inset: 0;
        background-image: url("data:image/svg+xml,%3Csvg viewBox='0 0 200 200' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='noiseFilter'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='3' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23noiseFilter)'/%3E%3C/svg%3E");
        opacity: 0.25;
        pointer-events: none;
        z-index: 0;
    }
    
    :global(.theme-dark) .chat-window-container::before {
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
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div 
        class="fixed inset-0 z-[100] flex items-center justify-center bg-black/90 backdrop-blur-md p-4 animate-in fade-in duration-300" 
        onclick={() => viewingImage = null}
        onkeydown={(e) => e.key === 'Escape' && (viewingImage = null)}
        role="button"
        tabindex="0"
        aria-label="Close preview"
    >
        <div class="relative max-w-4xl max-h-[90vh] flex flex-col items-center" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()} role="presentation">
            <button class="absolute -top-12 right-0 p-2 text-white/60 hover:text-white transition" onclick={() => viewingImage = null}>
                <LucideX size={32} />
            </button>
            <img src={viewingImage} alt="Preview" class="max-w-full max-h-full rounded-2xl shadow-2xl animate-in zoom-in-95 duration-300 pointer-events-none" />
        </div>
    </div>
{/if}
