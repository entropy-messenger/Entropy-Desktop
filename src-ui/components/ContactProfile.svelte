<script lang="ts">
  import { userStore, messageStore } from '../lib/stores/user';
  import { leaveGroup, addToGroup } from '../lib/actions/groups';
  import { deleteChat } from '../lib/actions/chat';
  import { toggleBlock, setTrustLevel, startChat } from '../lib/actions/contacts';
  import { signalManager } from '../lib/signal_manager';
  import { 
    LucideX, LucideShieldCheck, LucideShieldAlert, LucideInfo,
    LucideImage, LucideLink, LucideTrash2, LucideCheck as LucideCheckIcon, 
    LucideCopy, LucideLoader, LucideExternalLink
  } from 'lucide-svelte';
  import MediaThumbnail from './MediaThumbnail.svelte';
  import Avatar from './Avatar.svelte';
  import { addToast, showConfirm, showPrompt } from '../lib/stores/ui';
  import type { Chat } from '../lib/types';
  
  let { activeChat, onClose, onScrollToMessage } = $props<{ activeChat: Chat, onClose: () => void, onScrollToMessage: (id: string) => void }>();

  let safetyNumber = $state<{ digits: string; trustLevel: number } | null>(null);
  let loadingSafetyNumber = $state(false);

  async function loadSafetyNumber() {
    if (!activeChat || activeChat.isGroup) {
        safetyNumber = null;
        return;
    }
    loadingSafetyNumber = true;
    try {
        const result = await signalManager.getFingerprint(activeChat.peerHash);
        safetyNumber = result;
        // Keep store in sync if it's different
        if (activeChat.trustLevel !== result.trustLevel) {
            userStore.update(s => {
                if (s.chats[activeChat.peerHash]) s.chats[activeChat.peerHash].trustLevel = result.trustLevel;
                return s;
            });
        }
    } catch (e) {
        console.error("Error loading safety number:", e);
    } finally {
        loadingSafetyNumber = false;
    }
  }

  $effect(() => {
    if (activeChat && !activeChat.isGroup) {
        loadSafetyNumber();
    }
  });

  let mediaMessages = $derived(activeChat ? ($messageStore[activeChat.peerHash] || []).filter((m: any) => m.attachment && m.type === 'file') : []);
</script>

<div class="w-80 bg-entropy-bg flex flex-col animate-in slide-in-from-right duration-300 z-[40]">
    <div class="p-4 flex justify-between items-center bg-entropy-surface">
        <h2 class="font-bold text-entropy-text-primary flex items-center space-x-2"><LucideInfo size={18} /><span>Info & Media</span></h2>
        <button onclick={onClose} class="text-entropy-text-dim hover:text-red-500 transition"><LucideX size={20} /></button>
    </div>
    
    <div class="p-6 flex-1 overflow-y-auto custom-scrollbar space-y-8">
        <div class="flex flex-col items-center space-y-4">
            <Avatar hash={activeChat.peerHash} alias={activeChat.localNickname || activeChat.peerNickname} size="w-24 h-24" textSize="text-3xl" rounded="rounded-3xl" />
            <div class="text-center">
                <div class="flex items-center justify-center space-x-2">
                    <h3 class="text-xl font-bold text-entropy-text-primary">{activeChat.localNickname || activeChat.peerNickname || 'Peer'}</h3>
                    {#if !activeChat.isGroup}
                        {#if activeChat.trustLevel >= 2}
                            <LucideShieldCheck size={18} class="text-green-500" />
                        {:else if activeChat.trustLevel === 0}
                            <LucideShieldAlert size={18} class="text-red-500" />
                        {:else}
                            <LucideShieldCheck size={18} class="text-entropy-text-dim opacity-50" />
                        {/if}
                    {/if}
                </div>
                {#if activeChat.localNickname && activeChat.peerNickname}
                    <p class="text-[10px] font-bold text-entropy-primary uppercase mb-1 tracking-wide">Alias: {activeChat.peerNickname}</p>
                {/if}
                <p class="text-[11px] font-mono text-entropy-text-secondary break-all opacity-80">{activeChat.peerHash}</p>
            </div>
        </div>

        {#if !activeChat.isGroup}
            {@const currentTrust = $userStore.chats[activeChat.peerHash]?.trustLevel ?? 1}
            <div class="space-y-4 pt-2">
                 <div class="flex items-center justify-between">
                     <h4 class="text-[10px] font-black text-entropy-text-dim uppercase tracking-[0.1em]">Identity Verification</h4>
                     {#if currentTrust >= 2}
                        <div class="flex items-center space-x-1 text-green-500 animate-in fade-in zoom-in duration-300">
                            <LucideShieldCheck size={12} />
                            <span class="text-[9px] font-black uppercase tracking-widest">Verified Identity</span>
                        </div>
                    {:else if currentTrust === 0}
                        <div class="flex items-center space-x-1 text-red-500 animate-pulse">
                            <LucideShieldAlert size={12} />
                            <span class="text-[9px] font-black uppercase tracking-widest text-red-500">Identity Mismatch</span>
                        </div>
                    {:else}
                        <div class="flex items-center space-x-1 text-orange-500/80">
                            <LucideShieldCheck size={12} />
                            <span class="text-[9px] font-black uppercase tracking-widest">Trusted</span>
                        </div>
                    {/if}
                 </div>

                 <div class="bg-entropy-surface-light p-4 rounded-2xl border border-white/5 space-y-3">
                    {#if loadingSafetyNumber}
                        <div class="flex flex-col items-center py-4 space-y-2">
                            <LucideLoader size={20} class="animate-spin text-entropy-primary" />
                            <span class="text-[10px] text-entropy-text-dim uppercase font-bold tracking-widest">Generating Fingerprint...</span>
                        </div>
                    {:else if safetyNumber}
                        <div class="grid grid-cols-2 gap-x-4 gap-y-2 font-mono text-[11px] text-entropy-text-primary text-center opacity-90 leading-relaxed bg-black/5 dark:bg-white/5 p-3 rounded-xl border border-white/5">
                            {#each safetyNumber.digits.split('\n') as line}
                                {#each line.split(' ') as part}
                                    <div class="tracking-widest">{part}</div>
                                {/each}
                            {/each}
                        </div>

                         <div class="pt-2">
                            <button 
                                onclick={async () => {
                                    const nextLevel = currentTrust >= 2 ? 1 : 2;
                                    await setTrustLevel(activeChat!.peerHash, nextLevel);
                                    addToast(nextLevel === 2 ? "Identity Verified" : "Verification Reset", nextLevel === 2 ? 'success' : 'info');
                                }}
                                class="w-full py-2.5 rounded-xl text-[10px] font-black uppercase tracking-widest transition-all active:scale-[0.98] shadow-lg
                                {currentTrust >= 2
                                    ? 'bg-red-500/10 text-red-500 hover:bg-red-500/20' 
                                    : 'bg-entropy-accent text-white hover:bg-entropy-accent/90 shadow-entropy-accent/20'}"
                            >
                                {currentTrust >= 2 ? 'Reset Verification' : 'Verify Identity'}
                            </button>
                            <p class="text-[9px] text-entropy-text-dim text-center mt-3 leading-relaxed px-2">
                                {#if currentTrust === 0}
                                    <span class="text-red-500 font-bold block mb-1">WARNING: The identity for this contact has changed.</span>
                                {/if}
                                Verify the safety number above with this contact via another secure channel.
                            </p>
                        </div>
                    {:else}
                         <div class="text-[10px] text-center text-orange-500/80 font-bold py-2 uppercase tracking-tighter">Establishing Secure Session...</div>
                    {/if}
                 </div>
            </div>
        {/if}

        {#if activeChat.isGroup}
            <div class="space-y-2">
                 <div class="flex justify-between items-center">
                    <h4 class="text-xs font-bold text-entropy-text-dim uppercase tracking-widest">Members ({activeChat.members?.length || 0})</h4>
                    <button onclick={async () => {
                        let input = await showPrompt("Enter Peer Hash or Nickname:", "", "Add Member");
                        if (input) {
                            if (input.length === 64) {
                                addToGroup(activeChat!.peerHash, [input]);
                                addToast("Invitation sent!", 'success');
                            } else {
                                addToast("Please provide a full 64-char ID for now.", 'info');
                            }
                        }
                    }} class="text-[10px] font-black text-entropy-primary hover:underline uppercase">Add Member</button>
                 </div>
                 <div class="space-y-1 max-h-40 overflow-y-auto custom-scrollbar pr-1">
                    {#each activeChat.members || [] as member}
                        <!-- svelte-ignore a11y_click_events_have_key_events -->
                        <!-- svelte-ignore a11y_interactive_supports_focus -->
                        <div 
                            class="flex items-center space-x-2 bg-entropy-surface-light p-2 rounded-lg cursor-pointer hover:bg-entropy-primary/10 transition group/member"
                            onclick={() => {
                                if (member !== $userStore.identityHash) {
                                    startChat(member);
                                    onClose();
                                }
                            }}
                            role="button"
                        >
                            <Avatar hash={member} alias={member.slice(0, 8)} size="w-5 h-5" textSize="text-[8px]" rounded="rounded-md" />
                            <span class="text-[10px] font-mono text-entropy-text-secondary truncate flex-1 group-hover/member:text-entropy-primary transition-colors">{member.slice(0, 16)}...</span>
                            {#if member === $userStore.identityHash}
                                <span class="text-[8px] font-black bg-entropy-primary/10 text-entropy-primary px-1 rounded">YOU</span>
                            {/if}
                        </div>
                    {/each}
                 </div>
            </div>
        {/if}

        <div class="space-y-4">
            <div class="flex justify-between items-center">
                <h4 class="text-xs font-bold text-entropy-text-dim uppercase tracking-wider">Shared Media</h4>
                <LucideImage size={14} class="text-entropy-text-dim" />
            </div>
            {#if mediaMessages.length === 0}
                <div class="bg-entropy-surface-light rounded-2xl h-32 flex items-center justify-center text-xs text-entropy-text-dim">No media shared yet</div>
            {:else}
                <div class="grid grid-cols-3 gap-2">
                    {#each mediaMessages.slice(0, 9) as m}
                        <MediaThumbnail msg={m} onclick={() => onScrollToMessage(m.id)} />
                    {/each}
                </div>
            {/if}
        </div>


        {#if activeChat.isGroup}
            <div class="pt-4 border-t border-entropy-border/10">
                <button 
                    onclick={async () => { if (await showConfirm("Are you sure you want to leave this group? All history will be deleted.", "Leave Group")) { leaveGroup(activeChat!.peerHash); onClose(); } }}
                    class="w-full flex items-center justify-center space-x-2 p-3 rounded-xl bg-red-500/10 text-red-500 hover:bg-red-500/20 font-bold text-xs transition active:scale-[0.98]"
                >
                    <LucideTrash2 size={16} />
                    <span>Leave Group</span>
                </button>
            </div>
        {:else}
            <div class="pt-4 border-t border-entropy-border/10">
                <button 
                    onclick={async () => { if (await showConfirm("Are you sure you want to delete this conversation? This will permanently erase all messages and cannot be undone.", "Delete Chat")) { deleteChat(activeChat!.peerHash); onClose(); } }}
                    class="w-full flex items-center justify-center space-x-2 p-3 rounded-xl bg-red-500/10 text-red-500 hover:bg-red-500/20 font-bold text-xs transition active:scale-[0.98]"
                >
                    <LucideTrash2 size={16} />
                    <span>Delete Conversation</span>
                </button>
            </div>
        {/if}
    </div>
</div>
