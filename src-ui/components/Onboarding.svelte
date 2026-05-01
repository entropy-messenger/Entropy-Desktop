<script lang="ts">
  import { userStore } from '../lib/stores/user';
  import { LucideFingerprint, LucideActivity, LucideGhost, LucideArrowRight, LucideCheckCircle2 } from 'lucide-svelte';
  import { fade, fly, scale } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';

  let { onComplete, isMobile = false } = $props<{ onComplete: () => void, isMobile?: boolean }>();

  let step = $state(0);
  
  const steps = [
    {
      title: "Sovereign Identity",
      description: "Entropy generates a unique Signal Protocol identity. Your private keys are stored only in your encrypted local vault, ensuring absolute sovereignty over your communication.",
      icon: LucideFingerprint,
      color: "text-entropy-primary",
      bg: "bg-entropy-primary/10"
    },
    {
      title: "Network Mining",
      description: "To connect and prevent relay spam, your device performs a Proof-of-Work challenge (mining). This secures your presence without needing an account or phone number.",
      icon: LucideActivity,
      color: "text-yellow-500",
      bg: "bg-yellow-500/10"
    },
    {
      title: "Traffic Obfuscation",
      description: "Communications are padded to 1024 bytes and interleaved with random dummy traffic. This defeats traffic analysis, metadata harvesting, and keeps your communication patterns invisible.",
      icon: LucideGhost,
      color: "text-entropy-accent",
      bg: "bg-entropy-accent/10"
    }
  ];

  const next = () => {
    if (step < steps.length - 1) {
      step++;
    } else {
      onComplete();
    }
  };
</script>

<div class="fixed inset-0 z-[100] flex items-center justify-center bg-entropy-bg/90 backdrop-blur-2xl" transition:fade>
    <div class="max-w-xl w-full mx-4 relative">
        {#each steps as s, i}
            {#if step === i}
                <div 
                    class="bg-entropy-surface border border-white/5 rounded-[2.5rem] lg:rounded-[3rem] p-8 lg:p-12 shadow-2xl space-y-8 lg:space-y-10 text-center relative overflow-hidden"
                    in:fly={{ y: 20, duration: 600, delay: 200, easing: cubicOut }}
                    out:fly={{ y: -20, duration: 400, easing: cubicOut }}
                >
                    <!-- Background Glow -->
                    <div class="absolute -top-24 -right-24 w-64 h-64 {s.bg} blur-[100px] rounded-full opacity-50"></div>
                    
                    <div class="relative items-center justify-center flex py-6 lg:py-8">
                        <div class="absolute inset-0 {s.bg} blur-3xl rounded-full opacity-30 scale-75"></div>
                        <div class="w-24 h-24 lg:w-32 lg:h-32 relative flex items-center justify-center">
                            <div class="absolute inset-0 bg-white/5 backdrop-blur-md rounded-[2rem] lg:rounded-[2.5rem] border border-white/10 rotate-6 transition-transform group-hover:rotate-0 duration-700"></div>
                            <div class="absolute inset-0 {s.bg} rounded-[2rem] lg:rounded-[2.5rem] -rotate-6 transition-transform group-hover:rotate-3 duration-1000"></div>
                            <div class="relative z-10 flex items-center justify-center w-20 h-20 lg:w-24 lg:h-24 bg-entropy-surface rounded-[1.5rem] lg:rounded-[2rem] shadow-xl border border-white/5">
                                 <s.icon size={isMobile ? 36 : 48} class={s.color} strokeWidth={1.5} />
                            </div>
                        </div>
                    </div>
 
                    <div class="space-y-3 lg:space-y-4 relative">
                        <h2 class="text-3xl lg:text-4xl font-black text-entropy-text-primary tracking-tight leading-tight">{s.title}</h2>
                        <p class="text-entropy-text-secondary text-base lg:text-lg font-medium leading-relaxed opacity-80">
                            {s.description}
                        </p>
                    </div>

                    <div class="flex flex-col space-y-6 relative pt-4">
                        <button 
                            onclick={next}
                            class="w-full py-6 bg-white text-entropy-bg rounded-[2rem] font-black text-sm uppercase tracking-[0.2em] shadow-xl hover:bg-gray-100 transition-all active:scale-[0.98] flex items-center justify-center space-x-3 group"
                        >
                            <span>{step === steps.length - 1 ? 'Start Chatting' : 'Learn More'}</span>
                            {#if step === steps.length - 1}
                                <LucideCheckCircle2 size={20} />
                            {:else}
                                <LucideArrowRight size={20} class="group-hover:translate-x-1 transition-transform" />
                            {/if}
                        </button>

                        <div class="flex justify-center space-x-3">
                            {#each steps as _, dotIdx}
                                <div class="h-1.5 rounded-full transition-all duration-500 {step === dotIdx ? 'w-8 bg-entropy-primary' : 'w-1.5 bg-entropy-border'}"></div>
                            {/each}
                        </div>
                    </div>
                </div>
            {/if}
        {/each}
    </div>
</div>
