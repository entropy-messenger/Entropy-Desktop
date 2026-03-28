<script lang="ts">
  import { userStore } from '../lib/stores/user';
  import { 
    leaveGroup, bulkDelete, bulkStar, toggleBlock, setLocalNickname 
  } from '../lib/store';
  import { 
    LucideMoreVertical, LucideSearch, LucideCheckCircle, LucideEdit2, 
    LucideStar, LucideTrash2, LucideInfo, LucideCheck as LucideCheckIcon, LucideBan, LucideShieldCheck, LucideX
  } from 'lucide-svelte';
  import { addToast, showConfirm, showPrompt } from '../lib/stores/ui';
  import type { Chat } from '../lib/types';

  let { 
    activeChat, 
    selectionMode, 
    selectedIds,
    showMessageSearch,
    onViewImage,
    onCancelSelection,
    onToggleSearch,
    onShowGallery,
    onSelectionModeChange
  } = $props<{
    activeChat: Chat;
    selectionMode: boolean;
    selectedIds: string[];
    showMessageSearch: boolean;
    onViewImage: () => void;
    onCancelSelection: () => void;
    onToggleSearch: () => void;
    onShowGallery: () => void;
    onSelectionModeChange: (mode: boolean) => void;
  }>();

  let showOptions = $state(false);

  const handleBulkStar = () => { 
      if (activeChat) { 
          bulkStar(activeChat.peerHash, selectedIds); 
          onCancelSelection(); 
      } 
  };
  
  const handleBulkDelete = async () => { 
    if (activeChat && await showConfirm(`Delete ${selectedIds.length} messages?`, "Delete Messages")) { 
        bulkDelete(activeChat.peerHash, selectedIds); 
        onCancelSelection(); 
    } 
  };

  const handleSetLocalNickname = async () => {
      if (!activeChat) return;
      try {
          const val = await showPrompt("Set a local nickname for this contact:", activeChat.localNickname || activeChat.peerAlias || "", "Local Nickname");
          if (val !== null) {
              await setLocalNickname(activeChat.peerHash, val.trim() || null);
              addToast("Nickname updated", 'success');
          }
      } catch (e) {
          console.error("Failed to set nickname:", e);
          addToast("Failed to set nickname", 'error');
      }
  };
</script>

<div class="bg-entropy-surface/95 backdrop-blur-md p-3 px-4 flex justify-between items-center shadow-sm z-30">
    <div class="flex items-center space-x-3 overflow-hidden cursor-pointer" onclick={onViewImage} onkeydown={(e) => e.key === 'Enter' && onViewImage()} role="button" tabindex="0">
        <button 
            class="w-10 h-10 rounded-xl bg-gradient-to-tr {activeChat.isGroup ? 'from-purple-500 to-indigo-600' : 'from-blue-400 to-blue-600'} shrink-0 flex items-center justify-center text-white font-bold shadow-sm relative overflow-hidden active:scale-95 transition-transform"
        >
            {#if activeChat.pfp}
                <img src={activeChat.pfp} alt="" class="w-full h-full object-cover" />
            {:else}
                {activeChat.peerAlias ? activeChat.peerAlias[0].toUpperCase() : '?'}
            {/if}
        </button>
        <div class="min-w-0">
            <div class="flex items-center space-x-2">
                <div class="font-bold text-entropy-text-primary leading-tight truncate">
                    {activeChat.localNickname || activeChat.peerAlias || activeChat.peerHash.slice(0, 12)}
                </div>
                {#if activeChat.isVerified}
                    <LucideShieldCheck size={14} class="text-entropy-accent" />
                {/if}
            </div>
            <div class="text-[11px] font-medium h-4 truncate {activeChat.isTyping ? 'text-entropy-accent animate-pulse' : 'text-entropy-text-dim'}">
                {activeChat.isTyping ? "typing..." : ""}
            </div>
        </div>
    </div>

    <div class="flex items-center space-x-1">
        {#if selectionMode}
            <div class="flex items-center bg-entropy-primary/10 px-3 py-1 rounded-full mr-2">
                <span class="text-xs font-bold text-entropy-primary mr-4">{selectedIds.length} selected</span>
                <button onclick={handleBulkStar} class="p-2 text-entropy-primary hover:bg-entropy-primary/20 rounded-full transition"><LucideStar size={18} /></button>
                <button onclick={handleBulkDelete} class="p-2 text-red-600 hover:bg-red-100/20 rounded-full transition"><LucideTrash2 size={18} /></button>
                <button onclick={onCancelSelection} class="p-2 text-entropy-text-dim hover:bg-entropy-surface-light rounded-full transition ml-1"><LucideX size={18} /></button>
            </div>
        {:else}
            <button onclick={() => onSelectionModeChange(true)} class="p-2 text-entropy-text-dim hover:text-entropy-primary hover:bg-entropy-surface-light rounded-full transition" title="Select Messages"><LucideCheckCircle size={20} /></button>
            <button onclick={onToggleSearch} class="p-2 {showMessageSearch ? 'text-entropy-primary bg-entropy-primary/10' : 'text-entropy-text-dim'} hover:text-entropy-primary hover:bg-entropy-surface-light rounded-full transition"><LucideSearch size={20} /></button>
            <div class="relative">
                <button onclick={() => showOptions = !showOptions} class="p-2 text-entropy-text-dim hover:text-entropy-primary hover:bg-entropy-surface-light rounded-full transition"><LucideMoreVertical size={20} /></button>
                {#if showOptions}
                    <div class="absolute right-0 mt-2 w-56 bg-entropy-surface rounded-xl shadow-2xl py-2 z-[100] animate-in fade-in zoom-in-95 duration-150" onclick={() => showOptions = false} onkeypress={(e) => e.key === 'Enter' && (showOptions = false)} role="button" tabindex="0">
                        <button onclick={() => {onShowGallery(); showOptions = false;}} class="w-full text-left px-4 py-2 text-sm text-entropy-text-secondary hover:bg-entropy-surface-light flex items-center space-x-3"><LucideInfo size={16} /> <span>Contact Info</span></button>
                        <button onclick={() => {handleSetLocalNickname(); showOptions = false;}} class="w-full text-left px-4 py-2 text-sm text-entropy-text-secondary hover:bg-entropy-surface-light flex items-center space-x-3"><LucideEdit2 size={16} /> <span>Set Nickname</span></button>
                        <button onclick={() => {onSelectionModeChange(true); showOptions = false;}} class="w-full text-left px-4 py-2 text-sm text-entropy-text-secondary hover:bg-entropy-surface-light flex items-center space-x-3"><LucideCheckIcon size={16} /> <span>Select Messages</span></button>
                        <div class="h-px bg-entropy-border my-1"></div>
                        {#if activeChat.isGroup}
                            <button onclick={async () => { if (await showConfirm("Are you sure you want to leave this group?", "Leave Group")) { leaveGroup(activeChat.peerHash); showOptions = false; } }} class="w-full text-left px-4 py-2 text-sm text-red-600 hover:bg-red-500/10 flex items-center space-x-3">
                                <LucideTrash2 size={16} /> <span>Leave Group</span>
                            </button>
                        {:else}
                            <button onclick={() => {toggleBlock(activeChat.peerHash); showOptions = false;}} class="w-full text-left px-4 py-2 text-sm text-red-600 hover:bg-red-500/10 flex items-center space-x-3">
                                <LucideBan size={16} /> <span>{$userStore.blockedHashes.includes(activeChat.peerHash) ? 'Unblock Contact' : 'Block Contact'}</span>
                            </button>
                        {/if}
                    </div>
                {/if}
            </div>
        {/if}
    </div>
</div>
