<script lang="ts">
  import { onMount, tick } from 'svelte';
  import AttachmentRenderer from '../AttachmentRenderer.svelte';
  import { LucideCheck, LucideCheckCheck, LucideStar, LucideReply } from 'lucide-svelte';
  import type { Chat, Message } from '../../lib/types';

  interface Props {
    messages: Message[];
    activeChat: Chat;
    selectionMode: boolean;
    selectedIds: string[];
    onToggleSelect: (id: string) => void;
    onScrollToMessage: (id: string) => void;
    onSetReplyingTo: (msg: Message) => void;
  }

  let { 
    messages, 
    activeChat, 
    selectionMode, 
    selectedIds, 
    onToggleSelect, 
    onScrollToMessage, 
    onSetReplyingTo 
  }: Props = $props();

  let scrollContainer = $state<HTMLElement | null>(null);

  const scrollToBottom = async () => {
      await tick();
      if (scrollContainer && !selectionMode) scrollContainer.scrollTop = scrollContainer.scrollHeight;
  };

  let lastCount = $state(0);
  $effect(() => {
    if (messages.length !== lastCount) {
      lastCount = messages.length;
      scrollToBottom();
    }
  });

  const formatTime = (ts: number) => {
      return new Date(ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  };
</script>

<div 
    bind:this={scrollContainer}
    class="flex-1 overflow-y-auto p-4 md:p-6 space-y-4 custom-scrollbar relative"
>
    <!-- Day grouping could be added here -->
    {#each messages as msg (msg.id)}
        <div 
            id="msg-{msg.id}"
            class="flex {msg.isMine ? 'justify-end' : 'justify-start'} group items-end space-x-2"
        >
            {#if selectionMode}
                <div class="mb-2 mr-2">
                    <input 
                        type="checkbox" 
                        checked={selectedIds.includes(msg.id)} 
                        onchange={() => onToggleSelect(msg.id)}
                        class="w-5 h-5 rounded-lg border-2 border-indigo-200 text-indigo-600 focus:ring-indigo-500 transition cursor-pointer"
                    />
                </div>
            {/if}

            <div class="flex flex-col max-w-[85%] md:max-w-[70%] space-y-1">
                {#if activeChat.isGroup && !msg.isMine}
                    <span class="text-[10px] font-black uppercase tracking-widest text-indigo-500 mb-1 ml-3 px-2 py-0.5 bg-indigo-50 rounded-full inline-block w-fit">{msg.senderAlias || msg.senderHash.slice(0,8)}</span>
                {/if}

                <div 
                    class="relative p-3 rounded-2xl shadow-sm transition-all duration-200 
                    {msg.isMine ? 'bg-[#d9fdd3] rounded-br-none' : 'bg-white rounded-bl-none'} 
                    {selectedIds.includes(msg.id) ? 'ring-2 ring-indigo-500 scale-[0.98]' : ''}"
                    oncontextmenu={(e) => { e.preventDefault(); onToggleSelect(msg.id); }}
                >
                    {#if msg.isStarred}
                        <div class="absolute -top-2 -right-2 bg-yellow-400 text-white p-1 rounded-lg shadow-md scale-75">
                            <LucideStar size={12} fill="currentColor" />
                        </div>
                    {/if}

                    {#if msg.replyTo}
                        <button 
                            onclick={() => onScrollToMessage(msg.replyTo!.id)}
                            class="mb-2 p-2 bg-black/5 rounded-xl border-l-4 border-indigo-500 text-left block w-full hover:bg-black/10 transition active:scale-[0.98]"
                        >
                            <div class="text-[10px] font-black uppercase text-indigo-600 mb-0.5">{msg.replyTo.senderAlias || 'Peer'}</div>
                            <div class="text-xs text-gray-600 line-clamp-2 italic opacity-70">{msg.replyTo.content}</div>
                        </button>
                    {/if}

                    {#if msg.type === 'voice_note'}
                        <div class="flex flex-col space-y-2 min-w-[200px]">
                            <AttachmentRenderer {msg} />
                        </div>
                    {:else if msg.type === 'file'}
                        <AttachmentRenderer {msg} />
                        {#if msg.content}
                            <p class="mt-2 text-sm text-gray-800 whitespace-pre-wrap break-words leading-relaxed">{msg.content}</p>
                        {/if}
                    {:else}
                        <p class="text-sm text-gray-800 whitespace-pre-wrap break-words leading-relaxed font-medium">{msg.content}</p>
                    {/if}

                    {#if msg.linkPreview}
                        <a 
                            href={msg.linkPreview.url} 
                            target="_blank" 
                            rel="noopener noreferrer"
                            class="mt-3 block bg-black/5 rounded-xl overflow-hidden border border-black/5 hover:bg-black/10 transition group/link"
                        >
                            {#if msg.linkPreview.image}
                                <img src={msg.linkPreview.image} alt="" class="w-full h-32 object-cover opacity-90 group-hover/link:opacity-100 transition" />
                            {/if}
                            <div class="p-3 space-y-1">
                                <div class="text-[10px] font-black uppercase text-indigo-600 tracking-wider">{msg.linkPreview.siteName || 'Link'}</div>
                                <div class="text-xs font-bold text-gray-800 line-clamp-1">{msg.linkPreview.title}</div>
                                {#if msg.linkPreview.description}
                                    <div class="text-[10px] text-gray-500 line-clamp-2 leading-snug">{msg.linkPreview.description}</div>
                                {/if}
                            </div>
                        </a>
                    {/if}

                    <div class="flex items-center justify-end space-x-1 mt-1 opacity-40">
                        <span class="text-[9px] font-black uppercase tracking-tighter">{formatTime(msg.timestamp)}</span>
                        {#if msg.isMine}
                            {#if msg.status === 'sending'}
                                <div class="w-2 h-2 rounded-full border border-gray-400 border-t-transparent animate-spin"></div>
                            {:else if msg.status === 'sent'}
                                <LucideCheck size={12} />
                            {:else if msg.status === 'delivered' || msg.status === 'read'}
                                <LucideCheckCheck size={12} class={msg.status === 'read' ? 'text-blue-500' : ''} />
                            {/if}
                        {/if}
                    </div>
                </div>
            </div>

            <div class="flex flex-col space-y-1 opacity-0 group-hover:opacity-100 transition-all duration-200 mb-2">
                <button 
                    onclick={() => onSetReplyingTo(msg)}
                    class="p-2 bg-white/80 backdrop-blur rounded-lg shadow-sm hover:bg-white text-gray-500 hover:text-indigo-600 transition active:scale-90"
                >
                    <LucideReply size={14} />
                </button>
            </div>
        </div>
    {/each}
</div>

<style>
    .custom-scrollbar::-webkit-scrollbar { width: 4px; }
    .custom-scrollbar::-webkit-scrollbar-track { background: transparent; }
    .custom-scrollbar::-webkit-scrollbar-thumb { background: rgba(0,0,0,0.05); border-radius: 10px; }
    .custom-scrollbar::-webkit-scrollbar-thumb:hover { background: rgba(0,0,0,0.1); }
</style>
