<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { LucideTrash, LucideSquare, LucideSend, LucideMicOff, LucideX } from 'lucide-svelte';
  import VoiceNotePlayer from './VoiceNotePlayer.svelte';
  import { addToast } from '../lib/stores/ui';

  let { onSend, onCancel } = $props<{ 
      onSend: (blob: Blob, duration: number) => void, 
      onCancel: () => void 
  }>();

  let recordingState = $state<'recording' | 'preview' | 'error'>('recording');
  let errorMessage = $state<string | null>(null);
  let recordedBlob = $state<Blob | null>(null);
  let previewUrl = $state<string | null>(null);
  let startTime = $state<number>(0);
  let finalDurationSeconds = $state(0);
  let recordingSeconds = $state(0);
  let recordingInterval: any = null;
  let visualizerCanvas = $state<HTMLCanvasElement | null>(null);
  let currentVolume = $state(0);
  let mediaRecorder = $state<MediaRecorder | null>(null);
  let audioContext = $state<AudioContext | null>(null);
  let analyser = $state<AnalyserNode | null>(null);
  let animationFrame: number | null = null;

  const startRecording = async () => {
    try {
        const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
        startTime = performance.now();
        
        audioContext = new (window.AudioContext || (window as any).webkitAudioContext)();
        analyser = audioContext.createAnalyser();
        const source = audioContext.createMediaStreamSource(stream);
        source.connect(analyser);
        analyser.fftSize = 256;
        
        const dataArray = new Uint8Array(analyser.frequencyBinCount);
        const updateVolume = () => {
             if (analyser) {
                 analyser.getByteFrequencyData(dataArray);
                 const avg = dataArray.reduce((p, c) => p + c, 0) / dataArray.length;
                 currentVolume = avg / 255.0;
                 drawNativeWaveform();
                 animationFrame = requestAnimationFrame(updateVolume);
             }
        };
        updateVolume();
 
        const mimeType = MediaRecorder.isTypeSupported('audio/webm;codecs=opus') 
            ? 'audio/webm;codecs=opus' 
            : 'audio/ogg;codecs=opus';
        
        mediaRecorder = new MediaRecorder(stream, { 
            mimeType,
            audioBitsPerSecond: 16000 
        });
        const chunks: Blob[] = [];
        
        mediaRecorder.ondataavailable = (e) => {
            if (e.data.size > 0) chunks.push(e.data);
        };
        
        mediaRecorder.onstop = () => {
            const durationMs = performance.now() - startTime;
            if (durationMs < 500) {
                addToast("Recording too short", "error");
                cancelRecording();
                return;
            }

            recordedBlob = new Blob(chunks, { type: mimeType });
            previewUrl = URL.createObjectURL(recordedBlob);
            finalDurationSeconds = durationMs / 1000;
            if (recordingInterval) clearInterval(recordingInterval);
            recordingState = 'preview';
            isStopping = false;
        };

        mediaRecorder.start();
        recordingSeconds = 0;
        recordingInterval = setInterval(() => { 
            recordingSeconds = Math.floor((performance.now() - startTime) / 1000); 
        }, 200);
    } catch (e: any) { 
        // Start Error
        recordingState = 'error';
        if (e.name === 'NotAllowedError') {
            errorMessage = "Microphone Permission Denied";
        } else if (e.name === 'NotFoundError') {
            errorMessage = "No Microphone Found";
        } else {
            errorMessage = e.message || "Failed to start recording";
        }
    }
  };

  const drawNativeWaveform = () => {
      if (!visualizerCanvas) return;
      const ctx = visualizerCanvas.getContext('2d');
      if (!ctx) return;
      
      const width = visualizerCanvas.width;
      const height = visualizerCanvas.height;
      const bars = 50;
      const barWidth = width / bars;

      const imageData = ctx.getImageData(barWidth, 0, width - barWidth, height);
      ctx.clearRect(0, 0, width, height);
      ctx.putImageData(imageData, 0, 0);

      // Boost gain visually for modern sleek look
      const barHeight = Math.max(4, currentVolume * height * 1.5);
      ctx.fillStyle = '#ef4444';
      ctx.beginPath();
      ctx.roundRect(width - barWidth + 1, (height - barHeight) / 2, barWidth - 3, barHeight, 1.5);
      ctx.fill();
  };

  let isStopping = false;
  const stopRecording = async () => {
      if (recordingState === 'recording' && !isStopping && mediaRecorder) {
          isStopping = true;
          mediaRecorder.stop();
          mediaRecorder.stream.getTracks().forEach(t => t.stop());
          
          if (recordingInterval) clearInterval(recordingInterval);
          if (animationFrame) cancelAnimationFrame(animationFrame);
          if (audioContext) audioContext.close();
      }
  };

  const cancelRecording = async () => {
      if (mediaRecorder && recordingState === 'recording') {
          mediaRecorder.stop();
          mediaRecorder.stream.getTracks().forEach(t => t.stop());
      }
      if (recordingInterval) clearInterval(recordingInterval);
      if (animationFrame) cancelAnimationFrame(animationFrame);
      if (audioContext) audioContext.close();
      if (previewUrl) URL.revokeObjectURL(previewUrl);
      onCancel();
  };

  const handleSend = () => {
      if (recordedBlob) {
          onSend(recordedBlob, finalDurationSeconds);
      }
  };

  const formatRecordingTime = (s: number) => {
      const mins = Math.floor(s / 60);
      const secs = s % 60;
      return `${mins}:${secs.toString().padStart(2, '0')}`;
  };

  onMount(() => {
    startRecording();
    return () => {
        if (mediaRecorder && recordingState === 'recording') {
            mediaRecorder.stop();
            mediaRecorder.stream.getTracks().forEach(t => t.stop());
        }
        if (recordingInterval) clearInterval(recordingInterval);
        if (animationFrame) cancelAnimationFrame(animationFrame);
        if (audioContext) audioContext.close();
        if (previewUrl) URL.revokeObjectURL(previewUrl);
    };
  });
</script>

{#if recordingState === 'recording'}
    <div class="flex-1 flex items-center justify-between bg-white/[0.03] backdrop-blur-md px-5 py-2 rounded-[1.5rem] border border-red-500/20 shadow-xl shadow-red-500/5 animate-in slide-in-from-bottom-2 duration-300 h-[52px]">
        <div class="flex items-center space-x-4 flex-1 h-full">
            <div class="relative flex items-center justify-center">
                <div class="w-3 h-3 bg-red-500 rounded-full animate-ping absolute opacity-40"></div>
                <div class="w-3 h-3 bg-red-500 rounded-full relative shadow-[0_0_12px_rgba(239,68,68,0.8)]"></div>
            </div>
            <div class="flex flex-col">
                <span class="text-[9px] font-black text-red-500 uppercase tracking-[0.2em] leading-none mb-0.5">Capturing</span>
                <span class="text-xs font-mono font-black text-entropy-text-primary tabular-nums">{formatRecordingTime(recordingSeconds)}</span>
            </div>
            <div class="flex-1 h-8 flex items-center bg-white/5 rounded-xl px-4 border border-white/5">
                <canvas bind:this={visualizerCanvas} width="200" height="32" class="w-full h-full opacity-80"></canvas>
            </div>
        </div>
        <div class="flex items-center space-x-2 pl-3">
            <button onclick={cancelRecording} class="p-2.5 text-entropy-text-dim hover:text-red-500 hover:bg-red-500/10 rounded-xl transition-all active:scale-90" title="Discard"><LucideTrash size={20} /></button>
            <button onclick={stopRecording} class="w-10 h-10 bg-red-500 text-white rounded-xl shadow-[0_8px_16px_rgba(239,68,68,0.3)] hover:bg-red-600 transition-all active:scale-95 flex items-center justify-center group" title="Stop Recording">
                <LucideSquare size={16} fill="white" class="group-hover:scale-110 transition-transform" />
            </button>
        </div>
    </div>
{:else if recordingState === 'error'}
    <div class="flex-1 flex items-center justify-between bg-red-500/10 backdrop-blur-md px-5 py-2 rounded-[1.5rem] border border-red-500/40 shadow-xl shadow-red-500/10 animate-in slide-in-from-bottom-2 duration-300 h-[52px]">
        <div class="flex items-center space-x-3 text-red-500 flex-1">
            <div class="w-8 h-8 rounded-full bg-red-500/20 flex items-center justify-center">
                <LucideMicOff size={16} />
            </div>
            <div class="flex flex-col">
                <span class="text-[9px] font-black uppercase tracking-[0.2em] leading-none mb-0.5">Recording Error</span>
                <span class="text-xs font-black text-entropy-text-primary">{errorMessage}</span>
            </div>
        </div>
        <button onclick={cancelRecording} class="w-10 h-10 flex items-center justify-center text-red-500 hover:bg-red-500/10 rounded-xl transition-all" title="Close">
            <LucideX size={20} />
        </button>
    </div>
{:else if previewUrl}
    <div class="flex-1 flex items-center space-x-3 bg-white/[0.03] backdrop-blur-md px-4 py-2.5 rounded-[1.5rem] border border-white/10 shadow-2xl animate-in zoom-in-95 duration-300">
        <button onclick={cancelRecording} class="w-10 h-10 flex items-center justify-center text-entropy-text-dim hover:text-red-500 hover:bg-red-500/10 rounded-xl transition-all" title="Discard"><LucideTrash size={20} /></button>
        <div class="flex-1">
            <VoiceNotePlayer src={previewUrl} id="preview" isMine={true} initialDuration={finalDurationSeconds} />
        </div>
        <button onclick={handleSend} class="w-11 h-11 bg-entropy-primary text-white rounded-xl shadow-[0_8px_24px_rgba(139,92,246,0.3)] hover:bg-entropy-primary-dim active:scale-95 transition-all flex items-center justify-center group">
            <LucideSend size={22} class="translate-x-0.5 group-hover:translate-x-1 group-hover:-translate-y-0.5 transition-transform" />
        </button>
    </div>
{/if}
