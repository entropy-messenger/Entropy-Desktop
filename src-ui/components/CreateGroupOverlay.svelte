<script lang="ts">
  import { userStore } from '../lib/stores/user';
  import { lookupNickname } from '../lib/actions/contacts';
  import { createGroup } from '../lib/actions/groups';
  import { addToast } from '../lib/stores/ui';
  import { LucideUsers, LucideX, LucidePlus, LucideCheckCircle2 } from 'lucide-svelte';

  let { onClose } = $props<{ onClose: () => void }>();

  let groupName = $state("");
  let groupMembers = $state<string[]>([]);
  let memberInput = $state("");

  const addMember = async () => {
      let input = memberInput.trim().replace(/^entropy:\/\//, '');
      if (!input) return;

      if (input.length === 64 && /^[0-9a-fA-F]+$/.test(input)) {
          if (!groupMembers.includes(input)) groupMembers = [...groupMembers, input];
      } else {
          const hash = await lookupNickname(input);
          if (hash && !groupMembers.includes(hash)) {
              groupMembers = [...groupMembers, hash];
          } else if (!hash) {
              addToast("Could not find user with that hash or nickname.", 'error');
          }
      }
      memberInput = "";
  };

  const removeMember = (m: string) => {
      groupMembers = groupMembers.filter(x => x !== m);
  };

  const toggleMember = (m: string) => {
      if (groupMembers.includes(m)) {
          removeMember(m);
      } else {
          groupMembers = [...groupMembers, m];
      }
  };

  const handleCreateGroup = async () => {
      if (!groupName || groupMembers.length === 0) return;
      await createGroup(groupName, groupMembers);
      groupName = "";
      groupMembers = [];
      onClose();
  };
</script>

<div class="absolute inset-0 bg-entropy-bg z-[60] flex flex-col animate-in slide-in-from-right duration-300">
    <div class="p-4 flex justify-between items-center bg-entropy-surface">
        <h2 class="font-bold text-entropy-text-primary flex items-center space-x-2"><LucideUsers size={18} /><span>New Group</span></h2>
        <button onclick={onClose} class="text-entropy-text-secondary" aria-label="Close panel"><LucideX size={20} /></button>
    </div>
    <div class="p-6 flex-1 space-y-6 overflow-y-auto custom-scrollbar">
        <div class="space-y-2">
            <label for="group-name-input" class="text-xs font-bold text-entropy-text-dim uppercase">Group Name</label>
            <input id="group-name-input" bind:value={groupName} placeholder="Enter group name..." class="w-full p-3 bg-entropy-surface-light rounded-xl border-none focus:ring-2 focus:ring-entropy-primary/20" />
        </div>
        <div class="space-y-3">
            <label for="member-input" class="text-xs font-bold text-entropy-text-dim uppercase">Add Members</label>
            <div class="flex space-x-2">
                <input id="member-input" bind:value={memberInput} placeholder="Hash or Nickname..." class="flex-1 p-3 bg-entropy-surface-light rounded-xl border-none text-xs" onkeydown={(e) => e.key === 'Enter' && addMember()} />
                <button onclick={addMember} aria-label="Add Member" class="bg-entropy-primary text-white p-3 rounded-xl disabled:opacity-50" disabled={!memberInput}><LucidePlus size={20}/></button>
            </div>
        </div>
            
        <div class="space-y-4 pt-2">
            <div class="text-[10px] font-bold text-entropy-text-dim uppercase tracking-widest">Select from Contacts</div>
            <div class="space-y-1 max-h-48 overflow-y-auto custom-scrollbar">
                {#each Object.values($userStore.chats).filter(c => !c.isGroup) as contact}
                    <button 
                        onclick={() => toggleMember(contact.peerHash)}
                        class="w-full flex items-center justify-between p-2 rounded-xl border-2 transition {groupMembers.includes(contact.peerHash) ? 'border-entropy-primary bg-entropy-surface' : 'border-transparent bg-entropy-surface-light'}"
                    >
                        <div class="flex items-center space-x-3">
                            <div class="w-8 h-8 rounded-full bg-entropy-surface flex items-center justify-center text-[10px] font-bold text-entropy-primary">
                                {(contact.localNickname || contact.peerNickname || "?")[0].toUpperCase()}
                            </div>
                            <div class="text-left">
                                <div class="text-xs font-bold text-entropy-text-primary">{contact.localNickname || contact.peerNickname || contact.peerHash.slice(0, 8)}</div>
                                <div class="text-[9px] font-mono text-entropy-text-dim">{contact.peerHash.slice(0, 16)}...</div>
                            </div>
                        </div>
                        {#if groupMembers.includes(contact.peerHash)}
                            <LucideCheckCircle2 size={16} class="text-entropy-primary" />
                        {/if}
                    </button>
                {/each}
            </div>
        </div>
        <div class="space-y-4 pt-4 border-t border-entropy-surface">
            <button onclick={handleCreateGroup} disabled={!groupName || groupMembers.length === 0} class="w-full py-4 bg-entropy-primary text-white rounded-2xl font-bold shadow-lg active:scale-[0.98] transition disabled:opacity-50 disabled:active:scale-100 flex items-center justify-center space-x-2">
                <LucideUsers size={20} />
                <span>Create Group Chat</span>
            </button>
        </div>
    </div>
</div>
