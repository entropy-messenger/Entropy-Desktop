<script lang="ts">
  import { userStore, messageStore } from '../lib/stores/user';
  import { toggleStar, toggleBlock } from '../lib/actions/contacts';
  import { setReplyingTo, loadChatMessages, loadMoreMessages, loadNewerMessages, jumpToMessage, jumpToPresent, sendReceipt } from '../lib/actions/chat';
  import { LucideSearch, LucideX, LucideInfo, LucideLoader, LucideChevronDown, LucideBan } from 'lucide-svelte';
  
  import MessageBubble from './MessageBubble.svelte';
  import StarredMessages from './StarredMessages.svelte';
  import ContactProfile from './ContactProfile.svelte';
  import ChatHeader from './ChatHeader.svelte';
  import MessageInputArea from './MessageInputArea.svelte';
  
  import { tick } from 'svelte';
  import { playingVoiceNoteId } from '../lib/stores/audio';
  
  let { showStarredMessages = $bindable(false), onCloseStarred, isMobile }: { showStarredMessages?: boolean; onCloseStarred?: () => void; isMobile?: boolean } = $props();
  
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

  const getDayLabel = (ts: number) => {
    const date = new Date(ts);
    const today = new Date();
    const yesterday = new Date();
    yesterday.setDate(today.getDate() - 1);

    if (date.toDateString() === today.toDateString()) return "Today";
    if (date.toDateString() === yesterday.toDateString()) return "Yesterday";
    
    const options: Intl.DateTimeFormatOptions = { month: 'long', day: 'numeric' };
    if (date.getFullYear() !== today.getFullYear()) options.year = 'numeric';
    return date.toLocaleDateString(undefined, options);
  };

  let groupedMessages = $derived.by(() => {
    const msgs = activeMessages.filter(m => !messageSearchQuery || m.content.toLowerCase().includes(messageSearchQuery.toLowerCase()));
    const result: (any)[] = [];
    let lastDateLabel = "";

    for (const msg of msgs) {
        const dateLabel = getDayLabel(msg.timestamp);
        if (dateLabel !== lastDateLabel) {
            result.push({ type: 'date_divider', content: dateLabel, id: `divider-${msg.timestamp}` });
            lastDateLabel = dateLabel;
        }
        result.push(msg);
    }
    return result;
  });

  const scrollToBottom = async () => {
      await tick();
      if (scrollContainer && !selectionMode) {
          // If we were in a historical context, reloading chat messages resets back to current
          const state = $userStore;
          if (state.activeChatHash && state.chats[state.activeChatHash]?.hasMoreNewer) {
              await jumpToPresent(state.activeChatHash);
          }
          scrollContainer.scrollTo({ top: scrollContainer.scrollHeight, behavior: 'instant' });
      }
  };

  $effect(() => {
    if (activeMessages.length > 0 && activeChat && $userStore.activeChatHash === activeChat.peerHash) {
        // Reset local unread count for current active chat
        if (activeChat.unreadCount > 0) {
            userStore.update(s => {
                const c = s.chats[activeChat.peerHash];
                if (c) s.chats[activeChat.peerHash] = { ...c, unreadCount: 0 };
                return { ...s, chats: { ...s.chats } };
            });
        }

        // Only send network-level Signal receipts for 1:1 chats
        if (activeChat.isGroup) return;

        const unreadIncoming = activeMessages.filter(m => !m.isMine && m.status !== 'read');
        if (unreadIncoming.length > 0) {
            const ids = unreadIncoming.map(m => m.id);
            sendReceipt(activeChat.peerHash, ids, 'read');
        }
    }
  });

  let lastMessageCount = 0;
  $effect(() => {
    // Only auto-scroll if the NUMBER of messages increased (new message sent/received)
    // Avoid jumps for background status updates or starring.
    if (activeMessages.length > lastMessageCount) {
        if (!isLoadingMore && !activeChat?.hasMoreNewer) {
            scrollToBottom();
        }
    }
    lastMessageCount = activeMessages.length;
  });

  const handleScroll = async (e: Event) => {
      const target = e.target as HTMLElement;
      if (!activeChat) return;

      // Pagination: Show "load more" if we hit the top and have more to load
      if (target.scrollTop < 50 && !isLoadingMore && activeChat.hasMore) {
          isLoadingMore = true;
          lastScrollHeight = target.scrollHeight;
          const loadedCount = await loadMoreMessages(activeChat.peerHash);
          if (loadedCount && loadedCount > 0) {
              await tick();
              target.scrollTop = target.scrollHeight - lastScrollHeight;
          }
          isLoadingMore = false;
      }

      // Newer Messages: Load if we hit bottom and have "future" context
      const isNearBottom = target.scrollHeight - target.scrollTop - target.clientHeight < 50;
      if (isNearBottom && !isLoadingMore && activeChat.hasMoreNewer) {
          isLoadingMore = true;
          await loadNewerMessages(activeChat.peerHash);
          isLoadingMore = false;
      }

      // Visibility: Always show scroll-to-bottom if we are far enough from the floor or in history mode
      const threshold = 100;
      const distanceFromBottom = target.scrollHeight - target.scrollTop - target.clientHeight;
      showScrollToBottom = distanceFromBottom > threshold || !!activeChat.hasMoreNewer;
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
    <StarredMessages onClose={() => onCloseStarred && onCloseStarred()} onSelectChat={async (hash, msgId) => {
        userStore.update(s => ({ ...s, activeChatHash: hash }));
        if (onCloseStarred) onCloseStarred();
        await jumpToMessage(hash, msgId); 
        await tick(); 
        scrollToMessage(msgId); 
    }}/>
{:else if !activeChat}
    <div class="h-full w-full flex items-center justify-center bg-entropy-bg flex-col text-center p-8">
        <div class="w-20 h-20 bg-entropy-surface rounded-3xl flex items-center justify-center mb-6 shadow-xl animate-bounce-subtle">
            <img src="/logo.png" alt="logo" class="w-16 h-16 object-contain" />
        </div>
        <h2 class="text-2xl font-bold text-entropy-text-primary mb-2 tracking-tight">Entropy</h2>
        <p class="text-entropy-text-dim max-w-sm font-medium text-xs leading-relaxed opacity-60">Select a conversation to start messaging.</p>
    </div>
{:else}
    <div class="h-full w-full flex bg-entropy-bg relative overflow-hidden">
        <div class="flex-1 flex flex-col relative h-full min-w-0 chat-window-container">
            
            <ChatHeader 
                {activeChat}
                {selectionMode}
                bind:selectedIds
                {showMessageSearch}
                onViewImage={() => {}}
                onCancelSelection={() => {selectionMode = false; selectedIds = [];}}
                onToggleSearch={() => showMessageSearch = !showMessageSearch}
                onShowGallery={() => showGallery = true}
                onSelectionModeChange={(mode) => {selectionMode = mode; if (mode) selectedIds = [];}}
                {isMobile}
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

                {#each groupedMessages as msg (msg.id)}
                    {#if msg.type === 'date_divider'}
                        <div class="flex items-center justify-center py-6 px-12 animate-in fade-in duration-500">
                            <div class="h-[1px] flex-1 bg-gradient-to-r from-transparent via-entropy-border/30 to-transparent"></div>
                            <span class="mx-6 text-[10px] font-black text-entropy-primary uppercase tracking-[0.2em]">{msg.content}</span>
                            <div class="h-[1px] flex-1 bg-gradient-to-r from-transparent via-entropy-border/30 to-transparent"></div>
                        </div>
                    {:else if msg.type === 'system'}
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
                            {isMobile}
                        />
                    {/if}
                {/each}

                {#if activeChat.isTyping && $userStore.privacySettings.typingStatus !== 'nobody'}
                    <div class="flex items-center space-x-2 p-2 px-4 py-2.5 bg-entropy-surface-light/80 backdrop-blur-md rounded-2xl w-fit animate-in fade-in slide-in-from-bottom-2 duration-300 ml-4 mb-4 shadow-sm">
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
                    class="absolute bottom-32 right-8 p-2 bg-entropy-surface/90 backdrop-blur-2xl border border-entropy-primary/20 rounded-full shadow-2xl text-entropy-primary hover:scale-110 active:scale-95 transition-all z-[999] group ring-4 ring-black/5 animate-in fade-in slide-in-from-bottom-2 duration-300"
                    aria-label="Scroll to bottom"
                >
                    <LucideChevronDown size={20} strokeWidth={3} />
                    {#if activeChat.unreadCount > 0}
                        <span class="absolute -top-1.5 -right-1.5 bg-red-600 text-white text-[9px] font-black min-w-[18px] h-4.5 px-1 flex items-center justify-center rounded-full border-2 border-entropy-bg shadow-lg">
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
