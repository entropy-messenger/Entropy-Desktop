<script lang="ts">
    import { lightbox } from '../lib/stores/ui';
    import { LucidePaperclip, LucideX } from 'lucide-svelte';
    import { fade, scale } from 'svelte/transition';

    function close() {
        lightbox.set(null);
    }

    function handleKeydown(e: KeyboardEvent) {
        if (e.key === 'Escape') close();
    }
</script>

{#if $lightbox}
    <div 
        class="fixed inset-0 z-[10000] bg-black/95 backdrop-blur-2xl flex flex-col p-4 select-none touch-none"
        transition:fade={{ duration: 200 }}
        onclick={close}
        onkeydown={handleKeydown}
        role="button"
        tabindex="0"
    >
        <!-- Header -->
        <div class="flex justify-between items-center z-10 animate-in slide-in-from-top-4 duration-500">
            <div class="flex flex-col">
                <span class="text-white font-bold text-sm truncate max-w-[250px]">{$lightbox.fileName}</span>
                <span class="text-white/40 text-[10px] uppercase tracking-widest">
                    {($lightbox.size || 0) / 1024 > 1024 
                        ? (($lightbox.size || 0)/1024/1024).toFixed(1) + ' MB' 
                        : (($lightbox.size || 0)/1024).toFixed(1) + ' KB'}
                </span>
            </div>
            <button 
                class="w-10 h-10 rounded-full bg-white/10 flex items-center justify-center text-white hover:bg-white/20 active:scale-95 transition-all"
                onclick={close}
            >
                <LucideX size={20} />
            </button>
        </div>

        <!-- Image Container -->
        <div class="flex-1 flex items-center justify-center overflow-hidden p-2 sm:p-8">
            <img 
                src={$lightbox.src} 
                alt={$lightbox.alt}
                class="max-w-full max-h-full object-contain shadow-2xl rounded-sm"
                transition:scale={{ duration: 300, start: 0.95 }}
                onclick={(e) => e.stopPropagation()}
            />
        </div>

        <!-- Footer hint -->
        <div class="py-4 flex justify-center animate-in slide-in-from-bottom-4 duration-500">
            <span class="text-[10px] text-white/30 font-bold uppercase tracking-[0.3em]">Tap anywhere to dismiss</span>
        </div>
    </div>
{/if}
