<script lang="ts">
    import type { Message, Chat } from '../lib/types';
    import { 
        LucideCheck, LucideCheckCheck, LucideStar, LucideReply, 
        LucideExternalLink, LucideCheck as LucideCheckIcon 
    } from 'lucide-svelte';
    import AttachmentRenderer from './AttachmentRenderer.svelte';
    import MessageContent from './MessageContent.svelte';

    let { 
        msg, 
        activeChat, 
        selectionMode, 
        selectedIds,
        toggleSelect,
        scrollToMessage,
        setReplyingTo,
        toggleStar
    } = $props<{
        msg: Message,
        activeChat: Chat,
        selectionMode: boolean,
        selectedIds: string[],
        toggleSelect: (id: string) => void,
        scrollToMessage: (id: string) => void,
        setReplyingTo: (msg: Message) => void,
        toggleStar: (peerHash: string, id: string) => void
    }>();

    const isSelected = $derived(selectedIds.includes(msg.id));
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
            onclick={() => selectionMode && toggleSelect(msg.id)}
            role="button"
            tabindex="0"
            class="relative rounded-2xl shadow-sm transition-all duration-200 overflow-hidden cursor-pointer active:scale-[0.99]
                {msg.type === 'voice_note' ? 'p-1.5 px-2' : 'p-2.5 px-4 pb-1.5'}
                {msg.isMine ? (msg.isStarred ? 'bg-entropy-primary ring-1 ring-yellow-400/60 shadow-[0_0_10px_rgba(250,204,21,0.15)]' : 'bg-entropy-primary') : (msg.isStarred ? 'bg-entropy-surface-light ring-1 ring-yellow-500/40 shadow-[0_0_10px_rgba(250,204,21,0.1)]' : 'bg-entropy-surface-light')}
                {msg.isMine ? 'text-white rounded-tr-none' : 'text-entropy-text-primary rounded-tl-none'}
                {isSelected ? 'ring-4 ring-entropy-accent ring-opacity-50 opacity-100 scale-100' : ''}
            "
        >
            {#if activeChat.isGroup && !msg.isMine}
                <div class="text-[10px] font-bold text-entropy-primary mb-1 opacity-80">{msg.senderAlias || msg.senderHash.slice(0, 12)}</div>
            {/if}

            <MessageContent 
                {msg} 
                isMine={msg.isMine} 
                chatAddress={activeChat.peerHash} 
                {scrollToMessage}
            />
        </div>

        {#if !selectionMode}
            <div class="absolute {msg.isMine ? '-left-8' : '-right-8'} top-0 bottom-0 opacity-0 group-hover:opacity-100 flex flex-col items-center justify-center space-y-1 transition-all duration-200 z-10">
                <button onclick={() => setReplyingTo(msg)} class="p-1.5 hover:bg-entropy-surface-light bg-entropy-surface/80 backdrop-blur-sm rounded-full text-entropy-text-dim hover:text-entropy-primary shadow-sm transition active:scale-90" title="Reply"><LucideReply size={14} /></button>
                <button onclick={() => toggleStar(activeChat.peerHash, msg.id)} class="p-1.5 hover:bg-entropy-surface-light bg-entropy-surface/80 backdrop-blur-sm rounded-full text-entropy-text-dim hover:text-yellow-500 shadow-sm transition active:scale-90" title="Star"><LucideStar size={14} class={msg.isStarred ? 'fill-yellow-500 text-yellow-500' : ''} /></button>
            </div>
        {/if}
    </div>
</div>
