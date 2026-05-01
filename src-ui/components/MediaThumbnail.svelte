<script lang="ts">
    import { getMediaUrl } from '../lib/actions/chat';
    import { LucideMic, LucideFile, LucideImage, LucideLoader } from 'lucide-svelte';

    let { msg, onclick } = $props<{ msg: any; onclick: () => void }>();

    let thumbUrl = $state<string | null>(null);
    let loading = $state(false);

    async function loadThumbnail() {
        if (!msg.attachment) return;

        // 1. Prioritize pre-rendered thumbnail
        if (msg.attachment.thumbnail) {
            thumbUrl = msg.attachment.thumbnail;
            return;
        }

        // 2. If no thumbnail exists, we strictly do NOT use the proxy here.
        // We show a placeholder until the user choose to VIEW it.
        if (msg.attachment.fileType?.startsWith('image/')) {
            // No-op
        }
    }

    $effect(() => {
        loadThumbnail();
    });
</script>

<button
    {onclick}
    class="aspect-square bg-entropy-surface-light rounded-lg flex items-center justify-center text-entropy-primary relative group/file overflow-hidden hover:bg-entropy-surface transition-colors"
>
    {#if msg.attachment?.fileType?.startsWith('image/')}
        {#if thumbUrl}
            <img src={thumbUrl} alt="" class="w-full h-full object-cover" />
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
