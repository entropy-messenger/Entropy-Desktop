<script lang="ts">
  import { LucidePlay, LucidePause, LucideMic } from 'lucide-svelte';
  import { onMount } from 'svelte';
  import { playingVoiceNoteId } from '../lib/stores/audio';

  let { src, id, isMine = false } = $props();

  let audioEl = $state<HTMLAudioElement | null>(null);
  let canvasEl = $state<HTMLCanvasElement | null>(null);
  let isPlaying = $state(false);
  let currentTime = $state(0);
  let duration = $state(0);
  let playbackSpeed = $state(1);
  let waveformData = $state<number[]>([]);
  const speeds = [1, 1.5, 2];

  async function generateWaveform() {
    if (!src) return;
    try {
      const response = await fetch(src);
      const arrayBuffer = await response.arrayBuffer();
      const audioCtx = new (window.AudioContext || (window as any).webkitAudioContext)();
      const audioBuffer = await audioCtx.decodeAudioData(arrayBuffer);
      
      const rawData = audioBuffer.getChannelData(0);
      const samples = 45; // Slightly fewer bars for better spacing
      const blockSize = Math.floor(rawData.length / samples);
      const result = [];
      
      for (let i = 0; i < samples; i++) {
        let blockStart = blockSize * i;
        let sum = 0;
        for (let j = 0; j < blockSize; j++) {
          sum = sum + Math.abs(rawData[blockStart + j]);
        }
        result.push(sum / blockSize);
      }
      
      const max = Math.max(...result);
      waveformData = result.map(n => Math.max(0.1, n / max)); // Reduced min height to fix 'solid block' look
      drawWaveform();
    } catch (e) {
      console.error("Waveform generation failed:", e);
      waveformData = Array(45).fill(0.2);
    }
  }

  function drawWaveform() {
    if (!canvasEl || waveformData.length === 0) return;
    const ctx = canvasEl.getContext('2d');
    if (!ctx) return;

    const width = canvasEl.width;
    const height = canvasEl.height;
    const padding = 2.5; // Increased gap for distinction
    const barWidth = (width / waveformData.length) - padding;
    
    ctx.clearRect(0, 0, width, height);
    
    waveformData.forEach((val, i) => {
      const x = i * (barWidth + padding);
      const barHeight = val * height * 0.7;
      const y = (height - barHeight) / 2;
      
      const progress = currentTime / (duration || 1);
      const isPlayed = (i / waveformData.length) < progress;
      
      // COLORS:
      // If Mine: Bubble is Purple, so use White/Transparent White
      // If Peer: Bubble is Light/Gray, so use Purple/Transparent Purple
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
      audioEl.play();
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
    if (audioEl) duration = audioEl.duration;
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
               <LucideMic size={11} class="{isMine ? 'text-white/40' : 'text-entropy-primary/40'}" />
           </div>
      </div>
  </div>

  <audio 
    bind:this={audioEl} 
    src={src} 
    ontimeupdate={handleTimeUpdate} 
    onloadedmetadata={handleMetadata}
    onended={() => { isPlaying = false; currentTime = 0; playingVoiceNoteId.set(null); drawWaveform(); }}
    hidden
  ></audio>
</div>

<style>
  canvas {
    image-rendering: crisp-edges;
  }
</style>
