
<script lang="ts">
    import { attachmentStore } from '../lib/attachment_store';
    import { LucideMic, LucidePaperclip, LucideDownload, LucideLoader } from 'lucide-svelte';

    let { msg } = $props();

    let blobUrl = $state<string | null>(null);
    let loading = $state(false);
    let error = $state(false);

    import { signalManager } from '../lib/signal_manager';
    import { toHex } from '../lib/crypto';

    async function loadAttachment() {
        if (msg.attachment.data) {
            blobUrl = URL.createObjectURL(new Blob([msg.attachment.data], {type: msg.attachment.fileType}));
            return;
        }

        loading = true;
        try {
            const data = await attachmentStore.get(msg.id);
            if (data) {
                if (msg.attachment.isV2 && msg.attachment.bundle) {
                    
                    const hexData = toHex(data);
                    const decrypted = await signalManager.decryptMedia(hexData, msg.attachment.bundle);
                    blobUrl = URL.createObjectURL(new Blob([decrypted as any], {type: msg.attachment.fileType}));
                } else {
                    blobUrl = URL.createObjectURL(new Blob([data as any], {type: msg.attachment.fileType}));
                }
            } else {
                error = true;
            }
        } catch (e) {
            console.error("Attachment load error:", e);
            error = true;
        } finally {
            loading = false;
        }
    }

    $effect(() => {
        loadAttachment();
        return () => {
            if (blobUrl) URL.revokeObjectURL(blobUrl);
        };
    });
</script>

{#if msg.type === 'voice_note'}
    <div class="flex items-center space-x-2 py-1 min-w-[200px]">
        <LucideMic size={16} class="text-blue-500" />
        {#if blobUrl}
            <audio controls class="h-8 w-48 scale-90 -ml-4" src={blobUrl}></audio>
        {:else if loading}
            <LucideLoader size={16} class="animate-spin text-gray-400" />
        {:else}
            <span class="text-xs text-red-400">Error loading audio</span>
        {/if}
    </div>
{:else if msg.type === 'file'}
    <div class="flex flex-col space-y-2">
        {#if msg.attachment.fileType?.startsWith('image/') && blobUrl}
            <div class="relative group max-w-sm rounded-lg overflow-hidden border border-black/10 shadow-sm bg-gray-100">
                <img 
                    src={blobUrl} 
                    alt={msg.attachment.fileName} 
                    class="max-h-64 object-contain mx-auto"
                />
                <div class="absolute inset-0 bg-black/40 opacity-0 group-hover:opacity-100 transition-opacity flex items-center justify-center">
                    <a 
                        href={blobUrl} 
                        download={msg.attachment.fileName}
                        class="p-2 bg-white rounded-full text-black hover:scale-110 transition shadow-lg"
                        title="Download Image"
                    >
                        <LucideDownload size={20} />
                    </a>
                </div>
            </div>
        {/if}

        <div class="flex items-center space-x-2 bg-black/5 p-2 rounded border border-black/10">
            <LucidePaperclip size={18} class="text-gray-600" />
            <div class="flex-1 min-w-0">
                <div class="text-xs font-medium truncate">{msg.attachment.fileName}</div>
                <div class="text-[10px] text-gray-500">
                    {(msg.attachment.size || 0) / 1024 > 1024 
                        ? ((msg.attachment.size || 0)/1024/1024).toFixed(1) + ' MB' 
                        : ((msg.attachment.size || 0)/1024).toFixed(1) + ' KB'}
                </div>
            </div>
            {#if blobUrl}
                <a 
                    href={blobUrl} 
                    download={msg.attachment.fileName}
                    class="text-blue-600 hover:text-blue-700 font-bold text-xs"
                >
                    DL
                </a>
            {:else if loading}
                <LucideLoader size={16} class="animate-spin text-gray-400" />
            {:else}
                <button onclick={loadAttachment} class="text-xs text-blue-500 underline">Retry</button>
            {/if}
        </div>
    </div>
{/if}
