<script lang="ts">
  import { userStore } from '../lib/stores/user';
  import { sendMessage, setReplyingTo, sendFile } from '../lib/actions/chat';
  import { sendTypingStatus, toggleBlock } from '../lib/actions/contacts';
  import { 
    LucideSend, LucideMic, LucidePaperclip, LucideX, LucideBan
  } from 'lucide-svelte';
  import RecordingBar from './RecordingBar.svelte';
  import { untrack } from 'svelte';
  import { addToast } from '../lib/stores/ui';
  import type { Chat } from '../lib/types';

  let { activeChat } = $props<{ activeChat: Chat }>();

  let messageInput = $state("");
  let isRecording = $state(false);
  let messageInputEl = $state<HTMLTextAreaElement | null>(null);

  const MAX_CHAR_LIMIT = 4000;
  let replyingTo = $derived($userStore.replyingTo);

  $effect(() => {
    if (messageInput !== undefined && messageInputEl) {
        messageInputEl.style.height = 'auto';
        messageInputEl.style.height = Math.min(messageInputEl.scrollHeight, 200) + 'px';
    }
  });

  const handleSend = () => {
    if (!messageInput.trim() || !activeChat) return;

    if (messageInput.length > MAX_CHAR_LIMIT) {
        addToast(`Message too long (${messageInput.length}/${MAX_CHAR_LIMIT} characters)`, 'error');
        return;
    }

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

  async function onFileSelect() {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const selected = await open({
        multiple: false,
        directory: false,
    });
    
    if (selected && activeChat) {
        const path = (selected as any).path || selected as string;
        const parts = path.split(/[/\\]/);
        const fileName = parts[parts.length - 1];
        
        // On mobile, paths are content:// URIs that Rust can't read.
        // We read it here in the frontend first.
        const { readFile } = await import('@tauri-apps/plugin-fs');
        const fileData = await readFile(path);
        
        const { sendFile } = await import('../lib/actions/chat');
        sendFile(activeChat.peerHash, { name: fileName, type: 'file', data: fileData }, 'file');
    }
  }

  let isLocallyTyping = $state(false);
  let lastTypingPeer: string | null = null;

  $effect(() => {
      const currentInput = messageInput;
      const currentPeer = activeChat?.peerHash;
      const canSendTyping = $userStore.privacySettings.typingStatus === 'everyone';
      
      if (lastTypingPeer && (lastTypingPeer !== currentPeer || !canSendTyping)) {
          untrack(() => {
              if (isLocallyTyping && lastTypingPeer) {
                  sendTypingStatus(lastTypingPeer, false).catch(() => {});
                  isLocallyTyping = false;
              }
          });
      }
      lastTypingPeer = currentPeer ?? null;

      if (!canSendTyping) return;

      if (currentInput.length > 0 && activeChat && !activeChat.isGroup) {
          untrack(() => {
              if (!isLocallyTyping) {
                  isLocallyTyping = true;
                  sendTypingStatus(activeChat.peerHash, true).catch(() => {});
              }
          });
      } else if (currentInput.length === 0 && activeChat && !activeChat.isGroup) {
          untrack(() => {
              if (isLocallyTyping) {
                  isLocallyTyping = false;
                  sendTypingStatus(activeChat.peerHash, false).catch(() => {});
              }
          });
      }
      
      const timeout = setTimeout(() => {
          untrack(() => {
              if (isLocallyTyping && activeChat && !activeChat.isGroup) {
                  isLocallyTyping = false;
                  sendTypingStatus(activeChat.peerHash, false).catch(() => {});
              }
          });
      }, 3000); // 3s inactivity timeout

      return () => clearTimeout(timeout);
  });

  $effect(() => {
      if (isLocallyTyping && activeChat && !activeChat.isGroup && $userStore.privacySettings.typingStatus === 'everyone') {
          const interval = setInterval(() => {
              if (isLocallyTyping && activeChat) {
                  sendTypingStatus(activeChat.peerHash, true).catch(() => {});
              }
          }, 2000); // 2s heartbeat
          return () => clearInterval(interval);
      }
  });

</script>

<div class="flex flex-col w-full z-10">
    {#if $userStore.chats[activeChat.peerHash]?.isBlocked}
        <div class="p-4 mx-6 mb-6 bg-red-500/10 backdrop-blur-md border border-red-500/20 rounded-2xl flex flex-col items-center justify-center space-y-2 animate-in fade-in slide-in-from-bottom-4 duration-500">
            <LucideBan size={20} class="text-red-500" />
            <p class="text-xs font-bold text-red-500 uppercase tracking-widest">This contact is blocked</p>
            <button 
                onclick={() => toggleBlock(activeChat.peerHash)}
                class="text-[10px] font-black text-white bg-red-500 px-4 py-1.5 rounded-full hover:bg-red-600 transition-all uppercase tracking-tighter active:scale-95"
            >
                Unblock
            </button>
        </div>
    {:else}
        {#if replyingTo}
            <div class="px-4 py-2 bg-entropy-surface/95 backdrop-blur-md flex items-center animate-in slide-in-from-bottom duration-300 border-t border-entropy-border/5">
                <div class="flex-1 bg-entropy-surface-light rounded-lg p-2 px-3 border-l-2 border-entropy-primary flex items-center justify-between shadow-sm">
                    <div class="min-w-0 pr-4">
                        <div class="text-[10px] font-black text-entropy-primary truncate uppercase tracking-widest mb-0.5">
                            {replyingTo.isMine ? 'You' : ($userStore.nicknames[replyingTo.senderHash] || replyingTo.senderAlias || replyingTo.senderHash.slice(0, 8))}
                        </div>
                        <div class="text-xs text-entropy-text-secondary line-clamp-2 opacity-80 break-words">{replyingTo.content}</div>
                    </div>
                    <button onclick={() => setReplyingTo(null)} class="p-1.5 hover:bg-entropy-surface-light rounded-full text-entropy-text-dim hover:text-red-500 transition-colors">
                        <LucideX size={16} />
                    </button>
                </div>
            </div>
        {/if}

        <div class="p-3 bg-entropy-bg flex items-end space-x-2 min-h-[64px] pb-[calc(1rem+var(--sab,0px))]">
            {#if isRecording}
                <RecordingBar 
                    onSend={(blob, duration) => {
                        sendFile(activeChat!.peerHash, new File([blob], 'Voice Note'), 'voice_note', duration);
                        isRecording = false;
                    }}
                    onCancel={() => isRecording = false}
                />
            {:else}
                <button onclick={onFileSelect} class="p-3 text-entropy-text-dim hover:bg-entropy-surface-light rounded-full transition"><LucidePaperclip size={24} /></button>
                <div class="flex-1 flex flex-col items-end">
                    <textarea 
                        id="message-input"
                        bind:this={messageInputEl}
                        bind:value={messageInput}
                        onkeydown={handleKeydown}
                        rows="1"
                        class="w-full p-3.5 pb-4 rounded-2xl border-none focus:ring-1 focus:ring-entropy-primary bg-entropy-surface-light text-entropy-text-primary resize-none max-h-[200px] overflow-y-auto custom-scrollbar placeholder:text-entropy-text-dim leading-relaxed antialiased"
                        placeholder="Type a message" 
                        style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif;"
                    ></textarea>
                    {#if messageInput.length > 3500}
                        <span class="text-[9px] font-black mr-2 mt-1 {messageInput.length > MAX_CHAR_LIMIT ? 'text-red-500' : 'text-entropy-text-dim'}">
                            {messageInput.length} / {MAX_CHAR_LIMIT}
                        </span>
                    {/if}
                </div>
                {#if !messageInput.trim()}
                    <button onclick={() => isRecording = true} class="p-3 text-entropy-text-dim hover:bg-entropy-surface-light rounded-full shadow-sm"><LucideMic size={24} /></button>
                {:else}
                    <button onclick={handleSend} class="p-3 bg-entropy-primary text-white rounded-full hover:bg-entropy-primary-dim shadow-lg active:scale-95 transition-transform"><LucideSend size={24} /></button>
                {/if}
            {/if}
        </div>
    {/if}
</div>
