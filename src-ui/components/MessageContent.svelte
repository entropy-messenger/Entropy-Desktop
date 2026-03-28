<script lang="ts">
    import type { Message } from '../lib/types';
    import { 
        LucideCheck, LucideCheckCheck, LucideStar, 
        LucideExternalLink 
    } from 'lucide-svelte';
    import { userStore } from '../lib/stores/user';
    import AttachmentRenderer from './AttachmentRenderer.svelte';

    let { 
        msg, 
        isMine, 
        chatAddress,
        compactMode = false,
        scrollToMessage
    } = $props<{
        msg: Message;
        isMine: boolean;
        chatAddress: string;
        compactMode?: boolean;
        scrollToMessage?: (id: string) => void;
    }>();

    const isReplyToMine = msg.replyTo && msg.replyTo.senderHash === $userStore.identityHash;
</script>

<div class="relative w-full">
    {#if msg.replyTo}
        <div 
            onclick={() => scrollToMessage?.(msg.replyTo!.id)} 
            onkeypress={(e) => e.key === 'Enter' && scrollToMessage?.(msg.replyTo!.id)} 
            role="button" 
            tabindex="0" 
            class="bg-black/10 dark:bg-white/5 backdrop-blur-sm p-2 px-3 rounded-lg border-l-2 border-entropy-primary mb-2 cursor-pointer hover:bg-black/15 dark:hover:bg-white/10 transition-all active:scale-[0.98] overflow-hidden"
        >
            <div class="text-[10px] font-black {isMine ? 'text-white/90' : 'text-entropy-primary'} truncate leading-tight uppercase tracking-widest mb-0.5">
                {isReplyToMine ? 'You' : (msg.replyTo.senderAlias || msg.replyTo.senderHash?.slice(0, 8) || 'Peer')}
            </div>
            <div class="{isMine ? 'text-white/80' : 'text-entropy-text-secondary'} text-[11px] truncate opacity-90 leading-tight">
                {msg.replyTo.content}
            </div>
        </div>
    {/if}

    {#if (msg.type === 'voice_note' || msg.type === 'file') && msg.attachment}
        <AttachmentRenderer {msg} chatId={chatAddress} />
        {#if !compactMode}
             <div class="absolute bottom-1 right-2 px-1.5 py-0.5 rounded-full bg-black/40 backdrop-blur-md flex items-center space-x-1 select-none pointer-events-none z-20">
                {#if msg.isStarred}<LucideStar size={9} class="text-yellow-400 fill-yellow-400/50" />{/if}
                <span class="text-[8.5px] font-bold tabular-nums text-white/90">
                    {new Date(msg.timestamp).toLocaleTimeString([], {hour: '2-digit', minute:'2-digit'})}
                </span>
                {#if isMine}
                    <div class="flex items-center scale-90 translate-y-[0.5px]">
                        {#if msg.status === 'read'}<LucideCheckCheck size={11} class="text-cyan-400" />
                        {:else if msg.status === 'delivered'}<LucideCheckCheck size={11} class="text-white/90" />
                        {:else}<LucideCheck size={11} class="text-white/90" />{/if}
                    </div>
                {/if}
             </div>
        {/if}
    {:else}
        <div class="text-[14px] leading-snug whitespace-pre-wrap break-words relative overflow-hidden pb-0.5">
            {msg.content}
            <!-- Native float wrap for the timestamp block -->
            {#if !compactMode}
                <div class="float-right flex items-center space-x-0.5 select-none pointer-events-none pt-[8px] pl-1.5 mt-[-1px]">
                    {#if msg.isStarred}
                        <LucideStar size={9} class="{isMine ? 'text-white/50 fill-white/30' : 'text-yellow-500/80 fill-yellow-500/40'}" />
                    {/if}
                    <span class="text-[9px] font-bold tabular-nums opacity-60 {isMine ? 'text-white' : 'text-entropy-text-dim'} whitespace-nowrap">
                        {new Date(msg.timestamp).toLocaleTimeString([], {hour: '2-digit', minute:'2-digit'})}
                    </span>
                    {#if isMine}
                        <div class="flex items-center scale-90 translate-y-[0.5px] opacity-70">
                            {#if msg.status === 'read'}
                                <LucideCheckCheck size={11} class="text-cyan-300" />
                            {:else if msg.status === 'delivered'}
                                <LucideCheckCheck size={11} class="text-white" />
                            {:else}
                                <LucideCheck size={11} class="text-white" />
                            {/if}
                        </div>
                    {/if}
                </div>
            {/if}
        </div>
    {/if}

    {#if msg.linkPreview}
        <a href={msg.linkPreview.url} target="_blank" rel="noopener noreferrer" class="block mt-2 bg-black/10 dark:bg-white/10 rounded-xl overflow-hidden hover:bg-black/20 dark:hover:bg-white/20 transition no-underline">
            <div class="p-3">
                <div class="text-[10px] font-bold {isMine ? 'text-white/60' : 'text-entropy-accent'} mb-1 flex items-center space-x-1 uppercase tracking-wide">
                    <span>{msg.linkPreview.siteName}</span>
                    <LucideExternalLink size={10} />
                </div>
                <div class="text-[13px] font-bold {isMine ? 'text-white' : 'text-entropy-text-primary'} truncate mb-1">{msg.linkPreview.title}</div>
                <div class="text-[11px] {isMine ? 'text-white/70' : 'text-entropy-text-secondary'} line-clamp-2 leading-snug">{msg.linkPreview.url}</div>
            </div>
        </a>
    {/if}

    <!-- Compact Mode Status Cluster -->
    {#if compactMode}
        <div class="mt-1 flex items-center space-x-2 select-none pointer-events-none">
            {#if msg.isStarred}
                <LucideStar size={9} class="{isMine ? 'text-white/40 fill-white/20' : 'text-yellow-500/60 fill-yellow-500/20'}" />
            {/if}
            <span class="text-[8.5px] font-bold tabular-nums {isMine ? 'text-white/60' : 'text-entropy-text-secondary/70'}">
                {new Date(msg.timestamp).toLocaleTimeString([], {hour: '2-digit', minute:'2-digit'})}
            </span>
        </div>
    {/if}
</div>
