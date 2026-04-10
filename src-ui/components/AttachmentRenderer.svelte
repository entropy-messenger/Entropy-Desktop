<script lang="ts">
    import { 
        LucideMic, LucidePaperclip, LucideDownload, LucideLoader, 
        LucideCheck
    } from 'lucide-svelte';
    import { convertFileSrc, invoke } from '@tauri-apps/api/core';
    import { getAttachment, markAsDownloaded } from '../lib/actions/chat';
    import { fromBase64 } from '../lib/crypto';
    import { addToast } from '../lib/stores/ui';
    import VoiceNotePlayer from './VoiceNotePlayer.svelte';

    let { msg, chatId } = $props<{ msg: any, chatId: string }>();

    let blobUrl = $state<string | null>(null);
    let loading = $state(false);
    let error = $state(false);
    let isExporting = $state(false);
    let exportedPath = $state<string | null>(msg.attachment?.exportedPath || null);
    let wasCreatedInternally = $state(false);

    async function loadAttachment() {
        if (!msg.attachment || blobUrl || loading) return;
        
        const path = msg.attachment.vaultPath || msg.attachment.originalPath;

        // Step 1: High Performance Native Path (Zero-copy)
        if (path) {
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

    async function manualExport() {
        if (exportedPath) {
            openSavedFile();
            return;
        }
        
        if (!msg.attachment || isExporting) return;
        const srcPath = msg.attachment.vaultPath || msg.attachment.originalPath;
        
        if (!srcPath) {
            addToast("Media still decrypting. Please wait...", 'warning');
            return;
        }

        try {
            const { save } = await import('@tauri-apps/plugin-dialog');
            const targetPath = await save({
                defaultPath: msg.attachment.fileName,
            });

            if (targetPath) {
                isExporting = true;
                await invoke('db_export_media', { srcPath, targetPath });
                exportedPath = targetPath;
                addToast("Saved to: " + targetPath.split(/[/\\]/).pop(), 'success');
                if (chatId) markAsDownloaded(chatId, msg.id, targetPath);
            }

        } catch (e) {
            // Export failed
            addToast("Failed to save file", 'error');
        } finally {
            isExporting = false;
        }
    }

    $effect(() => {
        // Auto-load only voice notes for inline player functionality
        if (msg.type === 'voice_note' || msg.attachment?.fileName === 'voice_note.wav') {
            loadAttachment();
        }
        
        return () => {
            if (blobUrl && wasCreatedInternally) {
                URL.revokeObjectURL(blobUrl);
            }
        };
    });
</script>

{#if msg.type === 'voice_note' || msg.attachment?.fileName === 'voice_note.wav'}
    {#if blobUrl}
        <VoiceNotePlayer src={blobUrl} id={msg.id} isMine={msg.isMine} initialDuration={msg.attachment.duration || 0} />
    {:else if loading}
        <div class="flex items-center space-x-2 py-2 px-4 bg-entropy-surface-light rounded-2xl border border-white/5">
            <LucideLoader size={16} class="animate-spin text-entropy-primary" />
            <span class="text-[10px] font-bold text-entropy-text-primary uppercase tracking-widest">Loading Audio...</span>
        </div>
    {:else if msg.status === 'sending'}
        <div class="flex items-center space-x-2 py-2 px-4 bg-entropy-primary/10 rounded-2xl animate-pulse">
            <LucideLoader size={16} class="animate-spin text-entropy-primary" />
            <span class="text-[10px] font-bold text-entropy-primary uppercase tracking-wider">Processing...</span>
        </div>
    {:else}
        <div class="flex items-center space-x-2 py-2 px-4 bg-red-500/10 rounded-2xl">
            <LucideMic size={16} class="text-red-500" />
            <span class="text-[10px] font-bold text-red-500 uppercase tracking-widest">Unavailable</span>
        </div>
    {/if}
{:else if msg.type === 'file'}
    <div class="flex flex-col space-y-2">

        <div class="flex items-center space-x-3 bg-entropy-surface/60 backdrop-blur-md p-3 rounded-2xl shadow-sm border border-white/5 group/file hover:bg-entropy-surface/80 transition-all">
            <div class="w-10 h-10 rounded-xl bg-entropy-primary/10 flex items-center justify-center text-entropy-primary shrink-0 group-hover/file:scale-110 transition-transform">
                <LucidePaperclip size={20} />
            </div>
            <div class="flex-1 min-w-0">
                <button 
                    onclick={openSavedFile}
                    class="block text-left w-full group/name"
                    disabled={!exportedPath}
                >
                    <div class="text-[13px] font-bold truncate text-entropy-text-primary tracking-tight leading-none mb-1 {exportedPath ? 'group-hover/name:text-entropy-primary group-hover/name:underline' : ''}">
                        {msg.attachment.fileName}
                    </div>
                </button>
                <div class="text-[10px] font-bold text-entropy-text-dim uppercase tracking-widest opacity-80">
                    {(msg.attachment.size || 0) / 1024 > 1024 
                        ? ((msg.attachment.size || 0)/1024/1024).toFixed(1) + ' MB' 
                        : ((msg.attachment.size || 0)/1024).toFixed(1) + ' KB'}
                </div>
            </div>

            <div class="flex items-center space-x-1.5">
                <button 
                    onclick={manualExport}
                    class="w-9 h-9 rounded-xl {isExporting ? 'bg-entropy-primary animate-pulse' : (exportedPath ? 'bg-white text-entropy-primary' : 'bg-entropy-primary text-white')} flex items-center justify-center hover:bg-opacity-90 transition-all active:scale-95 shadow-lg disabled:opacity-50"
                    title={exportedPath ? "Open File" : "Export to Local PC"}
                    disabled={isExporting}
                >
                    {#if isExporting}
                        <LucideLoader size={18} class="animate-spin" />
                    {:else if exportedPath}
                        <LucideCheck size={18} />
                    {:else}
                        <LucideDownload size={18} />
                    {/if}
                </button>
            </div>
        </div>
    </div>
{/if}
