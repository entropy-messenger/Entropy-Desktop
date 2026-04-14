
<script lang="ts">
  import { getAvatarData } from '../lib/utils/avatar';
  import { LucideX, LucideDownload } from 'lucide-svelte';

  let { 
    hash = "0000000000000000000000000000000000000000000000000000000000000000", 
    alias = "?", 
    size = "w-12 h-12",
    textSize = "text-lg",
    rounded = "rounded-2xl",
    clickable = true
  } = $props<{ 
    hash?: string; 
    alias?: string; 
    size?: string;
    textSize?: string;
    rounded?: string;
    clickable?: boolean;
  }>();

  // Deterministically fetch the visual style from the hash
  let s = $derived(getAvatarData(hash));
  let gradId = $derived(`grad-${hash.substring(0, 8)}`);

  let showFullSize = $state(false);

  const toggleLightbox = (e: MouseEvent) => {
    if (!clickable) return;
    e.stopPropagation();
    showFullSize = !showFullSize;
  };
</script>

<div 
  class="{size} {rounded} flex items-center justify-center text-white font-bold {textSize} shadow-2xl overflow-hidden relative group/avatar transition-all duration-300 border border-white/5 ring-1 ring-white/10 {clickable ? 'cursor-zoom-in active:scale-95' : ''}"
  onclick={toggleLightbox}
  onkeydown={(e) => e.key === 'Enter' && toggleLightbox(e as any)}
  role="button"
  tabindex="0"
>
  <div class="absolute inset-0 w-full h-full" style="background-color: {s.baseBg};"></div>

  <svg class="absolute inset-0 w-full h-full pointer-events-none transition-transform duration-1000 group-hover/avatar:scale-125" viewBox="-5 -5 110 110" preserveAspectRatio="none">
    <defs>
        <filter id="light-spec">
            <feSpecularLighting surfaceScale="2" specularConstant="1.2" specularExponent="20" lighting-color="white" result="spec">
                <feDistantLight azimuth="45" elevation="60" />
            </feSpecularLighting>
            <feComposite in="SourceGraphic" in2="spec" operator="in" />
        </filter>
        <filter id="sigil-glow">
            <feDropShadow dx="0" dy="0" stdDeviation="1.5" flood-color="white" flood-opacity="0.3" />
        </filter>
    </defs>
    
    <g filter="url(#sigil-glow)">
      {#each s.shards as shard, i}
        {#if shard.type === 'polygon'}
          <polygon 
            points={shard.points} 
            fill={shard.color} 
            opacity={shard.opacity} 
            style="mix-blend-mode: {shard.blendMode};"
            transform="rotate({shard.rotation}, 50, 50)" 
          />
        {:else if shard.type === 'arc' && shard.d}
           <g transform="translate({shard.x}, {shard.y}) rotate({shard.rotation})">
              <path 
                d={shard.d} 
                fill="none" 
                stroke={shard.color} 
                stroke-width="1.5"
                opacity={shard.opacity} 
                stroke-linecap="round"
                style="mix-blend-mode: {shard.blendMode};"
              />
           </g>
        {/if}
      {/each}
    </g>
  </svg>

  <div class="absolute inset-x-0 inset-y-0 bg-white/5 opacity-0 group-hover/avatar:opacity-100 transition-opacity backdrop-blur-[0.5px]"></div>
</div>

{#if showFullSize}
    <div class="fixed inset-0 z-[9999] flex items-center justify-center bg-black/95 backdrop-blur-3xl animate-in fade-in duration-500 px-6" onclick={toggleLightbox} onkeydown={(e) => e.key === 'Escape' && (showFullSize = false)} role="button" tabindex="0">
        <div class="absolute top-8 right-8 flex space-x-6 z-[10000]">
            <button class="text-white/40 hover:text-white transition active:scale-95"><LucideDownload size={32} /></button>
            <button onclick={toggleLightbox} class="text-white/40 hover:text-white transition active:scale-95"><LucideX size={36} /></button>
        </div>
        
        <div class="relative group/modal max-w-full flex flex-col items-center" onclick={(e) => e.stopPropagation()} role="presentation">
            <div class="w-[85vw] h-[85vw] sm:w-[540px] sm:h-[540px] rounded-[50px] overflow-hidden shadow-[0_0_150px_rgba(0,0,0,1)] border border-white/5 relative">
                 <div class="absolute inset-0" style="background-color: {s.baseBg};"></div>
                 <svg class="absolute inset-0 w-full h-full p-12" viewBox="-5 -5 110 110">
                    <g filter="url(#light-spec)">
                        {#each s.shards as shard, i}
                            {#if shard.type === 'polygon'}
                                <polygon 
                                    points={shard.points} 
                                    fill={shard.color} 
                                    opacity={shard.opacity} 
                                    style="mix-blend-mode: {shard.blendMode};"
                                    transform="rotate({shard.rotation}, 50, 50)" 
                                />
                            {:else if shard.type === 'arc' && shard.d}
                                <g transform="translate({shard.x}, {shard.y}) rotate({shard.rotation})">
                                    <path 
                                        d={shard.d} 
                                        fill="none" 
                                        stroke={shard.color} 
                                        stroke-width="1.2"
                                        opacity={shard.opacity} 
                                        stroke-linecap="round"
                                        style="mix-blend-mode: {shard.blendMode};"
                                    />
                                </g>
                            {/if}
                        {/each}
                    </g>
                 </svg>
                 <div class="absolute inset-0 bg-white/5 pointer-events-none mix-blend-overlay"></div>
            </div>
            
            <div class="mt-12 text-center animate-in slide-in-from-bottom-8 duration-700">
                <h3 class="text-white font-black text-4xl tracking-tight pr-2 italic opacity-95">{alias}</h3>
                <div class="flex items-center justify-center space-x-2 mt-4">
                    <span class="text-[9px] font-black uppercase tracking-[0.4em] text-white/20 border-y border-white/5 py-1">Identity: {hash.slice(0, 24)}...</span>
                </div>
            </div>
        </div>
    </div>
{/if}
