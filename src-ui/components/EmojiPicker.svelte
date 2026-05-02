<script lang="ts">
  import { onMount } from 'svelte';
  import 'emoji-picker-element';

  let { onSelect, onClose } = $props<{ 
    onSelect: (emoji: string) => void,
    onClose: () => void 
  }>();

  let pickerEl = $state<HTMLElement | null>(null);

  onMount(() => {
    if (pickerEl) {
      pickerEl.addEventListener('emoji-click', (event: any) => {
        onSelect(event.detail.unicode);
      });

      // Handle clicking outside to close
      const handleClickOutside = (event: MouseEvent) => {
        const target = event.target as Element;
        // Don't close if clicking the trigger button (the button handles toggling)
        if (target.closest('#emoji-toggle-btn')) return;
        
        if (pickerEl && !pickerEl.contains(target)) {
          onClose();
        }
      };

      document.addEventListener('mousedown', handleClickOutside);
      return () => document.removeEventListener('mousedown', handleClickOutside);
    }
  });
</script>

<div class="absolute bottom-20 left-4 z-50 premium-shadow rounded-3xl overflow-hidden animate-in fade-in slide-in-from-bottom-2 duration-300 border border-white/10 glass">
  <emoji-picker bind:this={pickerEl} data-source="/emoji-data.json"></emoji-picker>
</div>

<style>
  emoji-picker {
    --num-columns: 8;
    --category-emoji-size: 1.1rem;
    --emoji-size: 1.5rem;
    --emoji-padding: 0.5rem;
    width: 350px;
    height: 420px;
    border-radius: 1.5rem;
    border: none;
    
    /* Premium Glass Theme */
    --background: transparent;
    --border-color: rgba(var(--entropy-text-dim), 0.15);
    --button-hover-background: rgba(var(--entropy-primary), 0.15);
    --button-active-background: rgba(var(--entropy-primary), 0.25);
    --outline-color: rgb(var(--entropy-primary));
    --indicator-color: rgb(var(--entropy-primary));
    --input-border-color: transparent;
    --input-placeholder-color: rgb(var(--entropy-text-dim));
    --input-font-color: rgb(var(--entropy-text-primary));
    --input-background: rgba(var(--entropy-text-dim), 0.1);
    --category-font-color: rgb(var(--entropy-text-secondary));
  }

  /* Search input styling */
  emoji-picker::part(search) {
    border-radius: 1rem;
    padding: 0.5rem 1rem;
    font-weight: 500;
  }

  /* WhatsApp/Telegram style scrollbar */
  emoji-picker::part(scroll-view) {
    scrollbar-width: thin;
    scrollbar-color: rgb(var(--entropy-text-dim) / 0.3) transparent;
    padding-right: 4px;
  }

  :global(.theme-dark) emoji-picker {
    --border-color: rgba(255, 255, 255, 0.08);
    --input-background: rgba(255, 255, 255, 0.05);
  }
</style>
