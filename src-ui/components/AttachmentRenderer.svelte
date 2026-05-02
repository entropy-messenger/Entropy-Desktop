<script lang="ts">
    import { 
        LucideMic, LucidePaperclip, LucideDownload, LucideLoader, 
        LucideCheck, LucidePlay, LucideClock, LucideCheckCheck, LucideX, LucideStar,
        LucideAlertCircle, LucideRefreshCw
    } from 'lucide-svelte';
    import { userStore } from '../lib/stores/user';
    import { convertFileSrc, invoke } from '@tauri-apps/api/core';
    import { getMediaUrl, markAsDownloaded } from '../lib/actions/chat';
    import { fromBase64 } from '../lib/crypto';
    import { addToast, lightbox, contextMenu, mediaProxyPort } from '../lib/stores/ui';
    import { get } from 'svelte/store';
    import { transfers } from '../lib/stores/transfers';
    import VoiceNotePlayer from './VoiceNotePlayer.svelte';
    import VideoPlayer from './VideoPlayer.svelte';

    let { msg, chatId, isMobile = false } = $props<{ msg: any, chatId: string, isMobile?: boolean }>();

    let mediaUrl = $state<string | null>(null);
    let loading = $state(false);
    let failed = $state(false);
    let error = $state(false);
    let isExporting = $state(false);
    let isMine = $derived(msg.senderHash === $userStore.identityHash || msg.isMine);
    
    // Consolidate exported path logic
    let exportedPath = $state<string | null>(null);
    $effect(() => {
        exportedPath = msg.attachment?.exportedPath || (isMine && !msg.attachment?.vaultPath ? msg.attachment?.originalPath : null);
    });

    // Cleanup on unmount
    $effect(() => {
        return () => {
            mediaUrl = null;
        };
    });

    // Thumbnails are used in the bubble; proxy is used for lightbox/export.
    // We strictly do NOT pre-load the full media in the bubble to prevent background decryption.
    $effect(() => {
        if ((isImage || isVideo) && msg.attachment?.vaultPath && !mediaUrl && !loading && !failed) {
            // No auto-load. We only load on explicit user interaction (Lightbox/Export).
        }
    });

    let isImage = $derived(msg.attachment?.fileType?.startsWith('image/') || /\.(jpg|jpeg|png|gif|webp)$/i.test(msg.attachment?.fileName || ''));
    let isVideo = $derived(msg.attachment?.fileType?.startsWith('video/') || /\.(mp4|webm|mov|ogg)$/i.test(msg.attachment?.fileName || ''));
    
    let fileSize = $derived.by(() => {
        const bytes = msg.attachment?.size || 0;
        if (bytes / 1024 > 1024) return (bytes / 1024 / 1024).toFixed(1) + ' MB';
        return (bytes / 1024).toFixed(1) + ' KB';
    });

    let activeTransfer = $derived.by(() => {
        const t_id = msg.attachment?.transferId;
        if (!t_id) return null;
        return $transfers[t_id] || null;
    });

    let progress = $derived(activeTransfer ? Math.round((activeTransfer.current / activeTransfer.total) * 100) : 0);

    async function loadAttachment() {
        // loadAttachment is now only used for getting a URL for specific interactions if needed.
        if (!msg.attachment || mediaUrl || loading || failed) return;
        
        loading = true;
        try {
            const url = await getMediaUrl(msg.id, msg.attachment.fileType || 'application/octet-stream');
            mediaUrl = url;
        } catch (e) {

            failed = true;
        } finally {
            loading = false;
        }
    }

    async function openSavedFile() {
        const path = exportedPath || (isMine ? (msg.attachment?.originalPath || msg.attachment?.path) : null);
        if (!path) return;
        try {
            await invoke('open_file', { path });
        } catch (e) {
            addToast("Failed to open file", 'error');
        }
    }

    function openContextMenu(e: MouseEvent) {
        e.preventDefault();
        e.stopPropagation();

        contextMenu.set({
            x: e.clientX,
            y: e.clientY,
            visible: true,
            fileName: msg.attachment?.fileName || 'File',
            label: isMobile ? 'Save to Device' : (exportedPath || isMine ? 'Open File' : 'Save to Device'),
            onSave: doExport
        });
    }

    async function doExport() {
        if ((exportedPath || isMine) && !(isMobile && !isImage && !isVideo)) {
            openSavedFile();
            return;
        }

        if (!msg.attachment || isExporting) return;

        try {
            isExporting = true;
            let targetPath: string | null = null;

            if (isMobile || true) { 
                const { save } = await import('@tauri-apps/plugin-dialog');
                targetPath = await save({ defaultPath: msg.attachment.fileName });
                
                if (targetPath) {
                    await invoke('vault_export_media', { id: msg.id, targetPath });
                    exportedPath = targetPath;
                    if (chatId) markAsDownloaded(chatId, msg.id, targetPath);
                    addToast("Saved successfully", 'success');
                }
            }
        } catch (e: any) {
            const msg_str = typeof e === 'string' ? e : (e?.message || JSON.stringify(e));
            addToast("Failed: " + msg_str, 'error');
        } finally {
            isExporting = false;
        }
    }

    let element = $state<HTMLElement | null>(null);

    // Interaction handler to trigger full decryption (Lightbox)
    function triggerFullView(e: MouseEvent) {
        e.stopPropagation();
        if (isImage || isVideo) {
            toggleLightbox(e);
        }
    }

    function toggleLightbox(e: MouseEvent | TouchEvent) {
        e.stopPropagation();
        lightbox.set({
            id: msg.id,
            src: mediaUrl,
            alt: msg.attachment.fileName,
            fileName: msg.attachment.fileName,
            fileType: msg.attachment.fileType,
            size: msg.attachment.size || 0,
            type: isVideo ? 'video' : 'image',
            content: msg.content,
            timestamp: msg.timestamp,
            senderNickname: $userStore.nicknames[msg.senderHash] || msg.senderHash?.slice(0, 8)
        });
    }
</script>

<div bind:this={element} class="w-full min-h-[40px]">
{#if msg.type === 'voice_note' || msg.attachment?.fileName === 'voice_note.wav'}
    {#if msg.status === 'sending'}
        <div class="flex flex-col space-y-1 w-full max-w-[200px]">
            <div class="flex items-center space-x-2 py-2 px-4 bg-entropy-primary/10 rounded-[0.9rem] animate-pulse">
                <LucideLoader size={16} class="animate-spin text-entropy-primary" />
                <span class="text-[10px] font-bold text-entropy-primary uppercase tracking-wider">
                    {activeTransfer ? `Sending... ${progress}%` : "Processing..."}
                </span>
            </div>
            {#if activeTransfer}
                <div class="h-1 w-full bg-entropy-surface-light rounded-full overflow-hidden">
                    <div class="h-full bg-entropy-primary transition-all duration-300" style="width: {progress}%"></div>
                </div>
            {/if}
        </div>
    {:else}
        <VoiceNotePlayer src={mediaUrl} id={msg.id} isMine={msg.isMine} initialDuration={msg.attachment.duration || 0} />
    {/if}
{:else if msg.type === 'file'}
    <div class="flex flex-col space-y-2 max-w-full">
        {#if (isImage || isVideo)}
            <!-- Media Container with Context Menu Support -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div 
                class="relative group/media overflow-hidden rounded-[0.9rem] border border-white/10 shadow-xl bg-entropy-surface flex flex-col w-full sm:w-[320px] transition-all hover:shadow-2xl active:scale-[0.98]"
                oncontextmenu={openContextMenu}
                onclick={triggerFullView}
                onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && triggerFullView(e as any)}
                role="button"
                tabindex="0"
                aria-label="View media full screen"
            >
                <div class="relative w-full aspect-video bg-black flex items-center justify-center overflow-hidden cursor-pointer">
                    {#if isImage}
                        <img 
                            src={msg.attachment.thumbnail || mediaUrl} 
                            alt={msg.attachment.fileName} 
                            class="w-full h-full object-cover transition-all duration-700 hover:scale-105 {msg.attachment.thumbnail && !mediaUrl ? 'blur-[4px] scale-110 opacity-100' : 'opacity-100'}"
                        />
                    {:else if isVideo}
                        <img 
                            src={msg.attachment.thumbnail} 
                            class="w-full h-full object-cover opacity-100 blur-[4px] scale-110 group-hover/media:scale-100 group-hover/media:blur-0 transition-all duration-700" 
                            alt="Video preview"
                        />
                        <div class="absolute inset-0 flex items-center justify-center bg-black/10 group-hover/media:bg-black/30 transition-colors">
                            <div class="w-14 h-14 rounded-full bg-white/10 backdrop-blur-xl flex items-center justify-center text-white border border-white/20 group-hover/media:scale-110 transition-transform shadow-2xl">
                                <LucidePlay size={28} fill="currentColor" class="ml-1" />
                            </div>
                        </div>
                    {/if}

                    {#if loading && !msg.attachment.thumbnail}
                        <div class="absolute inset-0 flex items-center justify-center bg-black backdrop-blur-sm">
                            <LucideLoader size={24} class="animate-spin text-entropy-primary" />
                        </div>
                    {/if}
                </div>
                
                <!-- Info Bar -->
                <div class="p-3 bg-entropy-surface flex items-center justify-between border-t border-white/5">
                    <div class="flex-1 min-w-0">
                        <div class="text-[12px] font-bold text-entropy-text-primary truncate mb-0.5 leading-none">{msg.attachment.fileName}</div>
                        <div class="text-[10px] font-bold text-entropy-text-dim uppercase tracking-wider opacity-100 mt-1">
                            {#if activeTransfer}
                                <span class="text-entropy-primary animate-pulse">{progress}%</span>
                            {:else}
                                {fileSize}
                            {/if}
                        </div>
                    </div>
                </div>
            </div>
        {:else if msg.status === 'failed'}
            <div class="flex flex-col items-center justify-center py-6 px-10 bg-red-500/10 rounded-[0.9rem] border border-red-500/20 w-full sm:w-[320px]">
                <LucideAlertCircle size={24} class="text-red-500 mb-2" />
                <span class="text-[10px] font-bold text-red-500 uppercase tracking-widest text-center">
                    Decryption Failed
                </span>
                {#if msg.error}
                    <span class="text-[9px] text-red-400/80 mt-1 text-center line-clamp-2 px-4 mb-3">
                        {msg.error}
                    </span>
                {/if}
                
                <button 
                    onclick={async () => {
                        loading = true;
                        try {
                            const { invoke } = await import('@tauri-apps/api/core');
                            await invoke('vault_retry_bridge', { msgId: msg.id });
                        } catch (e: any) {
                            console.error(e);
                        } finally {
                            loading = false;
                        }
                    }}
                    class="mt-1 px-4 py-1.5 bg-red-500 text-white rounded-full text-[10px] font-bold uppercase tracking-wider hover:bg-red-600 transition-colors shadow-lg active:scale-95 disabled:opacity-50"
                    disabled={loading}
                >
                    {#if loading}
                        <LucideLoader size={12} class="animate-spin" />
                    {:else}
                        Retry Decryption
                    {/if}
                </button>
            </div>
        {:else if loading}
            <div class="flex flex-col items-center justify-center py-10 px-16 bg-entropy-surface-light rounded-[0.9rem] border border-white/10 animate-pulse w-full sm:w-[320px]">
                <LucideLoader size={24} class="animate-spin text-entropy-primary mb-2" />
                <span class="text-[10px] font-bold text-entropy-text-dim uppercase tracking-widest">Decrypting...</span>
            </div>
        {:else}
            <!-- Generic File UI -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div 
                role="button"
                tabindex="0"
                class="flex items-center space-x-3 bg-entropy-surface/60 backdrop-blur-md p-3 rounded-[0.9rem] shadow-sm border border-white/5 group/file hover:bg-entropy-surface/80 transition-all max-w-full cursor-pointer active:scale-[0.98]"
                oncontextmenu={openContextMenu}
                onclick={openSavedFile}
                onkeydown={(e) => e.key === 'Enter' && openSavedFile()}
            >
                <div class="w-10 h-10 rounded-xl bg-entropy-primary/10 flex items-center justify-center text-entropy-primary shrink-0 group-hover/file:scale-110 transition-transform">
                    {#if isVideo}
                        <LucidePlay size={20} />
                    {:else}
                        <LucidePaperclip size={20} />
                    {/if}
                </div>
                <div class="flex-1 min-w-0 overflow-hidden">
                    <div class="block text-left w-full group/name min-w-0 overflow-hidden">
                        <div class="text-[13px] font-bold truncate text-entropy-text-primary tracking-tight leading-none mb-1 {exportedPath || isMine ? 'group-hover/name:text-entropy-primary group-hover/name:underline' : ''}">
                            {msg.attachment.fileName}
                        </div>
                    </div>
                    <div class="text-[10px] font-bold text-entropy-text-dim uppercase tracking-widest opacity-80">
                        {#if activeTransfer}
                            <span class="text-entropy-primary animate-pulse">{activeTransfer.direction === 'upload' ? 'Sending' : 'Receiving'} {progress}%</span>
                        {:else}
                            {fileSize}
                        {/if}
                    </div>
                </div>

                <div class="flex items-center space-x-1.5">
                    <button 
                        onclick={doExport}
                        class="w-9 h-9 rounded-xl {isExporting ? 'bg-entropy-primary animate-pulse' : (exportedPath || msg.isMine ? 'bg-white text-entropy-primary' : 'bg-entropy-primary text-white')} flex items-center justify-center hover:bg-opacity-90 transition-all active:scale-95 shadow-lg disabled:opacity-50"
                        title={exportedPath || msg.isMine ? "Open File" : "Export to Local PC"}
                        disabled={isExporting}
                    >
                        {#if isExporting}
                            <LucideLoader size={18} class="animate-spin" />
                        {:else if msg.isMine || exportedPath}
                            <LucidePaperclip size={18} />
                        {:else}
                            <LucideDownload size={18} />
                        {/if}
                    </button>
                </div>
            </div>
        {/if}
    </div>
{/if}
</div>
