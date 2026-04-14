<script lang="ts">
  import { LucidePlay, LucidePause, LucideMic } from 'lucide-svelte';
  import { onMount } from 'svelte';
  import { playingVoiceNoteId } from '../lib/stores/audio';
  import { invoke } from '@tauri-apps/api/core';

  let { src, id, isMine = false, initialDuration = 0 } = $props();
  const hasAuthoritativeDuration = $derived(initialDuration > 0);

  let audioEl = $state<HTMLAudioElement | null>(null);
  let canvasEl = $state<HTMLCanvasElement | null>(null);
  let isPlaying = $state(false);
  let currentTime = $state(0);
  let duration = $state(initialDuration);
  $effect(() => {
    if (initialDuration > 0) duration = initialDuration;
  });
  let playbackSpeed = $state(1);
  let waveformData = $state<number[]>([]);
  let blobUrl = $state<string | null>(null);
  const speeds = [1, 1.5, 2];

  let lastGeneratedId = $state<string | null>(null);
  async function generateWaveform() {
    if (!id || (id === lastGeneratedId && waveformData.length > 0)) return;
    try {
      let arrayBuffer: ArrayBuffer;
      
      if (id === 'preview' && src) {
        blobUrl = src;
        const response = await fetch(src);
        arrayBuffer = await response.arrayBuffer();
      } else {
        const bytes = await invoke<number[]>('vault_load_media', { id });
        arrayBuffer = new Uint8Array(bytes).buffer;
        
        if (blobUrl) URL.revokeObjectURL(blobUrl);
        blobUrl = URL.createObjectURL(new Blob([arrayBuffer]));
      }

      const audioCtx = new (window.AudioContext || (window as any).webkitAudioContext)();
      const audioBuffer = await audioCtx.decodeAudioData(arrayBuffer);
      
      const rawData = audioBuffer.getChannelData(0);
      const samples = 45; 
      const blockSize = Math.floor(rawData.length / samples);
      const result = [];
      
      for (let i = 0; i < samples; i++) {
        let blockStart = blockSize * i;
        let sum = 0;
        for (let j = 0; j < Math.min(blockSize, rawData.length - blockStart); j++) {
          sum = sum + Math.abs(rawData[blockStart + j]);
        }
        result.push(sum / (blockSize || 1));
      }
      
      const max = Math.max(...result) || 1;
      waveformData = result.map(n => Math.max(0.1, n / max));
      lastGeneratedId = id;
      drawWaveform();
    } catch (e) {
      // Waveform generation failed
      waveformData = Array(45).fill(0.2);
    }
  }

  function drawWaveform() {
    if (!canvasEl || waveformData.length === 0) return;
    const ctx = canvasEl.getContext('2d');
    if (!ctx) return;

    const width = canvasEl.width;
    const height = canvasEl.height;
    const padding = 2.5; 
    const barWidth = (width / waveformData.length) - padding;
    
    ctx.clearRect(0, 0, width, height);
    
    waveformData.forEach((val, i) => {
      const x = i * (barWidth + padding);
      const barHeight = val * height * 0.7;
      const y = (height - barHeight) / 2;
      
      const progress = currentTime / (duration || 1);
      const isPlayed = (i / waveformData.length) < progress;
      
      if (isPlayed) {
        ctx.fillStyle = isMine ? '#ffffff' : '#8b5cf6';
      } else {
        ctx.fillStyle = isMine ? 'rgba(255, 255, 255, 0.4)' : 'rgba(139, 92, 246, 0.35)';
      }
      
      ctx.beginPath();
      ctx.roundRect(x, y, barWidth, barHeight, 1.5);
      ctx.fill();
    });
  }

  function togglePlay() {
    if (!audioEl) return;
    if (isPlaying) {
      audioEl.pause();
      isPlaying = false;
      playingVoiceNoteId.set(null);
    } else {
      playingVoiceNoteId.set(id);
      audioEl.play().catch(err => {
          // Playback failed
          isPlaying = false;
          playingVoiceNoteId.set(null);
      });
      isPlaying = true;
    }
  }

  $effect(() => {
      const currentId = $playingVoiceNoteId;
      if (currentId !== id && isPlaying) {
          if (audioEl) audioEl.pause();
          isPlaying = false;
      }
  });

  function toggleSpeed() {
    if (!audioEl) return;
    const currentIndex = speeds.indexOf(playbackSpeed);
    const nextIndex = (currentIndex + 1) % speeds.length;
    playbackSpeed = speeds[nextIndex];
    audioEl.playbackRate = playbackSpeed;
  }

  function handleTimeUpdate() {
    if (audioEl) {
      currentTime = audioEl.currentTime;
      drawWaveform();
    }
  }

  function handleMetadata() {
    if (hasAuthoritativeDuration) {
        duration = initialDuration;
        return; 
    }
    
    if (audioEl && audioEl.duration !== Infinity && !isNaN(audioEl.duration)) {
        duration = audioEl.duration;
    }
  }

  function handleSeek(e: MouseEvent) {
    if (!canvasEl || !audioEl) return;
    const rect = canvasEl.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const progress = Math.max(0, Math.min(1, x / rect.width));
    audioEl.currentTime = progress * (duration || 0);
    currentTime = audioEl.currentTime;
    drawWaveform();
  }

  function formatTime(s: number) {
    if (!s || isNaN(s)) return "0:00";
    const mins = Math.floor(s / 60);
    const secs = Math.floor(s % 60);
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  onMount(() => {
    generateWaveform();
    return () => {
      if (audioEl) audioEl.pause();
      if (blobUrl) URL.revokeObjectURL(blobUrl);
    };
  });

  $effect(() => {
    if (src) generateWaveform();
  });
</script>

<div class="flex items-center space-x-3 py-1 px-1.5 min-w-[240px] select-none rounded-[1rem] transition-all {isPlaying ? 'bg-black/5' : ''}">
  <button 
    onclick={togglePlay}
    class="w-10 h-10 shrink-0 rounded-full flex items-center justify-center transition-all {isMine ? 'bg-white text-entropy-primary hover:bg-white/90' : 'bg-entropy-primary text-white hover:bg-entropy-primary-dim'}"
  >
    {#if isPlaying}
      <LucidePause size={20} fill="currentColor" />
    {:else}
      <LucidePlay size={20} fill="currentColor" class="translate-x-0.5" />
    {/if}
  </button>

  <div class="flex-1 space-y-0.5 min-w-0">
      <div 
        class="relative h-8 w-full cursor-pointer flex items-center" 
        onclick={handleSeek}
        onkeypress={(e) => e.key === 'Enter' && togglePlay()}
        role="button"
        tabindex="0"
      >
          <canvas 
            bind:this={canvasEl} 
            width="180" 
            height="32" 
            class="w-full h-full opacity-90"
            style="image-rendering: pixelated; image-rendering: -moz-crisp-edges; image-rendering: crisp-edges;"
          ></canvas>
      </div>
      <div class="flex justify-between items-center px-0.5">
           <span class="text-[10px] font-bold {isMine ? 'text-white/80' : 'text-entropy-primary/80'} tabular-nums">
               {formatTime(isPlaying ? currentTime : duration)}
           </span>
           <div class="flex items-center space-x-2">
               <button 
                   onclick={toggleSpeed}
                   class="text-[9px] font-black uppercase tracking-widest {isMine ? 'text-white/60' : 'text-entropy-primary/60'} hover:opacity-100 transition-opacity"
               >
                   {playbackSpeed}x
               </button>
               <LucideMic size={11} class={isMine ? 'text-white/40' : 'text-entropy-primary/40'} />
           </div>
      </div>
  </div>

  <audio 
    bind:this={audioEl} 
    src={blobUrl} 
    ontimeupdate={handleTimeUpdate} 
    onloadedmetadata={handleMetadata}
    onended={() => { isPlaying = false; currentTime = 0; playingVoiceNoteId.set(null); drawWaveform(); }}
    hidden
  ></audio>
</div>
