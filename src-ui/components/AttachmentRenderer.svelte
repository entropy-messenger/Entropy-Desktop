<script lang="ts">
    import { 
        LucideMic, LucidePaperclip, LucideDownload, LucideLoader, 
        LucideCheck, LucidePlay, LucideClock, LucideCheckCheck, LucideX, LucideStar
    } from 'lucide-svelte';
    import { userStore } from '../lib/stores/user';
    import { convertFileSrc, invoke } from '@tauri-apps/api/core';
    import { getAttachment, markAsDownloaded } from '../lib/actions/chat';
    import { fromBase64 } from '../lib/crypto';
    import { addToast, lightbox, contextMenu } from '../lib/stores/ui';
    import { transfers } from '../lib/stores/transfers';
    import VoiceNotePlayer from './VoiceNotePlayer.svelte';
    import VideoPlayer from './VideoPlayer.svelte';

    let { msg, chatId, isMobile = false } = $props<{ msg: any, chatId: string, isMobile?: boolean }>();

    let blobUrl = $state<string | null>(null);
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

    // Cleanup Blob URLs on unmount to prevent memory leaks
    $effect(() => {
        return () => {
            if (blobUrl && wasCreatedInternally) {
                URL.revokeObjectURL(blobUrl);
            }
        };
    });

    // Automatically trigger load for images/videos once available.
    // Small images use In-Memory decryption, while videos/large files use the Zero-RAM Proxy.
    $effect(() => {
        if ((isImage || isVideo) && msg.attachment?.vaultPath && !blobUrl && !loading && !failed) {
            loadAttachment();
        }
    });

    let isImage = $derived(msg.attachment?.fileType?.startsWith('image/') || /\.(jpg|jpeg|png|gif|webp)$/i.test(msg.attachment?.fileName || ''));
    let isVideo = $derived(msg.attachment?.fileType?.startsWith('video/') || /\.(mp4|webm|mov|ogg)$/i.test(msg.attachment?.fileName || ''));
    
    let fileSize = $derived.by(() => {
        const bytes = msg.attachment?.size || 0;
        if (bytes / 1024 > 1024) return (bytes / 1024 / 1024).toFixed(1) + ' MB';
        return (bytes / 1024).toFixed(1) + ' KB';
    });

    let wasCreatedInternally = $state(false);

    let activeTransfer = $derived.by(() => {
        const t_id = msg.attachment?.transferId;
        if (!t_id) return null;
        return $transfers[t_id] || null;
    });

    let progress = $derived(activeTransfer ? Math.round((activeTransfer.current / activeTransfer.total) * 100) : 0);

    async function loadAttachment() {
        if (!msg.attachment || blobUrl || loading || failed) return;
        
        // Step 1: High Performance Native Path (Only for unencrypted local files)
        const path = msg.attachment.originalPath;
        if (path && !msg.attachment.vaultPath) {
            blobUrl = convertFileSrc(path);
            wasCreatedInternally = false;
            loading = false;
            return;
        }

        // Step 1.5: Local Proxy Path (Zero-RAM for large media)
        if (msg.attachment.vaultPath && (isVideo || (isImage && (msg.attachment.size || 0) > 5 * 1024 * 1024))) {
            blobUrl = `http://localhost:51761/media/${msg.id}`;
            wasCreatedInternally = false;
            loading = false;
            return;
        }

        // Step 2: In-memory Fallback
        if (msg.attachment.data) {
            let bytes = msg.attachment.data;
            if (typeof bytes === 'string') bytes = fromBase64(bytes);
            blobUrl = URL.createObjectURL(new Blob([bytes as any], {type: msg.attachment.fileType}));
            wasCreatedInternally = true;
            return;
        }

        // Step 3: Vault Retrieval
        loading = true;
        failed = false;
        try {
            const data = await getAttachment(msg.id);
            if (data) {
                blobUrl = URL.createObjectURL(new Blob([data as any], {type: msg.attachment.fileType}));
                wasCreatedInternally = true;
            } else {
                failed = true;
            }
        } catch (e) {
            console.error("[UI] Decryption failed:", e);
            failed = true;
        } finally {
            loading = false;
        }
    }

    async function openSavedFile() {
        if (!exportedPath) return;
        try {
            await invoke('open_file', { path: exportedPath });
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
        if (exportedPath && !(isMobile && !isImage && !isVideo)) {
            openSavedFile();
            return;
        }

        if (!msg.attachment || isExporting) return;
        const srcPath = msg.attachment.vaultPath || msg.attachment.originalPath;
        if (!srcPath) {
            addToast("Attachment source not found.", 'warning');
            return;
        }

        try {
            isExporting = true;
            let targetPath: string | null = null;

            if (isMobile) {
                const { save } = await import('@tauri-apps/plugin-dialog');
                targetPath = await save({ defaultPath: msg.attachment.fileName });
                
                if (targetPath) {
                    const { writeFile } = await import('@tauri-apps/plugin-fs');
                    let dataToSave: Uint8Array;
                    
                    if (msg.attachment.data) {
                        let bytes = msg.attachment.data;
                        if (typeof bytes === 'string') bytes = fromBase64(bytes);
                        dataToSave = new Uint8Array(bytes as any);
                    } else {
                        const data = await getAttachment(msg.id);
                        if (!data) throw new Error("Could not fetch decrypted file data.");
                        dataToSave = new Uint8Array(data as any);
                    }
                    
                    await writeFile(targetPath, dataToSave);
                    exportedPath = targetPath;
                    if (chatId) markAsDownloaded(chatId, msg.id, targetPath);
                    addToast("Saved successfully", 'success');
                }
            } else {
                const { save } = await import('@tauri-apps/plugin-dialog');
                targetPath = await save({ defaultPath: msg.attachment.fileName });

                if (targetPath) {
                    await invoke('db_export_media', { srcPath, targetPath });
                    exportedPath = targetPath;
                    if (chatId) markAsDownloaded(chatId, msg.id, targetPath);
                    addToast(isMine ? "Opening file..." : "Saved to: " + targetPath.split(/[/\\]/).pop(), 'success');
                    openSavedFile();
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
            src: blobUrl,
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
            <div class="flex items-center space-x-2 py-2 px-4 bg-entropy-primary/10 rounded-2xl animate-pulse">
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
        <VoiceNotePlayer src={blobUrl} id={msg.id} isMine={msg.isMine} initialDuration={msg.attachment.duration || 0} />
    {/if}
{:else if msg.type === 'file'}
    <div class="flex flex-col space-y-2 max-w-full">
        {#if (isImage || isVideo)}
            <!-- Media Container with Context Menu Support -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div 
                class="relative group/media overflow-hidden rounded-2xl border border-white/10 shadow-lg bg-entropy-surface-light/30 flex flex-col w-full max-w-[280px] sm:max-w-[400px] min-h-[150px]"
                oncontextmenu={openContextMenu}
                onclick={triggerFullView}
            >
                {#if isImage}
                    <div class="relative w-full aspect-auto min-h-[150px] max-h-[500px] flex items-center justify-center bg-black/5 overflow-hidden">
                        <img 
                            src={msg.attachment.thumbnail || blobUrl} 
                            alt={msg.attachment.fileName} 
                            class="w-full h-full object-cover cursor-pointer hover:scale-105 transition-all duration-500 {msg.attachment.thumbnail && !blobUrl ? 'blur-[2px] scale-105' : ''}"
                        />
                    </div>
                {:else if isVideo}
                    <div class="relative w-full aspect-video bg-black flex items-center justify-center group/vid overflow-hidden cursor-pointer">
                        <img 
                            src={msg.attachment.thumbnail} 
                            class="w-full h-full object-cover opacity-80 blur-[2px] scale-105" 
                            alt="Video preview"
                        />
                        <div class="absolute inset-0 flex items-center justify-center bg-black/20 group-hover/vid:bg-black/40 transition-colors">
                            <div class="w-16 h-16 rounded-full bg-white/10 backdrop-blur-md flex items-center justify-center text-white border border-white/20 group-hover/vid:scale-110 transition-transform shadow-2xl">
                                <LucidePlay size={32} fill="currentColor" class="ml-1" />
                            </div>
                        </div>
                    </div>
                {/if}
                
                <!-- Bottom Info Bar (Minimalist) -->
                <div class="p-2.5 bg-entropy-surface/90 backdrop-blur-md flex items-center justify-between border-t border-white/5">
                    <div class="flex-1 min-w-0">
                        <div class="text-[11px] font-bold text-entropy-text-primary truncate mb-0.5 leading-none">{msg.attachment.fileName}</div>
                        <div class="text-[9px] font-bold text-entropy-text-dim uppercase tracking-wider opacity-60 mt-1">
                            {#if activeTransfer}
                                <span class="text-entropy-primary animate-pulse">{progress}%</span>
                            {:else}
                                {fileSize}
                            {/if}
                        </div>
                    </div>
                </div>
            </div>
        {:else if loading}
            <div class="flex flex-col items-center justify-center py-8 px-12 bg-entropy-surface-light rounded-2xl border border-white/10 animate-pulse">
                <LucideLoader size={24} class="animate-spin text-entropy-primary mb-2" />
                <span class="text-[10px] font-bold text-entropy-text-dim uppercase tracking-widest">Decrypting Media...</span>
            </div>
        {:else}
            <!-- Generic File UI -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div 
                class="flex items-center space-x-3 bg-entropy-surface/60 backdrop-blur-md p-3 rounded-2xl shadow-sm border border-white/5 group/file hover:bg-entropy-surface/80 transition-all max-w-full"
                oncontextmenu={openContextMenu}
            >
                <div class="w-10 h-10 rounded-xl bg-entropy-primary/10 flex items-center justify-center text-entropy-primary shrink-0 group-hover/file:scale-110 transition-transform">
                    {#if isVideo}
                        <LucidePlay size={20} />
                    {:else}
                        <LucidePaperclip size={20} />
                    {/if}
                </div>
                <div class="flex-1 min-w-0 overflow-hidden">
                    <button 
                        onclick={openSavedFile}
                        class="block text-left w-full group/name min-w-0 overflow-hidden"
                        disabled={!exportedPath && !msg.isMine}
                    >
                        <div class="text-[13px] font-bold truncate text-entropy-text-primary tracking-tight leading-none mb-1 {exportedPath || msg.isMine ? 'group-hover/name:text-entropy-primary group-hover/name:underline' : ''}">
                            {msg.attachment.fileName}
                        </div>
                    </button>
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
