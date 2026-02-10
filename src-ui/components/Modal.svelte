
<script lang="ts">
    import { modal } from '../lib/stores/ui';
    import { fade, scale } from 'svelte/transition';
    import { LucideShieldAlert, LucideHelpCircle } from 'lucide-svelte';

    let promptValue = $state("");

    $effect(() => {
        if ($modal?.type === 'prompt') {
            promptValue = $modal.defaultValue || "";
        }
    });

    const handleConfirm = () => {
        if ($modal) {
            $modal.onConfirm($modal.type === 'prompt' ? promptValue : undefined);
        }
    };

    const handleCancel = () => {
        if ($modal) {
            $modal.onCancel();
        }
    };

    const handleKeydown = (e: KeyboardEvent) => {
        if (e.key === 'Enter') handleConfirm();
        if (e.key === 'Escape') handleCancel();
    };
</script>

{#if $modal}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div 
        class="fixed inset-0 z-[200] flex items-center justify-center p-6"
        onkeydown={handleKeydown}
    >
        <!-- Backdrop -->
        <div 
            class="absolute inset-0 bg-gray-900/40 backdrop-blur-sm"
            transition:fade={{ duration: 200 }}
            onclick={handleCancel}
        ></div>

        <!-- Modal Content -->
        <div 
            class="bg-white rounded-[2.5rem] shadow-[0_30px_100px_-10px_rgba(0,0,0,0.3)] w-full max-w-md overflow-hidden relative z-10 border border-white"
            transition:scale={{ duration: 300, start: 0.9, opacity: 0 }}
        >
            <div class="p-8 space-y-6">
                <div class="flex items-center space-x-4">
                    <div class="w-12 h-12 rounded-2xl flex items-center justify-center {$modal.type === 'confirm' ? 'bg-amber-100 text-amber-600' : 'bg-blue-100 text-blue-600'}">
                        {#if $modal.type === 'confirm'}
                            <LucideShieldAlert size={24} />
                        {:else}
                            <LucideHelpCircle size={24} />
                        {/if}
                    </div>
                    <div>
                        <h2 class="text-xl font-black text-gray-900 tracking-tight">{$modal.title}</h2>
                        <p class="text-xs font-bold text-gray-400 uppercase tracking-widest italic">Action Required</p>
                    </div>
                </div>

                <p class="text-sm font-medium text-gray-600 leading-relaxed">
                    {$modal.message}
                </p>

                {#if $modal.type === 'prompt'}
                    <div class="space-y-2">
                        <input 
                            bind:value={promptValue}
                            type="text"
                            class="w-full px-5 py-4 bg-gray-50 border-2 border-transparent focus:border-blue-500/20 focus:bg-white rounded-2xl outline-none transition-all font-medium text-sm"
                            placeholder="Type here..."
                            onmount={(el) => (el as HTMLInputElement).focus()}
                        />
                    </div>
                {/if}

                <div class="flex space-x-3 pt-2">
                    <button 
                        onclick={handleCancel}
                        class="flex-1 py-4 bg-gray-50 text-gray-500 rounded-2xl font-bold text-xs uppercase tracking-widest hover:bg-gray-100 transition-colors"
                    >
                        {$modal.cancelText || 'Cancel'}
                    </button>
                    <button 
                        onclick={handleConfirm}
                        class="flex-1 py-4 {$modal.type === 'confirm' ? 'bg-red-500 hover:bg-red-600' : 'bg-blue-600 hover:bg-blue-700'} text-white rounded-2xl font-bold text-xs uppercase tracking-widest shadow-lg transition-all active:scale-95"
                    >
                        {$modal.confirmText || ($modal.type === 'confirm' ? 'Confirm' : 'OK')}
                    </button>
                </div>
            </div>
        </div>
    </div>
{/if}
