<script lang="ts">
  import { userStore, messageStore } from '../lib/stores/user';
  import { 
    leaveGroup, addToGroup, toggleBlock, toggleVerification 
  } from '../lib/store';
  import { signalManager } from '../lib/signal_manager';
  import { 
    LucideX, LucideShieldCheck, LucideShieldAlert, LucideInfo,
    LucideImage, LucideLink, LucideTrash2, LucideCheck as LucideCheckIcon, 
    LucideCopy, LucideLoader, LucideExternalLink
  } from 'lucide-svelte';
  import MediaThumbnail from './MediaThumbnail.svelte';
  import { addToast, showConfirm, showPrompt } from '../lib/stores/ui';
  import type { Chat } from '../lib/types';
  
  let { activeChat, onClose, onScrollToMessage } = $props<{ activeChat: Chat, onClose: () => void, onScrollToMessage: (id: string) => void }>();

  let safetyNumber = $state<{ digits: string; isVerified: boolean } | null>(null);
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

  let mediaMessages = $derived(activeChat ? ($messageStore[activeChat.peerHash] || []).filter((m: any) => m.attachment && (m.type === 'file' || m.type === 'voice_note')) : []);
  let linkMessages = $derived(activeChat ? ($messageStore[activeChat.peerHash] || []).filter((m: any) => m.linkPreview?.url) : []);
  
  let inviteCopied = $state(false);
  const copyInvite = () => {
      if (!activeChat) return;
      const link = `entropy://${activeChat.peerHash}`;
      navigator.clipboard.writeText(link);
      inviteCopied = true;
      setTimeout(() => inviteCopied = false, 2000);
  };
</script>

<div class="w-80 bg-entropy-bg flex flex-col animate-in slide-in-from-right duration-300 z-[40]">
    <div class="p-4 flex justify-between items-center bg-entropy-surface">
        <h2 class="font-bold text-entropy-text-primary flex items-center space-x-2"><LucideInfo size={18} /><span>Info & Media</span></h2>
        <button onclick={onClose} class="text-entropy-text-dim hover:text-red-500 transition"><LucideX size={20} /></button>
    </div>
    
    <div class="p-6 flex-1 overflow-y-auto custom-scrollbar space-y-8">
        <div class="flex flex-col items-center space-y-4">
            <div class="w-24 h-24 rounded-3xl bg-entropy-surface flex items-center justify-center text-entropy-primary text-3xl font-bold shadow-xl">
                {#if activeChat.pfp}<img src={activeChat.pfp} alt="" class="w-full h-full object-cover rounded-3xl" />{:else}{(activeChat.localNickname || activeChat.peerAlias || '?')[0].toUpperCase()}{/if}
            </div>
            <div class="text-center">
                <div class="flex items-center justify-center space-x-2">
                    <h3 class="text-xl font-bold text-entropy-text-primary">{activeChat.localNickname || activeChat.peerAlias || 'Peer'}</h3>
                    {#if !activeChat.isGroup && activeChat.isVerified}
                        <LucideShieldCheck size={18} class="text-entropy-accent" />
                    {/if}
                </div>
                {#if activeChat.localNickname && activeChat.peerAlias}
                    <p class="text-[10px] font-bold text-entropy-primary uppercase mb-1 tracking-wide">Alias: {activeChat.peerAlias}</p>
                {/if}
                <p class="text-[11px] font-mono text-entropy-text-secondary break-all opacity-80">{activeChat.peerHash}</p>
            </div>
        </div>

        {#if !activeChat.isGroup}
            <div class="space-y-4 pt-2">
                 <div class="flex items-center justify-between">
                    <h4 class="text-[10px] font-black text-entropy-text-dim uppercase tracking-[0.1em]">Identity Verification</h4>
                     {#if activeChat.isVerified}
                        <div class="flex items-center space-x-1 text-entropy-accent animate-in fade-in zoom-in duration-300">
                            <LucideShieldCheck size={12} />
                            <span class="text-[9px] font-black uppercase">Verified</span>
                        </div>
                    {:else}
                        <div class="flex items-center space-x-1 text-red-500/80">
                            <LucideShieldAlert size={12} />
                            <span class="text-[9px] font-black uppercase">Unverified</span>
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
                                    const verified = !activeChat!.isVerified;
                                    await toggleVerification(activeChat!.peerHash, verified);
                                    if (safetyNumber) safetyNumber.isVerified = verified;
                                    addToast(verified ? "Session Verified" : "Verification Removed", verified ? 'success' : 'info');
                                }}
                                class="w-full py-2.5 rounded-xl text-[10px] font-black uppercase tracking-widest transition-all active:scale-[0.98] shadow-lg
                                {activeChat.isVerified 
                                    ? 'bg-red-500/10 text-red-500 hover:bg-red-500/20' 
                                    : 'bg-entropy-accent text-white hover:bg-entropy-accent/90 shadow-entropy-accent/20'}"
                            >
                                {activeChat.isVerified ? 'Remove Verification' : 'Verify Identity'}
                            </button>
                            <p class="text-[9px] text-entropy-text-dim text-center mt-3 leading-relaxed px-2">
                                Verify the safety number above with this contact via another secure channel.
                            </p>
                        </div>
                    {:else}
                        <div class="text-[10px] text-center text-red-500/80 font-bold py-2">Encryption session not established.</div>
                    {/if}
                 </div>
            </div>
        {/if}
        
        {#if activeChat.isGroup}
            <div class="space-y-2">
                <h4 class="text-xs font-bold text-entropy-text-dim uppercase tracking-widest">Group Invite Link</h4>
                <div class="bg-entropy-surface-light p-3 rounded-xl flex items-center justify-between">
                    <span class="text-[11px] font-mono text-entropy-primary truncate">entropy://{activeChat.peerHash.slice(0, 32)}...</span>
                    <button onclick={copyInvite} class="p-2 hover:bg-entropy-primary/10 rounded-lg text-entropy-primary transition">
                        {#if inviteCopied}<LucideCheckIcon size={16} />{:else}<LucideCopy size={16} />{/if}
                    </button>
                </div>
            </div>

            <div class="space-y-2">
                 <div class="flex justify-between items-center">
                    <h4 class="text-xs font-bold text-entropy-text-dim uppercase tracking-widest">Members ({activeChat.members?.length || 0})</h4>
                    <button onclick={async () => {
                        let input = await showPrompt("Enter Peer Hash, Nickname, or link:", "", "Add Member");
                        if (input) {
                            input = input.trim().replace(/^entropy:\/\//, '');
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
                        <div class="flex items-center space-x-2 bg-entropy-surface-light p-2 rounded-lg">
                            <div class="w-5 h-5 rounded-md bg-entropy-primary/20 flex items-center justify-center text-[8px] font-bold text-entropy-primary">
                                {member.slice(0, 2).toUpperCase()}
                            </div>
                            <span class="text-[10px] font-mono text-entropy-text-secondary truncate flex-1">{member.slice(0, 16)}...</span>
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

        <div class="space-y-4">
            <div class="flex justify-between items-center">
                <h4 class="text-xs font-bold text-entropy-text-dim uppercase tracking-wider">Shared Links</h4>
                <LucideLink size={14} class="text-entropy-text-dim" />
            </div>
            {#if linkMessages.length === 0}
                <div class="bg-entropy-surface-light rounded-2xl h-24 flex items-center justify-center text-xs text-entropy-text-dim">No links shared yet</div>
            {:else}
                <div class="space-y-2">
                     {#each linkMessages.slice(0, 5) as m}
                        <a href={m.linkPreview!.url} target="_blank" rel="noopener noreferrer" class="block p-3 bg-entropy-surface-light hover:bg-entropy-surface rounded-xl transition duration-200 group/link">
                            <div class="flex items-start space-x-2">
                                <LucideExternalLink size={12} class="text-entropy-accent shrink-0 mt-0.5 group-hover/link:text-entropy-primary transition" />
                                <div class="flex-1 min-w-0">
                                    <div class="text-[11px] font-bold text-entropy-text-primary truncate">{m.linkPreview!.title || m.linkPreview!.url}</div>
                                    <div class="text-[10px] text-entropy-text-secondary truncate mt-0.5">{m.linkPreview!.url}</div>
                                    {#if m.linkPreview!.description}
                                        <div class="text-[9px] text-entropy-text-dim line-clamp-2 mt-1 leading-relaxed">{m.linkPreview!.description}</div>
                                    {/if}
                                </div>
                            </div>
                        </a>
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
        {/if}
    </div>
</div>
