<script lang="ts">
    import type { Message } from '../lib/types';
    import { 
        LucideCheck, LucideCheckCheck, LucideStar, LucideClock, LucideX 
    } from 'lucide-svelte';
    import { userStore } from '../lib/stores/user';
    import AttachmentRenderer from './AttachmentRenderer.svelte';
    import { open } from '@tauri-apps/plugin-shell';

    let { 
        msg, 
        isMine, 
        chatAddress,
        isMobile = false,
        compactMode = false,
        scrollToMessage
    } = $props<{
        msg: Message;
        isMine: boolean;
        chatAddress: string;
        compactMode?: boolean;
        isMobile?: boolean;
        scrollToMessage?: (id: string) => void;
    }>();

    const isReplyToMine = $derived(msg.replyTo && msg.replyTo.senderHash === $userStore.identityHash);

    const escapeHTML = (text: string) => {
        if (!text) return '';
        return text
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#39;');
    };

    const linkify = (text: string, mine: boolean) => {
        if (!text) return '';
        // Robust URL regex matching http(s), www. OR naked domains
        const urlRegex = /(https?:\/\/[^\s]+|www\.[^\s]+|[a-zA-Z0-9.-]+\.(?:com|net|org|io|dev|ai|app|me|network|xyz)(?:\/[^\s]*)?)/gi;
        const linkClass = mine ? 'text-cyan-300 font-bold underline decoration-cyan-300/40' : 'text-entropy-primary font-bold underline decoration-1';
        
        return text.split(urlRegex).map(part => {
            if (part && part.match(urlRegex)) {
                let rawHref = part;
                if (!rawHref.startsWith('http')) {
                    rawHref = `https://${rawHref}`;
                }
                
                // Escape everything for the attributes AND the content
                const safePart = escapeHTML(part);
                const safeHref = escapeHTML(rawHref);

                return `<a href="${safeHref}" target="_blank" class="message-link ${linkClass} underline-offset-4 hover:opacity-80 transition-all cursor-pointer">${safePart}</a>`;
            }
            // Escape everything else to avoid XSS
            return escapeHTML(part);
        }).join('');
    };

    const handleMessageClick = (e: MouseEvent) => {
        const target = e.target as HTMLElement;
        if (target.classList.contains('message-link')) {
            e.preventDefault();
            const href = target.getAttribute('href');
            if (href) open(href).catch(() => {});
        }
    };

    /**
     * Scans message content for 64-character hashes and replaces them with
     * nicknames if available in the $userStore.
     */
    const resolveHashesInText = (text: string) => {
        if (!text) return text;
        const hashRegex = /\b([a-fA-F0-9]{64})\b/g;
        return text.replace(hashRegex, (match) => {
            const nick = $userStore.nicknames[match.toLowerCase()];
            return nick ? nick : match.slice(0, 8);
        });
    };

    // resolve unknown hashes in protocol messages
    import { resolveIdentity } from '../lib/actions/contacts';
    $effect(() => {
        if (msg.type === 'group_management' || msg.type === 'system') {
            const hashRegex = /\b([a-fA-F0-9]{64})\b/g;
            const matches = msg.content.match(hashRegex);
            if (matches) {
                matches.forEach((hash: string) => {
                    const h = hash.toLowerCase();
                    if (!$userStore.nicknames[h]) {
                        resolveIdentity(h);
                    }
                });
            }
        }
    });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="relative w-full" onclick={handleMessageClick}>
    {#if msg.replyTo}
        <div 
            onclick={() => scrollToMessage?.(msg.replyTo!.id)} 
            onkeypress={(e) => e.key === 'Enter' && scrollToMessage?.(msg.replyTo!.id)} 
            role="button" 
            tabindex="0" 
            class="bg-black/10 dark:bg-white/5 backdrop-blur-sm p-2 px-3 rounded-lg border-l-2 border-entropy-primary mb-2 cursor-pointer hover:bg-black/15 dark:hover:bg-white/10 transition-all active:scale-[0.98] overflow-hidden"
        >
            <div class="text-[10px] font-black {isMine ? 'text-white/90' : 'text-entropy-primary'} line-clamp-1 leading-tight tracking-wide mb-0.5">
                {isReplyToMine ? 'You' : ($userStore.nicknames[msg.replyTo.senderHash] || msg.replyTo.senderAlias || msg.replyTo.senderHash?.slice(0, 8) || 'Peer')}
            </div>
            <div class="{isMine ? 'text-white/80' : 'text-entropy-text-secondary'} text-[11px] line-clamp-2 opacity-90 leading-tight break-words">
                {resolveHashesInText(msg.replyTo.content)}
            </div>
        </div>
    {/if}

    {#if (msg.type === 'voice_note' || msg.type === 'file') && msg.attachment}
        <AttachmentRenderer {msg} chatId={chatAddress} {isMobile} />
        
        {#if !compactMode}
             <div class="absolute bottom-1 right-2 px-1.5 py-0.5 rounded-full bg-black/40 backdrop-blur-md flex items-center space-x-1 select-none pointer-events-none z-20">
                {#if msg.isStarred}<LucideStar size={9} class="text-yellow-400 fill-yellow-400/50" />{/if}
                <span class="text-[10.5px] font-semibold tabular-nums text-white/90">
                    {new Date(msg.timestamp).toLocaleTimeString([], {hour: '2-digit', minute:'2-digit'})}
                </span>
                {#if isMine}
                    {@const canSeeReceipts = $userStore.privacySettings.readReceipts}
                    <div class="flex items-center scale-90 translate-y-[0.5px]">
                        {#if msg.status === 'pending' || msg.status === 'sending'}<LucideClock size={10} class="text-white/70 animate-pulse" />
                        {:else if msg.status === 'failed'}<LucideX size={11} class="text-red-400" />
                        {:else if msg.status === 'read' && canSeeReceipts}<LucideCheckCheck size={11} class="text-cyan-400" />
                        {:else if msg.status === 'read' || msg.status === 'delivered'}<LucideCheckCheck size={11} class="text-white/90" />
                        {:else}<LucideCheck size={11} class="text-white/90" />{/if}
                    </div>
                {/if}
             </div>
        {/if}
    {:else}
        <div class="text-[15.5px] leading-relaxed whitespace-pre-wrap break-words relative overflow-hidden pb-0.5 select-text font-medium antialiased">
            {@html linkify(resolveHashesInText(msg.content), isMine)}
            <!-- Native float wrap for the timestamp block -->
            {#if !compactMode}
                <div class="float-right flex items-center space-x-0.5 select-none pointer-events-none pt-[8px] pl-1.5 mt-[-1px]">
                    {#if msg.isStarred}
                        <LucideStar size={9} class={isMine ? 'text-white/50 fill-white/30' : 'text-yellow-500/80 fill-yellow-500/40'} />
                    {/if}
                    <span class="text-[11px] font-semibold tabular-nums opacity-70 {isMine ? 'text-white' : 'text-entropy-text-dim'} whitespace-nowrap">
                        {new Date(msg.timestamp).toLocaleTimeString([], {hour: '2-digit', minute:'2-digit'})}
                    </span>
                    {#if isMine}
                        {@const canSeeReceipts = $userStore.privacySettings.readReceipts}
                        <div class="flex items-center scale-90 translate-y-[0.5px] opacity-70">
                            {#if msg.status === 'pending' || msg.status === 'sending'}
                                <LucideClock size={10} class="{isMine ? 'text-white/70' : 'text-entropy-text-dim/70'} animate-pulse" />
                            {:else if msg.status === 'failed'}
                                <LucideX size={11} class="text-red-400" />
                            {:else if msg.status === 'read' && canSeeReceipts}
                                <LucideCheckCheck size={11} class="text-cyan-300" />
                            {:else if msg.status === 'read' || msg.status === 'delivered'}
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


    <!-- Compact Mode Status Cluster -->
    {#if compactMode}
        <div class="mt-1 flex items-center space-x-2 select-none pointer-events-none">
            {#if msg.isStarred}
                <LucideStar size={9} class={isMine ? 'text-white/40 fill-white/20' : 'text-yellow-500/60 fill-yellow-500/20'} />
            {/if}
            <span class="text-[10px] font-semibold tabular-nums {isMine ? 'text-white/60' : 'text-entropy-text-secondary/70'}">
                {new Date(msg.timestamp).toLocaleTimeString([], {hour: '2-digit', minute:'2-digit'})}
            </span>
        </div>
    {/if}
</div>
