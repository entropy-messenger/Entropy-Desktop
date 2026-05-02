<script lang="ts">
    import type { Message, Chat } from '../lib/types';
    import { 
        LucideCheck, LucideCheckCheck, LucideStar, LucideReply, 
        LucideExternalLink, LucideCheck as LucideCheckIcon 
    } from 'lucide-svelte';
    import { userStore } from '../lib/stores/user';
    import { resolveIdentity } from '../lib/actions/contacts';
    import AttachmentRenderer from './AttachmentRenderer.svelte';
    import MessageContent from './MessageContent.svelte';
    import { onMount } from 'svelte';

    let { 
        msg, 
        activeChat, 
        selectionMode, 
        selectedIds,
        toggleSelect,
        scrollToMessage,
        setReplyingTo,
        toggleStar,
        isMobile
    } = $props<{
        msg: Message,
        activeChat: Chat,
        selectionMode: boolean,
        selectedIds: string[],
        toggleSelect: (id: string) => void,
        scrollToMessage: (id: string) => void,
        setReplyingTo: (msg: Message) => void,
        toggleStar: (peerHash: string, id: string) => void,
        isMobile?: boolean
    }>();

    let showActions = $state(false);
    const isSelected = $derived(selectedIds.includes(msg.id));
    
    /**
     * Deterministically derives a vibrant, readable color from a sender's hash.
     * This ensures each user has a consistent visual identity in group chats.
     */
    const getSenderColor = (hash: string) => {
        if (!hash) return 'var(--entropy-primary)';
        
        const colors = [
            '#FF6B6B', '#4ECDC4', '#45B7D1', '#96CEB4', 
            '#FFEEAD', '#D4A5A5', '#9B59B6', '#3498DB', 
            '#E67E22', '#2ECC71', '#F1C40F', '#E74C3C'
        ];
        
        let hashValue = 0;
        for (let i = 0; i < hash.length; i++) {
            hashValue = hash.charCodeAt(i) + ((hashValue << 5) - hashValue);
        }
        
        return colors[Math.abs(hashValue) % colors.length];
    };

    onMount(() => {
        // Identity resolution: Fetch sender nickname if missing from local cache
        if (!msg.isMine && msg.senderHash && !$userStore.nicknames[msg.senderHash]) {
            resolveIdentity(msg.senderHash);
        }
        // Resolve metadata for referenced messages in replies
        if (msg.replyTo?.senderHash && !$userStore.nicknames[msg.replyTo.senderHash]) {
            resolveIdentity(msg.replyTo.senderHash);
        }
    });
</script>

<div id="msg-{msg.id}" class="flex {msg.isMine ? 'justify-end' : 'justify-start'} group items-center relative z-10 transition-opacity duration-300">
    {#if selectionMode}
        <div class="mr-4 order-first">
            <button 
                onclick={() => toggleSelect(msg.id)}
                class="w-5 h-5 rounded-full border-2 transition-all flex items-center justify-center {isSelected ? 'bg-entropy-primary border-entropy-primary' : 'border-entropy-border/10 bg-entropy-surface'}"
            >
                {#if isSelected}<LucideCheckIcon size={12} class="text-white" />{/if}
            </button>
        </div>
    {/if}

    <div class="flex flex-col {msg.isMine ? 'items-end' : 'items-start'} max-w-[65%] relative z-10">
        <div 
            style="word-break: break-all; min-width: {msg.isMine ? '68px' : '52px'};"
            ondblclick={() => setReplyingTo(msg)}
            onclick={() => { 
                if (selectionMode) { toggleSelect(msg.id); } 
                else if (isMobile) { showActions = !showActions; }
            }}
            role="button"
            tabindex="0"
            onkeydown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                    if (selectionMode) { toggleSelect(msg.id); } 
                    else if (isMobile) { showActions = !showActions; }
                }
            }}
            class="relative rounded-2xl shadow-sm transition-all duration-200 overflow-hidden cursor-default active:scale-[0.99] max-w-full
                {msg.type === 'voice_note' ? 'p-1.5 px-2' : (msg.type === 'file' ? 'p-1' : 'pt-1.5 px-4 pb-1.5')}
                {msg.isMine ? (msg.isStarred ? 'bg-entropy-primary-dim ring-1 ring-yellow-400/60 shadow-[0_0_10px_rgba(250,204,21,0.15)]' : 'bg-entropy-primary-dim') : (msg.isStarred ? 'bg-entropy-surface-light ring-1 ring-yellow-500/40 shadow-[0_0_10px_rgba(250,204,21,0.1)]' : 'bg-entropy-surface-light')}
                {msg.isMine ? 'text-white rounded-tr-none' : 'text-entropy-text-primary rounded-tl-none'}
                {isSelected ? 'ring-4 ring-entropy-accent ring-opacity-50 opacity-100 scale-100' : ''}
            "
        >
            {#if activeChat?.isGroup && !msg.isMine && msg.senderHash}
                <div class="mb-0.5 px-1 select-text">
                    <span 
                        class="text-[12.5px] font-bold tracking-tight"
                        style="color: {getSenderColor(msg.senderHash)}"
                    >
                        {$userStore.nicknames[msg.senderHash] || msg.senderHash.slice(0, 8)}
                    </span>
                </div>
            {/if}
 
            <MessageContent 
                {msg} 
                isMine={msg.isMine} 
                chatAddress={activeChat.peerHash} 
                {scrollToMessage}
                {isMobile}
            />
        </div>

        {#if !selectionMode}
            <div class="absolute {msg.isMine ? '-left-10' : '-right-10'} top-0 bottom-0 flex flex-col items-center justify-center space-y-1 transition-all duration-200 z-10
                {isMobile ? (showActions ? 'opacity-100 scale-100' : 'opacity-0 scale-90 pointer-events-none') : 'opacity-0 group-hover:opacity-100'}"
            >
                <button onclick={() => { setReplyingTo(msg); if(isMobile) showActions = false; }} class="p-1.5 hover:bg-entropy-surface-light bg-entropy-surface/90 backdrop-blur-sm rounded-full text-entropy-text-dim hover:text-entropy-primary shadow-md transition active:scale-90" title="Reply"><LucideReply size={isMobile ? 16 : 14} /></button>
                <button onclick={() => { toggleStar(activeChat.peerHash, msg.id); if(isMobile) showActions = false; }} class="p-1.5 hover:bg-entropy-surface-light bg-entropy-surface/90 backdrop-blur-sm rounded-full text-entropy-text-dim hover:text-yellow-500 shadow-md transition active:scale-90" title="Star"><LucideStar size={isMobile ? 16 : 14} class={msg.isStarred ? 'fill-yellow-500 text-yellow-500' : ''} /></button>
            </div>
        {/if}
    </div>
</div>
