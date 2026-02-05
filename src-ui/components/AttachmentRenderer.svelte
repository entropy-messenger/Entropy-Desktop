
<script lang="ts">
    import { attachmentStore } from '../lib/attachment_store';
    import { LucideMic, LucidePaperclip, LucideDownload, LucideLoader, LucidePlay, LucidePause } from 'lucide-svelte';
    import { userStore, setActiveAudio, clearActiveAudio } from '../lib/store';

    let { msg } = $props();

    let blobUrl = $state<string | null>(null);
    let loading = $state(false);
    let error = $state(false);

    // Audio states
    let audioPlayer = $state<HTMLAudioElement | null>(null);
    let isPlaying = $state(false);
    let currentTime = $state(0);
    let duration = $state(0);
    let playbackRate = $state(1);

    const togglePlaybackRate = () => {
        const rates = [1, 1.5, 2, 0.5];
        const currentIndex = rates.indexOf(playbackRate);
        playbackRate = rates[(currentIndex + 1) % rates.length];
        if (audioPlayer) audioPlayer.playbackRate = playbackRate;
    };

    let displayedDuration = $derived(
        (msg.attachment?.duration && msg.attachment.duration > 0)
        ? msg.attachment.duration
        : ((duration > 0 && isFinite(duration)) ? duration : 0)
    );

    import { signalManager } from '../lib/signal_manager';
    import { toHex } from '../lib/utils';

    function ensureUint8(data: any): Uint8Array {
        if (!data) return new Uint8Array(0);
        if (data instanceof Uint8Array) return data;
        if (data instanceof ArrayBuffer) return new Uint8Array(data);
        if (Array.isArray(data)) return new Uint8Array(data);
        
        // Fast path for Tauri's numeric object format
        // Avoid Object.values() on large data if possible
        if (typeof data === 'object' && 'length' in data && typeof data.length === 'number') {
            const arr = new Uint8Array(data.length);
            for (let i = 0; i < data.length; i++) arr[i] = data[i];
            return arr;
        }
        
        return new Uint8Array(0);
    }

    async function loadAttachment() {
        if (blobUrl) URL.revokeObjectURL(blobUrl);
        blobUrl = null;
        error = false;

        // Skip if chunked but not yet finished
        if (msg.attachment.isChunked && !msg.attachment.isComplete) return;

        loading = true;
        try {
            const data = await attachmentStore.get(msg.id);
            if (data) {
                const normalized = ensureUint8(data);
                if (msg.attachment.isV2 && msg.attachment.bundle) {
                    const hexData = toHex(normalized);
                    const decrypted = await signalManager.decryptMedia(hexData, msg.attachment.bundle);
                    blobUrl = URL.createObjectURL(new Blob([decrypted as any], {type: msg.attachment.fileType}));
                } else {
                    blobUrl = URL.createObjectURL(new Blob([normalized], {type: msg.attachment.fileType}));
                }
            } else {
                if (!msg.attachment.isChunked || msg.attachment.isComplete) {
                    error = true;
                }
            }
        } catch (e) {
            console.error("Attachment load error:", e);
            error = true;
        } finally {
            loading = false;
        }
    }

    import { onDestroy } from 'svelte';
    onDestroy(() => {
        if (blobUrl) {
            URL.revokeObjectURL(blobUrl);
            blobUrl = null;
        }
    });

    let lastProcessedMsgId = "";
    $effect(() => {
        const isComplete = msg.attachment?.isComplete;
        const isChunked = msg.attachment?.isChunked;
        
        if (msg.id !== lastProcessedMsgId || (isChunked && isComplete && !blobUrl && !loading)) {
             lastProcessedMsgId = msg.id;
             loadAttachment();
        }
    });

    $effect(() => {
        const activeId = $userStore.activeAudioId;
        if (activeId && activeId !== msg.id && isPlaying && audioPlayer) {
            audioPlayer.pause(); // DOM pause fires onpause -> sets isPlaying = false
        }
    });

    const togglePlayback = () => {
        if (!audioPlayer) return;
        if (isPlaying) {
            audioPlayer.pause();
        } else {
            setActiveAudio(msg.id, msg.groupId || msg.senderHash);
            audioPlayer.play();
        }
    };


    const formatTime = (time: number) => {
        if (!time || isNaN(time) || !isFinite(time)) return "0:00";
        let totalSeconds = Math.floor(time);
        // Only divide by 1000 if it's clearly a millisecond timestamp/duration (e.g. > 1000000ms)
        if (totalSeconds > 360000) { 
            totalSeconds = Math.floor(time / 1000);
        }
        const mins = Math.floor(totalSeconds / 60);
        const secs = Math.floor(totalSeconds % 60);
        return `${mins}:${secs.toString().padStart(2, '0')}`;
    };

</script>

{#if msg.type === 'voice_note'}
    <div class="flex items-center space-x-3 py-1 min-w-[220px] max-w-full">
        {#if blobUrl}
            <button 
                onclick={togglePlayback}
                class="w-10 h-10 flex-shrink-0 bg-indigo-500 text-white rounded-full flex items-center justify-center hover:bg-indigo-600 transition active:scale-90 shadow-sm"
            >
                {#if isPlaying}
                    <LucidePause size={18} fill="currentColor" />
                {:else}
                    <LucidePlay size={18} fill="currentColor" class="ml-1" />
                {/if}
            </button>

            <div class="flex-1 flex flex-col space-y-1 min-w-0">
                <div class="flex items-center space-x-2">
                    <div class="flex-1 h-1.5 bg-black/5 rounded-full relative overflow-hidden group">
                        <div 
                            class="absolute top-0 left-0 h-full bg-indigo-500/60 transition-all duration-100"
                            style="width: {Math.min(100, (currentTime / (displayedDuration || 1)) * 100)}%"
                        ></div>
                        <input 
                            type="range" 
                            min="0" 
                            max={displayedDuration} 
                            step="0.01" 
                            bind:value={currentTime}
                            oninput={() => { if (audioPlayer) audioPlayer.currentTime = currentTime; }}
                            class="absolute inset-0 opacity-0 cursor-pointer"
                        />
                    </div>
                </div>
                <div class="flex justify-between items-center px-0.5 mt-[-2px]">
                    <div class="flex items-center space-x-2">
                        <span class="text-[9px] font-black text-indigo-500 tabular-nums">
                            {formatTime(currentTime)}
                        </span>
                        <button 
                            onclick={togglePlaybackRate}
                            class="text-[8px] font-black px-1.5 h-4 bg-black/5 hover:bg-black/10 text-gray-500 rounded transition tabular-nums"
                        >
                            {playbackRate}x
                        </button>
                    </div>
                    <span class="text-[9px] font-black text-gray-400 tabular-nums">
                        {formatTime(displayedDuration)}
                    </span>
                </div>
            </div>

            <audio 
                src={blobUrl} 
                bind:this={audioPlayer}
                bind:currentTime
                bind:duration
                bind:playbackRate
                onplay={() => isPlaying = true}
                onpause={() => {
                    isPlaying = false;
                    clearActiveAudio(msg.id);
                }}
                onended={() => { 
                    isPlaying = false; 
                    currentTime = 0; 
                    clearActiveAudio(msg.id);
                }}
                class="hidden"
            ></audio>
        {:else if msg.attachment?.isChunked && !msg.attachment?.isComplete}
             <div class="w-10 h-10 bg-indigo-100 rounded-full flex items-center justify-center text-indigo-500">
                <LucideLoader size={18} class="animate-spin" />
            </div>
            <div class="flex-1">
                <div class="text-[10px] font-black uppercase text-indigo-600 mb-1">Downloading VN...</div>
                <div class="h-1.5 w-full bg-indigo-200/50 rounded-full overflow-hidden">
                    <div class="h-full bg-indigo-500 transition-all duration-300" style="width: {msg.attachment.progress || 0}%"></div>
                </div>
            </div>
        {:else if loading}
            <div class="w-10 h-10 bg-gray-100 rounded-full flex items-center justify-center">
                <LucideLoader size={18} class="animate-spin text-gray-400" />
            </div>
            <div class="flex-1 space-y-2">
                <div class="h-1.5 bg-gray-100 rounded-full w-full"></div>
                <div class="h-2 w-8 bg-gray-50 rounded"></div>
            </div>
        {:else if error}
            <div class="w-10 h-10 bg-red-50 rounded-full flex items-center justify-center text-red-400">
                <LucideMic size={18} />
            </div>
            <button onclick={loadAttachment} class="text-[10px] font-black uppercase text-red-500 hover:underline">Retry</button>
        {:else}
             <div class="w-10 h-10 bg-gray-100 rounded-full flex items-center justify-center text-gray-400">
                <LucideMic size={18} />
            </div>
            <span class="text-[10px] font-black uppercase text-gray-400">Loading Audio...</span>
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
                    {#if msg.attachment.isChunked && !msg.attachment.isComplete}
                         Downloading... {Math.round(msg.attachment.progress || 0)}%
                    {:else}
                        {(msg.attachment.size || 0) / 1024 > 1024 
                            ? ((msg.attachment.size || 0)/1024/1024).toFixed(1) + ' MB' 
                            : ((msg.attachment.size || 0)/1024).toFixed(1) + ' KB'}
                    {/if}
                </div>
                {#if msg.attachment.isChunked && !msg.attachment.isComplete}
                    <div class="w-full h-1 bg-black/10 rounded-full mt-1 overflow-hidden">
                        <div class="h-full bg-blue-500 transition-all duration-300" style="width: {msg.attachment.progress || 0}%"></div>
                    </div>
                {/if}
            </div>
            {#if blobUrl}
                <a 
                    href={blobUrl} 
                    download={msg.attachment.fileName}
                    class="p-2 hover:bg-black/5 rounded-full transition text-blue-600"
                    title="Download File"
                >
                    <LucideDownload size={16} />
                </a>
            {:else if loading || (msg.attachment.isChunked && !msg.attachment.isComplete)}
                <LucideLoader size={16} class="animate-spin text-gray-400" />
            {:else if error}
                <button onclick={loadAttachment} class="text-[10px] font-bold text-blue-500 uppercase hover:underline">Retry</button>
            {/if}
        </div>
    </div>
{/if}

<style>
    input[type=range] {
        -webkit-appearance: none;
        background: transparent;
        width: 100%;
    }
    input[type=range]:focus {
        outline: none;
    }
    input[type=range]::-webkit-slider-thumb {
        -webkit-appearance: none;
        height: 12px;
        width: 12px;
        border-radius: 50%;
        background: currentColor;
        cursor: pointer;
        margin-top: -5px;
        box-shadow: 0 0 2px rgba(0,0,0,0.2);
        opacity: 0;
        transition: opacity 0.2s;
    }
    .group:hover input[type=range]::-webkit-slider-thumb {
        opacity: 1;
    }
</style>
