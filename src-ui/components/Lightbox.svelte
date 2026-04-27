<script lang="ts">
    import { lightbox, addToast } from '../lib/stores/ui';
    import { LucideX, LucideDownload, LucideArrowLeft, LucideShare2, LucideInfo, LucideLoader } from 'lucide-svelte';
    import { fade, scale, fly } from 'svelte/transition';
    import { invoke } from '@tauri-apps/api/core';
    import VideoPlayer from './VideoPlayer.svelte';
    import { getAttachment } from '../lib/actions/chat';

    let loading = $state(false);
    let blobUrl = $state<string | null>(null);

    $effect(() => {
        const lb = $lightbox;
        if (lb) {
            if (lb.src) {
                blobUrl = lb.src;
            } else if (lb.id) {
                loadFullMedia(lb.id, lb.fileType || 'application/octet-stream');
            }
        }
        
        return () => {
            if (blobUrl && blobUrl.startsWith('blob:')) {
                URL.revokeObjectURL(blobUrl);
            }
            blobUrl = null;
            loading = false;
        };
    });

    async function loadFullMedia(id: string, type: string) {
        loading = true;
        try {
            const data = await getAttachment(id);
            if (data) {
                blobUrl = URL.createObjectURL(new Blob([data], { type }));
            } else {
                addToast("Failed to fetch attachment", 'error');
                close();
            }
        } catch (e) {
            addToast("Failed to decrypt media", 'error');
            close();
        } finally {
            loading = false;
        }
    }

    function close() {
        lightbox.set(null);
    }

    function handleKeydown(e: KeyboardEvent) {
        if (e.key === 'Escape') close();
    }

    async function downloadImage() {
        if (!$lightbox || !blobUrl) return;
        try {
            const { save } = await import('@tauri-apps/plugin-dialog');
            const targetPath = await save({ defaultPath: $lightbox.fileName });
            
            if (targetPath) {
                // Since we have the blob data here, we could potentially use it,
                // but the backend command db_export_media expects a path.
                // However, we can use plugin-fs to write the data if we have it.
                const { writeFile } = await import('@tauri-apps/plugin-fs');
                
                // Fetch the blob data again or use the cached version
                const response = await fetch(blobUrl);
                const arrayBuffer = await response.arrayBuffer();
                await writeFile(targetPath, new Uint8Array(arrayBuffer));
                
                addToast("Saved to: " + targetPath.split(/[/\\]/).pop(), 'success');
            }
        } catch (e) {
            addToast("Save failed", 'error');
        }
    }

    const formatTimestamp = (ts?: number) => {
        if (!ts) return '';
        return new Date(ts).toLocaleString([], { 
            month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' 
        });
    };
</script>

{#if $lightbox}
    <div 
        class="fixed inset-0 z-[10000] bg-black flex flex-col select-none touch-none overflow-hidden"
        transition:fade={{ duration: 250 }}
        onkeydown={handleKeydown}
        role="button"
        tabindex="0"
    >
        <!-- Top Bar (WhatsApp Style) -->
        <div 
            class="absolute top-0 inset-x-0 z-20 bg-gradient-to-b from-black/80 via-black/40 to-transparent flex items-center justify-between px-4 sm:px-6"
            style="padding-top: calc(var(--sat, 0px) + 1.5rem); height: calc(var(--sat, 0px) + 5.5rem);"
            transition:fly={{ y: -20, duration: 300 }}
        >
            <div class="flex items-center space-x-4 max-w-[70%]">
                <button 
                    onclick={close}
                    class="p-2 -ml-2 text-white/90 hover:bg-white/10 rounded-full transition-colors active:scale-90"
                >
                    <LucideArrowLeft size={24} />
                </button>
                <div class="flex flex-col min-w-0">
                    <span class="text-white font-bold text-base truncate">
                        {$lightbox.senderNickname || 'User'}
                    </span>
                    <span class="text-white/60 text-[11px] font-medium tracking-tight">
                        {formatTimestamp($lightbox.timestamp)}
                    </span>
                </div>
            </div>

            <div class="flex items-center space-x-1 sm:space-x-3">
                <button 
                    onclick={downloadImage}
                    class="p-2.5 text-white/80 hover:text-white hover:bg-white/10 rounded-full transition-all active:scale-90"
                    title="Save to device"
                >
                    <LucideDownload size={22} />
                </button>
                <button 
                    class="p-2.5 text-white/80 hover:text-white hover:bg-white/10 rounded-full transition-all active:scale-90"
                    title="Forward"
                >
                    <LucideShare2 size={22} />
                </button>
                <button 
                    onclick={close}
                    class="p-2.5 text-white/80 hover:text-white hover:bg-white/10 rounded-full transition-all active:scale-90 ml-1"
                >
                    <LucideX size={24} />
                </button>
            </div>
        </div>

        <!-- Media Container -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div 
            class="flex-1 flex items-center justify-center p-0 sm:p-4 z-10 min-h-0 min-w-0 w-full"
            style="padding-top: calc(var(--sat, 0px) + 6rem); padding-bottom: {$lightbox.content ? 'calc(var(--sab, 0px) + 8rem)' : 'calc(var(--sab, 0px) + 2rem)'};"
            onclick={close}
        >
            {#if loading}
                <div class="flex flex-col items-center space-y-4">
                    <div class="relative">
                        <LucideLoader size={48} class="animate-spin text-white/20" />
                        <div class="absolute inset-0 flex items-center justify-center">
                            <LucideLoader size={24} class="animate-spin text-white" style="animation-direction: reverse; animation-duration: 0.5s;" />
                        </div>
                    </div>
                    <span class="text-[10px] font-black text-white/40 uppercase tracking-[0.3em]">Decrypting Sovereign Media</span>
                </div>
            {:else if blobUrl}
                {#if $lightbox.type === 'video'}
                    <div class="w-full max-w-5xl aspect-video rounded-lg overflow-hidden shadow-2xl" onclick={(e) => e.stopPropagation()}>
                        <VideoPlayer src={blobUrl} expanded={true} />
                    </div>
                {:else}
                    <img 
                        src={blobUrl} 
                        alt={$lightbox.alt}
                        class="max-w-full max-h-full object-contain shadow-[0_0_50px_rgba(0,0,0,0.5)]"
                        transition:scale={{ duration: 400, start: 0.9, opacity: 0 }}
                        onclick={(e) => e.stopPropagation()}
                    />
                {/if}
            {/if}
        </div>

        <!-- Bottom Caption Bar (WhatsApp Style) -->
        {#if $lightbox.content}
            <div 
                class="absolute bottom-0 inset-x-0 z-20 bg-gradient-to-t from-black/90 via-black/60 to-transparent p-6 flex justify-center"
                style="padding-bottom: calc(var(--sab) + 2.5rem);"
                transition:fly={{ y: 20, duration: 300 }}
            >
                <div class="max-w-3xl w-full flex justify-center items-center text-center">
                    <p class="text-white text-sm sm:text-base font-medium leading-relaxed drop-shadow-md select-text cursor-auto px-4 text-center break-words max-w-full">
                        {$lightbox.content}
                    </p>
                </div>
            </div>
        {:else}
            <!-- Fallback dismissal hint -->
            <div class="absolute bottom-6 inset-x-0 flex justify-center opacity-30 pointer-events-none">
                <span class="text-[10px] text-white font-bold uppercase tracking-[0.4em]">Tap to dismiss</span>
            </div>
        {/if}
    </div>
{/if}

<style>
    /* Ensure the bars look great on mobile browsers with address bars */
    div {
        scrollbar-width: none;
    }
    div::-webkit-scrollbar {
        display: none;
    }
</style>
