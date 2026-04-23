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

    let { msg, chatId, isMobile = false } = $props<{ msg: any, chatId: string, isMobile?: boolean }>();

    let blobUrl = $state<string | null>(null);
    let loading = $state(false);
    let error = $state(false);
    let isExporting = $state(false);
    let isMine = $derived(msg.senderHash === $userStore.identityHash || msg.isMine);
    let exportedPath = $state<string | null>(msg.attachment?.exportedPath || (isMine && !msg.attachment?.vaultPath ? msg.attachment?.originalPath : null));
    $effect(() => {
        if (msg.attachment?.exportedPath) exportedPath = msg.attachment.exportedPath;
        else if (isMine && !msg.attachment?.vaultPath && msg.attachment?.originalPath) exportedPath = msg.attachment.originalPath;
    });
    let wasCreatedInternally = $state(false);

    let activeTransfer = $derived.by(() => {
        const t_id = msg.attachment?.transferId;
        if (!t_id) return null;
        return $transfers[t_id] || null;
    });

    let progress = $derived(activeTransfer ? Math.round((activeTransfer.current / activeTransfer.total) * 100) : 0);

    async function loadAttachment() {
        if (!msg.attachment || blobUrl || loading) return;
        
        // Step 1: High Performance Native Path (Only for unencrypted local files)
        const path = msg.attachment.originalPath;
        if (path && !msg.attachment.vaultPath) {
            blobUrl = convertFileSrc(path);
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
        try {
            const data = await getAttachment(msg.id);
            if (data) {
                blobUrl = URL.createObjectURL(new Blob([data as any], {type: msg.attachment.fileType}));
                wasCreatedInternally = true;
            } else {
                error = true;
            }
        } catch (e) {
            error = true;
        } finally {
            loading = false;
        }
    }

    async function openSavedFile() {
        if (!exportedPath) return;
        try {
            await invoke('open_file', { path: exportedPath });
        } catch (e) {
            // Failed to open file
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
            label: exportedPath || isMine ? 'Open File' : 'Save to Device',
            onSave: doExport
        });
    }

    async function doExport() {

        // Already exported or sender has original — open directly
        if (exportedPath) {
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

            if (isMine || isMobile) {
                // Auto-save to Downloads — no dialog for the sender
                const { downloadDir, join } = await import('@tauri-apps/api/path');
                const downDir = await downloadDir();
                // Ensure unique name by prefixing with short ID part if it's not our own sent file
                const fileName = isMine ? msg.attachment.fileName : `${msg.id.substring(0, 5)}_${msg.attachment.fileName}`;
                targetPath = await join(downDir, fileName);
            } else {
                const { save } = await import('@tauri-apps/plugin-dialog');
                targetPath = await save({ defaultPath: msg.attachment.fileName });
            }

            if (targetPath) {
                await invoke('db_export_media', { srcPath, targetPath });
                exportedPath = targetPath;
                if (chatId) markAsDownloaded(chatId, msg.id, targetPath);
                addToast(isMine ? "Opening file..." : "Saved to: " + targetPath.split(/[/\\]/).pop(), 'success');
                openSavedFile();
            }

        } catch (e: any) {
            const msg_str = typeof e === 'string' ? e : (e?.message || JSON.stringify(e));
            addToast("Failed: " + msg_str, 'error');
        } finally {
            isExporting = false;
        }
    }

    let isImage = $derived(msg.attachment?.fileType?.startsWith('image/') || /\.(jpg|jpeg|png|gif|webp)$/i.test(msg.attachment?.fileName || ''));
    let isVideo = $derived(msg.attachment?.fileType?.startsWith('video/') || /\.(mp4|webm|mov|ogg)$/i.test(msg.attachment?.fileName || ''));

    let observer: IntersectionObserver;
    let element = $state<HTMLElement | null>(null);
    let isVisible = $state(false);

    $effect(() => {
        if (!element || (!isImage && !isVideo && msg.type !== 'voice_note')) return;

        observer = new IntersectionObserver((entries) => {
            const entry = entries[0];
            isVisible = entry.isIntersecting;
            
            if (isVisible) {
                loadAttachment();
            } else if (blobUrl && wasCreatedInternally) {
                // Garbage collect decrypted bytes when scrolled away to save RAM
                URL.revokeObjectURL(blobUrl);
                blobUrl = null;
            }
        }, {
            rootMargin: '200px' // Start loading slightly before it hits the screen for smoothness
        });

        observer.observe(element);

        return () => {
            observer.disconnect();
            if (blobUrl && wasCreatedInternally) {
                URL.revokeObjectURL(blobUrl);
            }
        };
    });

    function toggleLightbox(e: MouseEvent | TouchEvent) {
        e.stopPropagation();
        if (blobUrl) {
            lightbox.set({
                src: blobUrl,
                alt: msg.attachment.fileName,
                fileName: msg.attachment.fileName,
                size: msg.attachment.size || 0
            });
        }
    }
</script>

<div bind:this={element} class="w-full min-h-[40px]">
{#if msg.type === 'voice_note' || msg.attachment?.fileName === 'voice_note.wav'}
    {#if blobUrl}
        <VoiceNotePlayer src={blobUrl} id={msg.id} isMine={msg.isMine} initialDuration={msg.attachment.duration || 0} />
    {:else if loading}
        <div class="flex items-center space-x-2 py-2 px-4 bg-entropy-surface-light rounded-2xl border border-white/5">
            <LucideLoader size={16} class="animate-spin text-entropy-primary" />
            <span class="text-[10px] font-bold text-entropy-text-primary uppercase tracking-widest">Loading Audio...</span>
        </div>
    {:else if msg.status === 'sending'}
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
        <div class="flex items-center space-x-2 py-2 px-4 bg-red-500/10 rounded-2xl">
            <LucideMic size={16} class="text-red-500" />
            <span class="text-[10px] font-bold text-red-500 uppercase tracking-widest">Unavailable</span>
        </div>
    {/if}
{:else if msg.type === 'file'}
    <div class="flex flex-col space-y-2 max-w-full">
        {#if (isImage || isVideo) && blobUrl}
            <!-- Media Container with Context Menu Support -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div 
                class="relative group/media overflow-hidden rounded-2xl border border-white/10 shadow-lg bg-entropy-surface-light/30 flex flex-col w-full max-w-[280px] sm:max-w-[400px] min-h-[150px]"
                oncontextmenu={openContextMenu}
            >
                {#if isImage}
                    <button class="block w-full text-left" onclick={toggleLightbox}>
                        <img src={blobUrl} alt={msg.attachment.fileName} class="w-full h-auto max-h-[350px] object-cover transition-transform duration-500 group-hover/media:scale-105" />
                    </button>
                {:else if isVideo}
                    <div class="relative w-full aspect-video bg-black flex items-center justify-center">
                        <video 
                            src={blobUrl} 
                            controls 
                            class="w-full h-full max-h-[350px] object-contain"
                            poster={null}
                        >
                            <track kind="captions" />
                            Your browser does not support the video tag.
                        </video>
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
                                {(msg.attachment.size || 0) / 1024 > 1024 
                                    ? ((msg.attachment.size || 0)/1024/1024).toFixed(1) + ' MB' 
                                    : ((msg.attachment.size || 0)/1024).toFixed(1) + ' KB'}
                            {/if}
                        </div>
                    </div>
                </div>
            </div>
        {:else}
            <div class="flex items-center space-x-3 bg-entropy-surface/60 backdrop-blur-md p-3 rounded-2xl shadow-sm border border-white/5 group/file hover:bg-entropy-surface/80 transition-all max-w-full">
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
                            {(msg.attachment.size || 0) / 1024 > 1024 
                                ? ((msg.attachment.size || 0)/1024/1024).toFixed(1) + ' MB' 
                                : ((msg.attachment.size || 0)/1024).toFixed(1) + ' KB'}
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
        
        {#if activeTransfer && !isImage && !isVideo}
            <div class="mt-1 h-1 w-full bg-entropy-surface-light rounded-full overflow-hidden">
                <div class="h-full bg-entropy-primary transition-all duration-500 shadow-[0_0_8px_rgba(var(--entropy-primary-rgb),0.5)]" style="width: {progress}%"></div>
            </div>
        {/if}
    </div>
{/if}
</div>
