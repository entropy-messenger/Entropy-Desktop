<script lang="ts">
  import { sendTypingStatus, setActiveAudio, clearActiveAudio } from '../../lib/store';
  import { LucidePaperclip, LucideMic, LucideSend, LucideX, LucidePlay, LucidePause, LucideTrash2, LucideSquare } from 'lucide-svelte';
  import type { Message } from '../../lib/types';
  import { onDestroy, untrack } from 'svelte';
  import { userStore } from '../../lib/stores/user';

  interface Props {
    peerHash?: string;
    replyingTo: Message | null;
    onCancelReply: () => void;
    onSend: (text: string) => void;
    onFileSelect: (file: File) => void;
    onToggleRecording: () => void;
    isRecording: boolean;
    recordedBlob?: Blob | null;
    onDiscardRecording?: () => void;
    onSendRecording?: (duration?: number) => void;
  }

  let { 
    peerHash, replyingTo, onCancelReply, onSend, onFileSelect, 
    onToggleRecording, isRecording, recordedBlob, onDiscardRecording, onSendRecording 
  }: Props = $props();

  let messageInput = $state("");
  let fileInput = $state<HTMLInputElement | null>(null);
  let isTyping = $state(false);
  let typingTimeout: any = null;

  // Voice Note states
  let recordedBlobUrl = $state<string | null>(null);
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

  // Recording Timer
  let recordingSeconds = $state(0);
  let recordedDuration = $state(0);

  let previewDuration = $derived(
    (duration > 0 && isFinite(duration)) 
      ? duration 
      : (recordedDuration || recordingSeconds || 1)
  );

  let progressPercentage = $derived(
    Math.min(100, (currentTime / (previewDuration || 1)) * 100)
  );

  let lastStatusSentTp = 0;
  const handleInputChange = (e: Event) => {
      const target = e.target as HTMLTextAreaElement;
      messageInput = target.value;
      
      const now = Date.now();
      const hasContent = messageInput.length > 0;

      if (peerHash) {
          if (hasContent) {
              // Send "typing: true" if we're not already typing OR if it's been a while (re-broadcast)
              if (!isTyping || now - lastStatusSentTp > 4000) {
                  isTyping = true;
                  lastStatusSentTp = now;
                  sendTypingStatus(peerHash, true);
              }
              
              // Reset the "stopped typing" timer
              if (typingTimeout) clearTimeout(typingTimeout);
              typingTimeout = setTimeout(() => {
                  if (isTyping && peerHash) {
                      isTyping = false;
                      sendTypingStatus(peerHash, false);
                  }
              }, 1500);
          } else {
              // Input cleared! Send "typing: false" immediately
              if (isTyping) {
                  isTyping = false;
                  if (typingTimeout) clearTimeout(typingTimeout);
                  sendTypingStatus(peerHash, false);
              }
          }
      }
  };

  const handleBlur = () => {
      if (isTyping && peerHash) {
          isTyping = false;
          if (typingTimeout) clearTimeout(typingTimeout);
          sendTypingStatus(peerHash, false);
      }
  };

  $effect(() => {
    const blob = recordedBlob;
    untrack(() => {
      if (blob) {
          if (recordedBlobUrl) URL.revokeObjectURL(recordedBlobUrl);
          recordedBlobUrl = URL.createObjectURL(blob);
      } else {
          if (recordedBlobUrl) URL.revokeObjectURL(recordedBlobUrl);
          recordedBlobUrl = null;
          isPlaying = false;
      }
    });
  });

  let timerInterval: any = null;
  $effect(() => {
    if (isRecording) {
      if (!timerInterval) {
        recordingSeconds = 0;
        recordedDuration = 0;
        timerInterval = setInterval(() => {
          recordingSeconds++;
        }, 1000);
      }
    } else {
      if (timerInterval) {
        clearInterval(timerInterval);
        timerInterval = null;
        recordedDuration = recordingSeconds;
      }
    }

    return () => {
      if (timerInterval) {
        clearInterval(timerInterval);
        timerInterval = null;
      }
    };
  });

  onDestroy(() => {
      if (recordedBlobUrl) URL.revokeObjectURL(recordedBlobUrl);
      if (isTyping && peerHash) {
          sendTypingStatus(peerHash, false);
      }
  });

  const handleSend = () => {
    if (!messageInput.trim()) return;
    if (isTyping && peerHash) {
        isTyping = false;
        if (typingTimeout) clearTimeout(typingTimeout);
        sendTypingStatus(peerHash, false);
    }
    onSend(messageInput);
    messageInput = "";
  };


  const handleKeydown = (e: KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        handleSend();
    }
  };

  const onFileInputChange = (e: Event) => {
    const files = (e.target as HTMLInputElement).files;
    if (files && files.length > 0) onFileSelect(files[0]);
  };

  $effect(() => {
    const activeId = $userStore.activeAudioId;
    if (activeId && activeId !== 'recording-preview' && isPlaying && audioPlayer) {
        audioPlayer.pause();
    }
  });

  const togglePlayback = () => {
      if (!audioPlayer) return;
      if (isPlaying) {
          audioPlayer.pause();
      } else {
          setActiveAudio('recording-preview', peerHash || 'unknown');
          audioPlayer.play();
      }
  };

  const formatElapsedTime = (time: number) => {
      if (!time || isNaN(time) || !isFinite(time)) return "0:00";
      let totalSeconds = Math.floor(time);
      if (totalSeconds > 360000) {
          totalSeconds = Math.floor(time / 1000);
      }
      const mins = Math.floor(totalSeconds / 60);
      const secs = Math.floor(totalSeconds % 60);
      return `${mins}:${secs.toString().padStart(2, '0')}`;
  };
</script>

<div class="bg-white/95 backdrop-blur-md p-3 pb-4 border-t border-gray-100 z-20">
    {#if replyingTo}
        <div class="mb-3 p-3 bg-indigo-50/80 backdrop-blur rounded-2xl flex items-center justify-between border border-indigo-100 shadow-sm animate-in slide-in-from-bottom-2 duration-200">
            <div class="flex items-center space-x-3 overflow-hidden">
                <div class="w-1 h-8 bg-indigo-500 rounded-full"></div>
                <div class="flex flex-col min-w-0">
                    <span class="text-[10px] font-black uppercase text-indigo-600 tracking-wider">Replying to {replyingTo.senderAlias || 'Peer'}</span>
                    <span class="text-xs text-gray-600 truncate opacity-70 italic">{replyingTo.content}</span>
                </div>
            </div>
            <button onclick={onCancelReply} class="p-2 text-gray-400 hover:text-indigo-600 transition hover:bg-white rounded-xl">
                <LucideX size={18} />
            </button>
        </div>
    {/if}

    <div class="flex items-center space-x-2 max-w-5xl mx-auto h-[52px]">
        {#if recordedBlobUrl}
            <div class="flex-1 flex items-center bg-gray-50 rounded-2xl px-4 h-full space-x-4 animate-in zoom-in-95 duration-200 border border-gray-100">
                <button 
                    onclick={onDiscardRecording}
                    class="p-2 text-gray-400 hover:text-red-500 transition active:scale-90"
                    title="Discard"
                >
                    <LucideTrash2 size={20} />
                </button>

                <div class="flex-1 flex items-center space-x-3 px-2 border-l border-r border-gray-200 h-full">
                    <button 
                        onclick={togglePlayback}
                        class="p-2 bg-indigo-600 text-white rounded-full hover:bg-indigo-700 transition active:scale-95 shadow-md flex-shrink-0"
                    >
                        {#if isPlaying}
                            <LucidePause size={16} />
                        {:else}
                            <LucidePlay size={16} class="ml-0.5" />
                        {/if}
                    </button>
                    
                    <div class="flex-1 h-1.5 bg-gray-200 rounded-full relative overflow-hidden">
                        <div 
                            class="absolute top-0 left-0 h-full bg-indigo-500"
                            style="width: {progressPercentage}%"
                        ></div>
                    </div>
                    
                    <button 
                        onclick={togglePlaybackRate}
                        class="text-[9px] font-black w-8 h-5 flex items-center justify-center bg-gray-100 text-gray-500 hover:bg-gray-200 rounded-md transition-colors tabular-nums"
                    >
                        {playbackRate}x
                    </button>

                    <span class="text-[10px] font-black text-gray-500 tabular-nums w-8 flex-shrink-0">
                        {formatElapsedTime(currentTime || 0)}
                    </span>

                    <audio 
                        src={recordedBlobUrl} 
                        bind:this={audioPlayer}
                        bind:currentTime
                        bind:duration
                        bind:playbackRate
                        onplay={() => isPlaying = true}
                        onpause={() => {
                            isPlaying = false;
                            clearActiveAudio('recording-preview');
                        }}
                        onended={() => { 
                            isPlaying = false; 
                            currentTime = 0; 
                            clearActiveAudio('recording-preview');
                        }}
                        class="hidden"
                    ></audio>
                </div>

                <button 
                    onclick={() => {
                        if (isTyping && peerHash) {
                            isTyping = false;
                            if (typingTimeout) clearTimeout(typingTimeout);
                            sendTypingStatus(peerHash, false);
                        }
                        onSendRecording?.(recordingSeconds > 0 ? recordingSeconds : (duration > 0 && isFinite(duration) ? duration : 0));
                    }}
                    class="p-3 bg-indigo-600 text-white rounded-xl shadow-lg shadow-indigo-200 hover:bg-indigo-700 hover:-translate-y-0.5 transition-all active:scale-95 flex-shrink-0"
                >
                    <LucideSend size={20} />
                </button>
            </div>
        {:else if isRecording}
            <div class="flex-1 flex items-center bg-red-50 rounded-2xl px-5 h-full space-x-4 border border-red-100 animate-in slide-in-from-right-4 duration-300">
                <div class="flex items-center space-x-3 text-red-500 flex-1">
                    <div class="relative">
                        <div class="w-3 h-3 bg-red-500 rounded-full animate-ping absolute inset-0 opacity-75"></div>
                        <div class="w-3 h-3 bg-red-600 rounded-full relative"></div>
                    </div>
                    <div class="flex flex-col">
                        <span class="text-[9px] font-black uppercase tracking-[0.2em]">Recording</span>
                        <div class="text-[10px] font-bold text-red-400">Capturing audio...</div>
                    </div>
                </div>
                <div class="text-sm font-mono font-black text-red-600 tabular-nums bg-white px-4 py-1.5 rounded-xl border border-red-100 shadow-sm">
                    {formatElapsedTime(recordingSeconds)}
                </div>
            </div>
            <button 
                onclick={onToggleRecording}
                class="w-12 h-12 bg-red-600 text-white rounded-2xl flex items-center justify-center transition-all active:scale-90 shadow-lg shadow-red-200 hover:bg-red-700 animate-in zoom-in-75 duration-200"
                title="Stop Recording"
            >
                <LucideSquare size={24} fill="currentColor" />
            </button>
        {:else}
            <button 
                onclick={() => fileInput?.click()}
                class="p-3.5 text-gray-500 hover:text-indigo-600 hover:bg-indigo-50 rounded-2xl transition active:scale-90"
            >
                <LucidePaperclip size={22} />
            </button>
            <input type="file" bind:this={fileInput} onchange={onFileInputChange} class="hidden" />

            <div class="flex-1 relative h-full">
                <textarea 
                    value={messageInput}
                    oninput={handleInputChange}
                    onblur={handleBlur}
                    onkeydown={handleKeydown}
                    placeholder="Secure message..."
                    class="w-full h-full bg-gray-100 border-none rounded-2xl p-3.5 px-4 text-sm focus:ring-2 focus:ring-indigo-500/20 focus:bg-white transition-all resize-none font-medium custom-scrollbar"
                    rows="1"
                ></textarea>
            </div>

            {#if messageInput.trim().length > 0}
                <button 
                    onclick={handleSend}
                    class="w-12 h-12 bg-indigo-600 text-white rounded-2xl flex items-center justify-center shadow-lg shadow-indigo-200 hover:bg-indigo-700 hover:-translate-y-0.5 transition-all active:scale-95"
                >
                    <LucideSend size={22} />
                </button>
            {:else}
                <button 
                    onclick={onToggleRecording}
                    class="w-12 h-12 bg-gray-100 text-gray-500 hover:bg-indigo-50 hover:text-indigo-600 rounded-2xl flex items-center justify-center transition-all active:scale-95"
                    title="Record Voice Note"
                >
                    <LucideMic size={22} />
                </button>
            {/if}
        {/if}
    </div>
</div>

<style>
    textarea::-webkit-scrollbar { width: 4px; }
    textarea::-webkit-scrollbar-track { background: transparent; }
    textarea::-webkit-scrollbar-thumb { background: rgba(0,0,0,0.05); border-radius: 10px; }

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
        background: #4f46e5;
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
