<script lang="ts">
  import { LucidePlay, LucidePause, LucideMic, LucideLoader } from 'lucide-svelte';
  import { onMount } from 'svelte';
  import { playingVoiceNoteId } from '../lib/stores/audio';
  import { getMediaUrl } from '../lib/actions/chat';

  let { src, id, isMine = false, initialDuration = 0 } = $props();
  const hasAuthoritativeDuration = $derived(initialDuration > 0);

  let audioEl = $state<HTMLAudioElement | null>(null);
  let canvasEl = $state<HTMLCanvasElement | null>(null);
  let isPlaying = $state(false);
  let isLoading = $state(false);
  let currentTime = $state(0);
  let duration = $state(initialDuration);
  $effect(() => {
    if (initialDuration > 0) duration = initialDuration;
  });
  let playbackSpeed = $state(1);
  let waveformData = $state<number[]>([]);
  let proxyUrl = $state<string | null>(null);
  const speeds = [1, 1.5, 2];

  let lastGeneratedId = $state<string | null>(null);

  async function loadAudioProxy() {
      if (!id || id === 'preview') return;
      try {
          isLoading = true;
          const url = await getMediaUrl(id, 'audio/wav');
          proxyUrl = url;
          
          // Generate real waveform by fetching from proxy
          const response = await fetch(url);
          const arrayBuffer = await response.arrayBuffer();
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

      } finally {
          isLoading = false;
      }
  }

  async function generatePreviewWaveform() {
      if (!src) return;
      try {
          proxyUrl = src;
          const response = await fetch(src);
          const arrayBuffer = await response.arrayBuffer();
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
          drawWaveform();
      } catch(e) {
          waveformData = Array(45).fill(0.2);
          drawWaveform();
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

  async function togglePlay() {
    if (!audioEl) return;

    // JIT Loading Logic
    if (!proxyUrl && !src && !isLoading) {
        await loadAudioProxy();
        
        // Wait a micro-tick for Svelte to bind the new proxyUrl to audioEl.src
        setTimeout(() => {
            if (!audioEl) return;
            audioEl.play().catch(err => {
                isPlaying = false;
                playingVoiceNoteId.set(null);
            });
            isPlaying = true;
            playingVoiceNoteId.set(id);
        }, 50);
        return;
    }

    if (isPlaying) {
      audioEl.pause();
      isPlaying = false;
      playingVoiceNoteId.set(null);
    } else {
      playingVoiceNoteId.set(id);
      audioEl.play().catch(err => {
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
    
    // Auto-load if attempting to seek an unloaded audio
    if (!proxyUrl && !src && !isLoading) {
        loadAudioProxy().then(() => {
            setTimeout(() => {
                executeSeek(e);
            }, 50);
        });
        return;
    }
    executeSeek(e);
  }

  function executeSeek(e: MouseEvent) {
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
    if (src) {
        generatePreviewWaveform();
    } else {
        // Create a high-entropy dummy waveform that is unique to each ID
        const result = [];
        const str = id || 'fallback';
        for (let i = 0; i < 45; i++) {
            // Use different characters from the ID to jitter each bar uniquely
            const charCode = str.charCodeAt(i % str.length);
            const secondaryCode = str.charCodeAt((i + 5) % str.length);
            
            // Generate a more 'jagged' and unique variation
            let val = ((charCode * (i + 1)) % 50) / 100; // 0.0 to 0.5
            val = val + ((secondaryCode % 10) / 40);    // add some jitter
            
            result.push(Math.max(0.15, Math.min(0.7, val)));
        }
        waveformData = result;
        drawWaveform();
    }
    
    return () => {
      if (audioEl) audioEl.pause();
    };
  });

</script>

<div class="flex items-center space-x-3 py-1 px-1.5 min-w-[210px] select-none rounded-[1.2rem] transition-all {isPlaying ? 'bg-black/5' : ''}">
  <button 
    onclick={togglePlay}
    class="w-10 h-10 shrink-0 rounded-full flex items-center justify-center transition-all {isMine ? 'bg-white text-entropy-primary hover:bg-white/90' : 'bg-entropy-primary text-white hover:bg-entropy-primary-dim'}"
  >
    {#if isLoading}
        <LucideLoader size={20} class="animate-spin {isMine ? 'text-entropy-primary' : 'text-white'}" />
    {:else if isPlaying}
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
      <div class="flex items-center px-2">
           <button 
               onclick={toggleSpeed}
               class="text-[9px] font-black uppercase tracking-tighter px-1.5 py-0.5 rounded transition-all active:scale-90 mr-2
               {isMine 
                   ? (playbackSpeed > 1 ? 'bg-white text-entropy-primary shadow-sm' : 'bg-white/10 text-white/80 hover:bg-white/20') 
                   : (playbackSpeed > 1 ? 'bg-entropy-primary text-white shadow-sm' : 'bg-entropy-primary/10 text-entropy-primary hover:bg-entropy-primary/20')}"
           >
               {playbackSpeed}x
           </button>
           <span class="text-[9px] font-bold {isMine ? 'text-white/70' : 'text-entropy-primary/70'} tabular-nums">
               {formatTime(isPlaying ? currentTime : duration)}
           </span>
           <div class="flex-1"></div>
      </div>
  </div>

  <audio 
    bind:this={audioEl} 
    src={src || proxyUrl} 
    ontimeupdate={handleTimeUpdate} 
    onloadedmetadata={handleMetadata}
    onended={() => { isPlaying = false; currentTime = 0; playingVoiceNoteId.set(null); drawWaveform(); }}
    hidden
  ></audio>
</div>
