
<script lang="ts">
    import { toasts, removeToast } from '../lib/stores/ui';
    import { fly } from 'svelte/transition';
    import { LucideCheckCircle2, LucideAlertCircle, LucideInfo, LucideAlertTriangle, LucideX } from 'lucide-svelte';
</script>

<div class="fixed top-8 right-8 z-[100] flex flex-col space-y-3 pointer-events-none w-80">
    {#each $toasts as toast (toast.id)}
        <div 
            in:fly={{ x: 50, duration: 300 }}
            out:fly={{ x: 50, duration: 200 }}
            class="pointer-events-auto bg-white/90 backdrop-blur-xl border border-white shadow-[0_10px_30px_-5px_rgba(0,0,0,0.1)] rounded-2xl p-4 flex items-start space-x-3 group relative overflow-hidden"
        >
            <div class="mt-0.5">
                {#if toast.type === 'success'}
                    <LucideCheckCircle2 size={18} class="text-green-500" />
                {:else if toast.type === 'error'}
                    <LucideAlertCircle size={18} class="text-red-500" />
                {:else if toast.type === 'warning'}
                    <LucideAlertTriangle size={18} class="text-amber-500" />
                {:else}
                    <LucideInfo size={18} class="text-blue-500" />
                {/if}
            </div>
            
            <div class="flex-1 pr-4">
                <p class="text-xs font-bold text-gray-800 leading-snug">{toast.message}</p>
            </div>

            <button 
                onclick={() => removeToast(toast.id)}
                class="text-gray-300 hover:text-gray-500 transition-colors"
                aria-label="Close"
            >
                <LucideX size={14} />
            </button>

            <!-- Progress bar for auto-hide -->
            {#if toast.duration && toast.duration > 0}
                <div 
                    class="absolute bottom-0 left-0 h-1 bg-current opacity-10"
                    style="width: 100%; animation: shrink {toast.duration}ms linear forwards; color: {
                        toast.type === 'success' ? '#22c55e' : 
                        toast.type === 'error' ? '#ef4444' : 
                        toast.type === 'warning' ? '#f59e0b' : '#3b82f6'
                    }"
                ></div>
            {/if}
        </div>
    {/each}
</div>

<style>
    @keyframes shrink {
        from { width: 100%; }
        to { width: 0%; }
    }
</style>
