<script lang="ts">
  import { userStore, messageStore } from '../lib/stores/user';
  import { LucideStar, LucideX } from 'lucide-svelte';
  import MessageContent from './MessageContent.svelte';

  let { onClose, onSelectChat } = $props<{ 
      onClose: () => void, 
      onSelectChat: (peerHash: string, msgId: string) => void 
  }>();

  let allStarredMessages = $derived(Object.entries($messageStore).flatMap(([peerHash, msgs]) => 
    msgs.filter(m => m.isStarred).map(m => ({
        ...m,
        peerHash,
        peerNickname: $userStore.chats[peerHash]?.localNickname || $userStore.chats[peerHash]?.peerNickname || peerHash.slice(0, 8)
    }))
  ).sort((a, b) => b.timestamp - a.timestamp));
</script>

<div class="h-full w-full flex flex-col bg-entropy-bg">
    <div class="p-4 flex justify-between items-center bg-entropy-surface/50 backdrop-blur-md border-b border-entropy-border/5">
        <div class="flex items-center space-x-3">
            <LucideStar size={24} class="text-yellow-500" />
            <div>
                <h2 class="font-bold text-entropy-text-primary text-lg">Starred Messages</h2>
                <p class="text-[10px] text-entropy-text-dim">All your starred messages across all conversations</p>
            </div>
        </div>
        <button onclick={onClose} class="p-2 hover:bg-entropy-surface-light rounded-full text-entropy-text-dim hover:text-entropy-text-primary transition" title="Close">
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
                        onclick={() => onSelectChat(msg.peerHash, msg.id)}
                        class="w-full text-left p-5 bg-entropy-surface/50 hover:bg-entropy-surface rounded-2xl transition-all group relative overflow-hidden"
                        title="View in Chat"
                    >
                        <div class="flex justify-between items-start mb-3">
                             <div class="flex items-center space-x-2">
                                <LucideStar size={14} class="text-yellow-500 fill-yellow-500" />
                                <span class="text-xs font-black uppercase text-entropy-primary tracking-tight">{msg.peerNickname}</span>
                             </div>
                             <span class="text-[10px] font-bold text-entropy-text-dim">{new Date(msg.timestamp).toLocaleDateString()}</span>
                        </div>
                        
                        <div class="pointer-events-none">
                            <MessageContent 
                                {msg} 
                                isMine={msg.isMine} 
                                chatAddress={msg.peerHash}
                                compactMode={true}
                            />
                        </div>

                        <div class="absolute top-4 right-4 opacity-0 group-hover:opacity-100 transition-opacity">
                            <div class="bg-entropy-primary text-white text-[9px] font-black px-2 py-1 rounded shadow-lg">VIEW IN CHAT</div>
                        </div>
                    </button>
                {/each}
            </div>
        {/if}
    </div>
</div>
