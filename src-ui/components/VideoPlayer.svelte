<script lang="ts">
    import { LucidePlay, LucidePause, LucideVolume2, LucideVolumeX, LucideMaximize } from 'lucide-svelte';
    import { onMount } from 'svelte';

    let { src, expanded = false } = $props<{ src: string, expanded?: boolean }>();

    let videoElement = $state<HTMLVideoElement | null>(null);
    let isPlaying = $state(false);
    let isMuted = $state(false);
    let currentTime = $state(0);
    let duration = $state(0);
    let showControls = $state(true);
    let controlsTimeout = $state<any>(null);
    let isFullscreen = $state(false);
    
    let isPro = $derived(isFullscreen || expanded);

    onMount(() => {
        const handleFsChange = () => {
            isFullscreen = !!document.fullscreenElement;
        };
        document.addEventListener('fullscreenchange', handleFsChange);
        return () => {
            document.removeEventListener('fullscreenchange', handleFsChange);
        };
    });

    const togglePlay = (e?: Event) => {
        if (e) e.stopPropagation();
        if (!videoElement) return;
        if (isPlaying) videoElement.pause();
        else videoElement.play();
    };

    const toggleMute = (e: Event) => {
        e.stopPropagation();
        if (!videoElement) return;
        isMuted = !isMuted;
        videoElement.muted = isMuted;
    };

    const handleTimeUpdate = () => {
        if (!videoElement) return;
        currentTime = videoElement.currentTime;
    };

    const handleLoadedMetadata = () => {
        if (!videoElement) return;
        duration = videoElement.duration;
    };

    const handleSeek = (e: Event) => {
        const target = e.target as HTMLInputElement;
        const time = (parseFloat(target.value) / 100) * duration;
        if (!videoElement) return;
        videoElement.currentTime = time;
    };

    const formatTime = (time: number) => {
        const minutes = Math.floor(time / 60);
        const seconds = Math.floor(time % 60);
        return `${minutes}:${seconds.toString().padStart(2, '0')}`;
    };

    const resetControlsTimeout = () => {
        showControls = true;
        if (controlsTimeout) clearTimeout(controlsTimeout);
        controlsTimeout = setTimeout(() => {
            if (isPlaying) showControls = false;
        }, 2500);
    };

    const toggleFullscreen = (e: Event) => {
        e.stopPropagation();
        if (!videoElement) return;
        if (!document.fullscreenElement) {
            videoElement.parentElement?.requestFullscreen();
        } else {
            document.exitFullscreen();
        }
    };

    const handleVideoClick = (e: MouseEvent) => {
        e.stopPropagation();
        if (e.detail === 2) {
            toggleFullscreen(e);
        } else {
            togglePlay();
        }
    };
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div 
    class="relative w-full aspect-video bg-black rounded-xl overflow-hidden group/player select-none"
    onmousemove={resetControlsTimeout}
    onmouseleave={() => isPlaying && (showControls = false)}
    ontouchstart={resetControlsTimeout}
>
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_media_has_caption -->
    <video
        bind:this={videoElement}
        {src}
        class="w-full h-full object-contain cursor-pointer"
        onclick={handleVideoClick}
        onplay={() => isPlaying = true}
        onpause={() => isPlaying = false}
        ontimeupdate={handleTimeUpdate}
        onloadedmetadata={handleLoadedMetadata}
        playsinline
        autoplay={expanded}
    ></video>

    <!-- Overlay Controls -->
    <div 
        class="absolute inset-0 transition-opacity duration-300 flex flex-col {isPro ? 'justify-end p-3 bg-gradient-to-t from-black/60 via-transparent to-transparent' : 'justify-center items-center'} {showControls || !isPlaying ? 'opacity-100' : 'opacity-0 pointer-events-none'}"
        style={isPro ? "padding-bottom: calc(var(--sab) + 0.75rem); padding-left: calc(var(--sal) + 0.75rem); padding-right: calc(var(--sar) + 0.75rem);" : ""}
    >
        {#if isPro}
            <!-- Full Controls (Only in Fullscreen) -->
            <!-- Progress Bar -->
            <div class="relative w-full h-1 group/progress mb-3 px-1">
                <input 
                    type="range" 
                    min="0" 
                    max="100" 
                    value={(currentTime / duration) * 100 || 0} 
                    oninput={handleSeek}
                    class="absolute inset-0 w-full h-full opacity-0 cursor-pointer z-10"
                />
                <div class="absolute inset-x-1 inset-y-0 bg-white/20 rounded-full overflow-hidden">
                    <div 
                        class="h-full bg-entropy-primary shadow-[0_0_8px_rgba(var(--entropy-primary-rgb),0.6)]" 
                        style="width: {(currentTime / duration) * 100 || 0}%"
                    ></div>
                </div>
            </div>

            <!-- Controls Bar -->
            <div class="flex items-center justify-between w-full">
                <div class="flex items-center space-x-4">
                    <button 
                        onclick={togglePlay}
                        class="text-white hover:text-entropy-primary transition-colors transform active:scale-90"
                    >
                        {#if isPlaying}
                            <LucidePause size={20} fill="currentColor" />
                        {:else}
                            <LucidePlay size={20} fill="currentColor" />
                        {/if}
                    </button>

                    <div class="flex items-center space-x-2">
                        <button onclick={toggleMute} class="text-white/80 hover:text-white transition-colors">
                            {#if isMuted}
                                <LucideVolumeX size={20} />
                            {:else}
                                <LucideVolume2 size={20} />
                            {/if}
                        </button>
                    </div>

                    <div class="text-white/80 text-[10px] font-bold tabular-nums tracking-widest uppercase">
                        {formatTime(currentTime)} / {formatTime(duration)}
                    </div>
                </div>

                <button 
                    onclick={toggleFullscreen}
                    class="text-white/80 hover:text-white transition-all transform active:scale-90"
                >
                    <LucideMaximize size={20} />
                </button>
            </div>
        {:else}
            <!-- Minimal Center Play Button (Normal Mode) -->
            <button 
                class="w-16 h-16 rounded-full bg-black/40 backdrop-blur-md flex items-center justify-center text-white border border-white/10 hover:bg-black/60 transition-all scale-110 shadow-2xl active:scale-95"
                onclick={togglePlay}
            >
                {#if isPlaying}
                    <LucidePause size={32} fill="currentColor" />
                {:else}
                    <LucidePlay size={32} fill="currentColor" class="ml-1" />
                {/if}
            </button>
        {/if}
    </div>
</div>
