
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
            class="absolute inset-0 bg-entropy-bg/60 backdrop-blur-sm"
            transition:fade={{ duration: 200 }}
            onclick={handleCancel}
        ></div>

        <!-- Modal Content -->
        <div 
            class="bg-entropy-surface rounded-[2.5rem] shadow-[0_30px_100px_-10px_rgba(0,0,0,0.5)] w-full max-w-md overflow-hidden relative z-10"
            transition:scale={{ duration: 300, start: 0.9, opacity: 0 }}
        >
            <div class="p-8 space-y-6">
                <div class="flex items-center space-x-4">
                    <div class="w-12 h-12 rounded-2xl flex items-center justify-center {$modal.type === 'confirm' ? 'bg-amber-100/10 text-amber-500' : 'bg-entropy-primary/10 text-entropy-primary'}">
                        {#if $modal.type === 'confirm'}
                            <LucideShieldAlert size={24} />
                        {:else}
                            <LucideHelpCircle size={24} />
                        {/if}
                    </div>
                    <div>
                        <h2 class="text-xl font-bold text-entropy-text-primary tracking-tight">{$modal.title}</h2>
                        <p class="text-[10px] font-bold text-entropy-text-dim uppercase tracking-wider italic">Action Required</p>
                    </div>
                </div>

                <p class="text-sm font-medium text-entropy-text-secondary leading-relaxed">
                    {$modal.message}
                </p>

                {#if $modal.type === 'prompt'}
                    <div class="space-y-2">
                        <input 
                            bind:value={promptValue}
                            type="text"
                            class="w-full px-5 py-4 bg-entropy-surface-light focus:bg-entropy-surface rounded-2xl outline-none transition-all font-medium text-sm text-entropy-text-primary placeholder:text-entropy-text-dim"
                            placeholder="Type here..."
                        />
                    </div>
                {/if}

                <div class="flex space-x-3 pt-2">
                    <button 
                        onclick={handleCancel}
                        class="flex-1 py-4 bg-entropy-surface-light text-entropy-text-secondary rounded-2xl font-bold text-[10px] uppercase tracking-wider hover:bg-entropy-surface transition-colors"
                    >
                        {$modal.cancelText || 'Cancel'}
                    </button>
                    <button 
                        onclick={handleConfirm}
                        class="flex-1 py-4 {$modal.type === 'confirm' ? 'bg-red-500 hover:bg-red-600' : 'bg-entropy-primary hover:bg-entropy-primary-dim'} text-white rounded-2xl font-bold text-[10px] uppercase tracking-wider shadow-lg transition-all active:scale-[0.98]"
                    >
                        {$modal.confirmText || ($modal.type === 'confirm' ? 'Confirm' : 'OK')}
                    </button>
                </div>
            </div>
        </div>
    </div>
{/if}
