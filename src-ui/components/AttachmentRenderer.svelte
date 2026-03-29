<script lang="ts">
    import { LucideMic, LucidePaperclip, LucideDownload, LucideLoader, LucideCheck, LucideImage } from 'lucide-svelte';
    import { convertFileSrc, invoke } from '@tauri-apps/api/core';
    import { getAttachment, markAsDownloaded } from '../lib/actions/chat';
    import { fromBase64 } from '../lib/crypto';
    import VoiceNotePlayer from './VoiceNotePlayer.svelte';

    let { msg, chatId } = $props<{ msg: any, chatId: string }>();

    let blobUrl = $state<string | null>(null);
    let loading = $state(false);
    let error = $state(false);
    let isDownloading = $state(false);
    let downloadSuccess = $state(false);

    $effect(() => {
        downloadSuccess = msg.attachment?.isDownloaded || false;
    });

    $effect(() => {
        // Auto-load image previews
        if (!blobUrl && !loading && !error && msg.attachment) {
            const isImage = msg.attachment.fileType?.startsWith('image/') || 
                          /\.(jpg|jpeg|png|gif|webp)$/i.test(msg.attachment.fileName || '');
            if (isImage || msg.type === 'voice_note' || msg.attachment.originalPath) {
                loadAttachment();
            }
        }
    });

    async function showInFolder() {
        const path = msg.attachment?.vaultPath || msg.attachment?.originalPath;
        if (path) {
            console.debug("[Attachment] Showing in folder:", path);
            await invoke('show_in_folder', { path });
        }
    }

    async function loadAttachment() {
        if (!msg.attachment) return;
        
        const path = msg.attachment.vaultPath || msg.attachment.originalPath;

        // Step 1: Use native-decrypted file on disk (High Performance)
        if (path) {
            blobUrl = convertFileSrc(path);
            loading = false;
            return;
        }

        // Step 2: Handle in-memory data (Small fragments/Standard)
        if (msg.attachment.data) {
            let bytes = msg.attachment.data;
            if (typeof bytes === 'string') bytes = fromBase64(bytes);
            blobUrl = URL.createObjectURL(new Blob([bytes], {type: msg.attachment.fileType}));
            return;
        }

        // Step 3: Fetch from vault (Cold storage retrieval)
        loading = true;
        try {
            const data = await getAttachment(msg.id);
            if (data) {
                blobUrl = URL.createObjectURL(new Blob([data], {type: msg.attachment.fileType}));
            } else {
                error = true;
            }
        } catch (e) {
            error = true;
        } finally {
            loading = false;
        }
    }

    async function manualDownload() {
        if (isDownloading) return;
        const path = msg.attachment?.vaultPath || msg.attachment?.originalPath;
        if (!path) return;

        isDownloading = true;
        try {
            // Since it's already on disk (Native-First), manual download 
            // is just revealing it or moving it. For now, reveal is safer.
            await invoke('show_in_folder', { path });
            downloadSuccess = true;
            if (chatId) markAsDownloaded(chatId, msg.id);
        } catch (e: any) {
            console.error("[Attachment] Reveal failed:", e);
        } finally {
            isDownloading = false;
        }
    }

    $effect(() => {
        loadAttachment();
        return () => {
            if (blobUrl) {
                console.debug("[Attachment] Revoking blob URL:", blobUrl);
                URL.revokeObjectURL(blobUrl);
            }
        };
    });
</script>

{#if msg.type === 'voice_note' || msg.attachment?.fileName === 'voice_note.wav'}
    {#if blobUrl}
        <VoiceNotePlayer src={blobUrl} id={msg.id} isMine={msg.isMine} />
    {:else if loading}
        <div class="flex items-center space-x-2 py-2 px-4 bg-entropy-surface-light rounded-2xl border border-white/5">
            <LucideLoader size={16} class="animate-spin text-entropy-primary" />
            <span class="text-[10px] font-bold text-entropy-text-primary uppercase tracking-widest">Loading Voice Note...</span>
        </div>
    {:else if msg.status === 'sending'}
        <div class="flex items-center space-x-2 py-2 px-4 bg-entropy-primary/10 rounded-2xl animate-pulse">
            <LucideLoader size={16} class="animate-spin text-entropy-primary" />
            <span class="text-[10px] font-bold text-entropy-primary uppercase tracking-wider">Sending...</span>
        </div>
    {:else}
        <div class="flex items-center space-x-2 py-2 px-4 bg-red-500/10 rounded-2xl">
            <LucideMic size={16} class="text-red-500" />
            <span class="text-[10px] font-bold text-red-500 uppercase tracking-widest">Error loading audio</span>
        </div>
    {/if}
{:else if msg.type === 'file'}
    <div class="flex flex-col space-y-2">
        {#if (msg.attachment.fileType?.startsWith('image/') || /\.(jpg|jpeg|png|gif|webp)$/i.test(msg.attachment.fileName || '')) && blobUrl}
            <div class="relative group max-w-sm rounded-lg overflow-hidden shadow-sm bg-entropy-surface-light">
                <img 
                    src={blobUrl} 
                    alt={msg.attachment.fileName} 
                    class="max-h-64 object-contain mx-auto"
                    onerror={() => {
                        console.error("[Attachment] Image failed to load:", blobUrl);
                        // If it's an asset URL and failed, it's likely a permission issue
                        if (blobUrl?.startsWith('asset:')) {
                            console.warn("[Attachment] Asset protocol blocked. Backend config might be missing protocol permissions.");
                        }
                    }}
                />
                <div class="absolute inset-0 bg-black/40 opacity-0 group-hover:opacity-100 transition-opacity flex items-center justify-center backdrop-blur-[2px]">
                    <button 
                        onclick={manualDownload}
                        class="p-2 bg-white rounded-full text-black hover:scale-110 transition shadow-lg disabled:opacity-80 disabled:cursor-wait"
                        title={downloadSuccess ? 'Downloaded' : 'Download Image'}
                        disabled={isDownloading || downloadSuccess}
                    >
                        {#if isDownloading}
                            <LucideLoader size={20} class="animate-spin text-entropy-primary" />
                        {:else if downloadSuccess}
                            <LucideCheck size={20} class="text-green-500" />
                        {:else}
                            <LucideDownload size={20} />
                        {/if}
                    </button>
                </div>
            </div>
        {/if}

        <div class="flex items-center space-x-3 bg-entropy-surface/40 backdrop-blur-sm p-3 rounded-2xl shadow-sm group/file hover:bg-entropy-surface/60 transition-colors">
            <div class="w-10 h-10 rounded-xl bg-entropy-primary/10 flex items-center justify-center text-entropy-primary shrink-0">
                <LucidePaperclip size={20} />
            </div>
            <button 
                onclick={showInFolder}
                class="flex-1 min-w-0 text-left hover:bg-white/5 p-1 rounded transition-colors disabled:cursor-auto"
                disabled={!(msg.attachment.vaultPath || msg.attachment.originalPath)}
                title={(msg.attachment.vaultPath || msg.attachment.originalPath) ? `Show in Folder: ${msg.attachment.vaultPath || msg.attachment.originalPath}` : ''}
            >
                <div class="text-[12px] font-bold truncate {msg.attachment.originalPath ? 'text-entropy-primary' : 'text-entropy-text-primary'} tracking-tight">{msg.attachment.fileName}</div>
                <div class="text-[10px] font-medium text-entropy-text-dim uppercase tracking-wider">
                    {(msg.attachment.size || 0) / 1024 > 1024 
                        ? ((msg.attachment.size || 0)/1024/1024).toFixed(1) + ' MB' 
                        : ((msg.attachment.size || 0)/1024).toFixed(1) + ' KB'}
                </div>
            </button>
        {#if blobUrl}
            <button 
                onclick={manualDownload}
                class="w-8 h-8 rounded-lg {downloadSuccess ? 'bg-entropy-accent' : 'bg-entropy-primary'} text-white flex items-center justify-center hover:bg-opacity-90 transition shadow-md active:scale-95 disabled:opacity-70 disabled:cursor-wait"
                title={downloadSuccess ? 'Downloaded' : 'Download File'}
                disabled={isDownloading || downloadSuccess}
            >
                {#if isDownloading}
                    <LucideLoader size={14} class="animate-spin" />
                {:else if downloadSuccess || blobUrl}
                    <LucideCheck size={14} />
                {:else}
                    <LucideDownload size={14} />
                {/if}
            </button>
        {:else if loading || msg.status === 'sending'}
            <div class="w-8 h-8 flex items-center justify-center">
                <LucideLoader size={16} class="animate-spin text-entropy-primary" />
            </div>
        {:else if msg.attachment.fileType?.startsWith('image/')}
            <div class="w-8 h-8 rounded-lg bg-entropy-primary/10 text-entropy-primary flex items-center justify-center">
                <LucideImage size={16} />
            </div>
        {:else}
            <button onclick={loadAttachment} class="w-8 h-8 rounded-lg bg-red-500/10 text-red-500 flex items-center justify-center hover:bg-red-500/20 italic transition font-black text-[9px]">!</button>
        {/if}
    </div>
</div>
{/if}
