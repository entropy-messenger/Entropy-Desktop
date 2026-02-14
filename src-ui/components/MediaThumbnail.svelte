<script lang="ts">
    import { attachmentStore } from '../lib/attachment_store';
    import { LucideMic, LucideFile, LucideImage, LucideLoader } from 'lucide-svelte';
    import { signalManager } from '../lib/signal_manager';

    let { msg, onclick } = $props<{ msg: any; onclick: () => void }>();

    let blobUrl = $state<string | null>(null);
    let loading = $state(false);

    async function loadThumbnail() {
        if (!msg.attachment || msg.type !== 'file' || !msg.attachment.fileType?.startsWith('image/')) {
            return;
        }

        loading = true;
        try {
            if (msg.attachment.data) {
                blobUrl = URL.createObjectURL(new Blob([msg.attachment.data], {type: msg.attachment.fileType}));
                return;
            }

            const data = await attachmentStore.get(msg.id);
            if (data) {
                if (msg.attachment.isV2 && msg.attachment.bundle) {
                    const decrypted = await signalManager.decryptMedia(data, msg.attachment.bundle);
                    blobUrl = URL.createObjectURL(new Blob([decrypted as any], {type: msg.attachment.fileType}));
                }
            }
        } catch (e) {
            console.error("[MediaThumbnail] Load error:", e);
        } finally {
            loading = false;
        }
    }

    $effect(() => {
        loadThumbnail();
        return () => {
            if (blobUrl) URL.revokeObjectURL(blobUrl);
        };
    });
</script>

<button
    {onclick}
    class="aspect-square bg-entropy-surface-light rounded-lg flex items-center justify-center text-entropy-primary relative group/file overflow-hidden hover:bg-entropy-surface transition-colors"
>
    {#if msg.attachment?.fileType?.startsWith('image/')}
        {#if blobUrl}
            <img src={blobUrl} alt="" class="w-full h-full object-cover" />
            <div class="absolute inset-0 bg-black/20 opacity-0 group-hover/file:opacity-100 flex items-center justify-center transition-opacity duration-200">
                <LucideImage size={24} class="text-white drop-shadow-lg" />
            </div>
        {:else if loading}
            <LucideLoader size={20} class="animate-spin text-entropy-text-dim" />
        {:else}
            <LucideImage size={24} />
        {/if}
        <div class="absolute bottom-0 left-0 right-0 bg-gradient-to-t from-black/60 to-transparent p-1.5">
            <div class="text-[8px] text-white font-bold truncate">{msg.attachment.fileName || 'Image'}</div>
        </div>
    {:else if msg.type === 'voice_note'}
        <LucideMic size={24} />
        <div class="absolute inset-0 bg-entropy-primary/10 opacity-0 group-hover/file:opacity-100 flex items-center justify-center transition-opacity duration-200">
            <div class="text-[9px] font-bold text-entropy-primary">Play</div>
        </div>
    {:else}
        <LucideFile size={24} />
        <div class="absolute bottom-0 left-0 right-0 bg-gradient-to-t from-black/60 to-transparent p-1.5">
            <div class="text-[8px] text-white font-bold truncate">{msg.attachment?.fileName || 'File'}</div>
        </div>
    {/if}
    <div class="absolute top-1 right-1 opacity-0 group-hover/file:opacity-100 transition-opacity">
        <div class="bg-entropy-primary text-white text-[8px] font-black px-1.5 py-0.5 rounded">VIEW</div>
    </div>
</button>
