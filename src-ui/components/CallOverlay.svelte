
<script lang="ts">
    import { callStore, callManager } from '../lib/call_manager';
    import { LucidePhoneOff, LucidePhone, LucideMic, LucideMicOff, LucideVideo, LucideVideoOff } from 'lucide-svelte';

    let localVideo = $state<HTMLVideoElement | null>(null);
    let remoteVideo = $state<HTMLVideoElement | null>(null);
    let localVideoSmall = $state<HTMLVideoElement | null>(null);
    let remoteVideoLarge = $state<HTMLVideoElement | null>(null);

    $effect(() => {
        if (localVideo && $callStore.localStream && localVideo.srcObject !== $callStore.localStream) {
            localVideo.srcObject = $callStore.localStream;
            localVideo.play().catch(() => {});
        }
    });

    $effect(() => {
        if (remoteVideo && $callStore.remoteStream && remoteVideo.srcObject !== $callStore.remoteStream) {
            remoteVideo.srcObject = $callStore.remoteStream;
            remoteVideo.play().catch(() => {});
        }
    });

    $effect(() => {
        if (localVideoSmall && $callStore.localStream && localVideoSmall.srcObject !== $callStore.localStream) {
            localVideoSmall.srcObject = $callStore.localStream;
            localVideoSmall.play().catch(() => {});
        }
    });

    $effect(() => {
        if (remoteVideoLarge && $callStore.remoteStream && remoteVideoLarge.srcObject !== $callStore.remoteStream) {
            remoteVideoLarge.srcObject = $callStore.remoteStream;
            remoteVideoLarge.play().catch(() => {});
        }
    });

    const hangup = () => {
        callManager.endCall();
    };

    const accept = () => {
        callManager.acceptCall();
    };
</script>

{#if $callStore.isActive}
    <div class="fixed inset-0 bg-black/90 z-[100] flex flex-col items-center justify-center text-white">
        
        
        <video bind:this={remoteVideo} autoplay playsinline style="position:fixed; top:0; left:0; width:1px; height:1px; opacity:0; pointer-events:none;"></video>
        <video bind:this={localVideo} autoplay playsinline muted style="position:fixed; top:0; left:0; width:1px; height:1px; opacity:0; pointer-events:none;"></video>

        {#if $callStore.status === 'ringing'}
            <div class="flex flex-col items-center space-y-6 animate-bounce-subtle">
                <div class="w-24 h-24 rounded-full bg-blue-500 flex items-center justify-center text-4xl font-bold">
                    {$callStore.peerHash?.slice(0, 1).toUpperCase()}
                </div>
                <div class="text-2xl font-semibold">Incoming {$callStore.type} Call...</div>
                <div class="text-sm text-gray-400">{$callStore.peerHash}</div>
                
                <div class="flex space-x-12 mt-12">
                    <button onclick={hangup} class="p-6 bg-red-500 rounded-full hover:bg-red-600 transition shadow-lg shadow-red-500/20">
                        <LucidePhoneOff size={32} />
                    </button>
                    <button onclick={accept} class="p-6 bg-green-500 rounded-full hover:bg-green-600 transition shadow-lg shadow-green-500/20">
                        <LucidePhone size={32} />
                    </button>
                </div>
            </div>
        {:else}
            
            <div class="relative w-full h-full flex flex-col items-center justify-center p-8">
                
                {#if $callStore.type === 'video'}
                    <div class="relative w-full max-w-4xl aspect-video bg-gray-900 rounded-2xl overflow-hidden shadow-2xl border border-white/10">
                        
                        <video 
                            bind:this={remoteVideoLarge}
                            autoplay 
                            playsinline 
                            class="w-full h-full object-cover"
                        ></video>

                        
                        <div class="absolute bottom-6 right-6 w-48 aspect-video bg-black rounded-lg overflow-hidden border-2 border-white/20 shadow-xl">
                            <video 
                                bind:this={localVideoSmall}
                                autoplay 
                                playsinline 
                                muted 
                                class="w-full h-full object-cover"
                            ></video>
                        </div>
                    </div>
                {:else}
                    <div class="flex flex-col items-center space-y-6">
                        <div class="w-32 h-32 rounded-full bg-gradient-to-tr from-blue-500 to-purple-600 flex items-center justify-center text-5xl font-bold shadow-2xl animate-pulse">
                            {$callStore.peerHash?.slice(0, 1).toUpperCase()}
                        </div>
                        <div class="text-3xl font-bold">Voice Call</div>
                        <div class="text-blue-400 font-medium tracking-widest uppercase text-xs">
                            {$callStore.status === 'connected' 
                                ? `${Math.floor($callStore.duration / 60).toString().padStart(2, '0')}:${($callStore.duration % 60).toString().padStart(2, '0')}` 
                                : 'Connecting...'}
                        </div>
                    </div>
                {/if}

                
                <div class="mt-auto pb-12 flex space-x-6 items-center">
                    <button class="p-4 bg-white/10 hover:bg-white/20 rounded-full transition"><LucideMic size={24} /></button>
                    <button onclick={hangup} class="p-6 bg-red-500 hover:bg-red-600 rounded-full transition shadow-xl shadow-red-600/30">
                        <LucidePhoneOff size={32} />
                    </button>
                    <button class="p-4 bg-white/10 hover:bg-white/20 rounded-full transition"><LucideVideo size={24} /></button>
                </div>
            </div>
        {/if}
    </div>
{/if}

<style>
    @keyframes bounce-subtle {
        0%, 100% { transform: translateY(0); }
        50% { transform: translateY(-10px); }
    }
    .animate-bounce-subtle {
        animation: bounce-subtle 2s infinite ease-in-out;
    }
</style>
