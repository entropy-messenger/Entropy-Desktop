
<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { LucideX, LucideCheck, LucideRefreshCcw, LucideRotateCw, LucidePlus, LucideMinus, LucideMove, LucideZoomIn, LucideZoomOut } from 'lucide-svelte';
  import { fly, fade } from 'svelte/transition';

  let { image, onSave, onCancel } = $props<{ 
    image: string, 
    onSave: (cropped: string) => void, 
    onCancel: () => void 
  }>();

  let canvas = $state<HTMLCanvasElement | null>(null);
  let imgElement = $state<HTMLImageElement | null>(null);

  let zoom = $state(1);
  let rotation = $state(0);
  let position = $state({ x: 0, y: 0 });
  let isDragging = $state(false);
  let startPos = $state({ x: 0, y: 0 });

  const DISPLAY_SIZE = 320; 
  const OUTPUT_SIZE = 512; 

  onMount(() => {
    const img = new Image();
    img.src = image;
    img.onload = () => {
      imgElement = img;
      const minDim = Math.min(img.width, img.height);
      if (minDim > DISPLAY_SIZE) {
        zoom = DISPLAY_SIZE / minDim;
      } else {
        zoom = 1;
      }
      position = { x: 0, y: 0 };
    };
  });

  function reset() {
    if (!imgElement) return;
    const minDim = Math.min(imgElement.width, imgElement.height);
    if (minDim > DISPLAY_SIZE) {
      zoom = DISPLAY_SIZE / minDim;
    } else {
      zoom = 1;
    }
    rotation = 0;
    position = { x: 0, y: 0 };
  }

  function handleMouseDown(e: MouseEvent) {
    isDragging = true;
    startPos = { x: e.clientX - position.x, y: e.clientY - position.y };
  }

  function handleMouseMove(e: MouseEvent) {
    if (!isDragging) return;
    position = {
      x: e.clientX - startPos.x,
      y: e.clientY - startPos.y
    };
  }

  function handleMouseUp() {
    isDragging = false;
  }

  function handleWheel(e: WheelEvent) {
    e.preventDefault();
    const delta = e.deltaY > 0 ? 0.95 : 1.05;
    zoom = Math.max(0.01, Math.min(10, zoom * delta));
  }

  function save() {
    if (!canvas || !imgElement) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    canvas.width = OUTPUT_SIZE;
    canvas.height = OUTPUT_SIZE;

    const factor = OUTPUT_SIZE / DISPLAY_SIZE;

    ctx.clearRect(0, 0, OUTPUT_SIZE, OUTPUT_SIZE);
    
    // Background
    ctx.fillStyle = "#121216";
    ctx.fillRect(0, 0, OUTPUT_SIZE, OUTPUT_SIZE);

    ctx.save();
    
    ctx.translate(OUTPUT_SIZE / 2, OUTPUT_SIZE / 2);
    ctx.translate(position.x * factor, position.y * factor);
    ctx.rotate((rotation * Math.PI) / 180);
    ctx.scale(zoom * factor, zoom * factor);
    
    // Draw centered
    ctx.drawImage(imgElement, -imgElement.width / 2, -imgElement.height / 2);
    
    ctx.restore();
    
    onSave(canvas.toDataURL('image/jpeg', 0.95));
  }
</script>

<div class="fixed inset-0 z-[100] flex items-center justify-center bg-black/80 backdrop-blur-md p-4" transition:fade>
  <div class="bg-[#121216] w-full max-w-sm rounded-[2.5rem] overflow-hidden shadow-[0_32px_64px_-12px_rgba(0,0,0,0.8)] flex flex-col border border-white/10" transition:fly={{ y: 30, duration: 400 }}>
    
    <!-- Header -->
    <div class="p-6 flex justify-between items-center border-b border-white/5 bg-white/[0.02]">
      <div>
        <h2 class="text-white text-lg font-black uppercase tracking-[0.2em] leading-tight">Edit Photo</h2>
        <p class="text-[9px] font-bold text-entropy-primary uppercase tracking-[0.1em] mt-1 opacity-70">Adjust your look</p>
      </div>
      <button onclick={onCancel} class="p-2 hover:bg-white/5 rounded-2xl text-white/40 transition active:scale-90"><LucideX size={20} /></button>
    </div>

    <!-- Main Workspace -->
    <div class="p-8 flex flex-col items-center">
      <div 
        class="relative w-[320px] h-[320px] rounded-full overflow-hidden bg-black/40 shadow-[inset_0_4px_12px_rgba(0,0,0,0.5)] cursor-move select-none group border border-white/10"
        onmousedown={handleMouseDown}
        onwheel={handleWheel}
        role="presentation"
      >
        {#if imgElement}
          <img 
            src={image} 
            alt="To crop" 
            class="absolute pointer-events-none transition-none"
            style="
              transform: translate({position.x}px, {position.y}px) rotate({rotation}deg) scale({zoom});
              left: 50%;
              top: 50%;
              width: initial;
              height: initial;
              margin-left: -{imgElement.width / 2}px;
              margin-top: -{imgElement.height / 2}px;
            "
          />
        {/if}
        
        <!-- Guide Overlay -->
        <div class="absolute inset-0 rounded-full shadow-[0_0_0_1000px_rgba(0,0,0,0.4)] pointer-events-none z-10"></div>
        <div class="absolute inset-0 rounded-full border border-white/5 pointer-events-none z-10"></div>
      </div>

      <!-- Controls UI -->
      <div class="mt-8 w-full space-y-6">
        <div class="flex items-center space-x-4 px-2">
          <LucideMinus size={14} class="text-white/30" />
          <input 
            type="range" 
            min="0.01" 
            max="5" 
            step="0.001" 
            bind:value={zoom}
            class="flex-1 accent-entropy-primary h-1 bg-white/10 rounded-full appearance-none cursor-pointer"
          />
          <LucidePlus size={14} class="text-white/30" />
        </div>

        <div class="flex items-center justify-center space-x-3">
          <button onclick={() => rotation = (rotation + 90) % 360} class="flex-1 py-3.5 bg-white/[0.03] hover:bg-white/10 rounded-2xl text-white transition flex items-center justify-center space-x-2 border border-white/5 shadow-sm active:scale-95">
            <LucideRotateCw size={16} />
            <span class="text-[10px] font-black uppercase tracking-widest">Rotate</span>
          </button>
          <button onclick={reset} class="p-3.5 bg-white/[0.03] hover:bg-white/10 rounded-2xl text-white/40 transition border border-white/5 active:scale-95" title="Reset">
            <LucideRefreshCcw size={16} />
          </button>
        </div>
      </div>
    </div>

    <!-- Actions Footer -->
    <div class="p-6 bg-white/[0.02] border-t border-white/5 flex space-x-3">
      <button onclick={onCancel} class="px-6 py-4 text-white/40 font-black text-[10px] uppercase tracking-widest hover:bg-white/5 rounded-2xl transition">Discard</button>
      <button onclick={save} class="flex-1 py-4 bg-entropy-primary text-white rounded-2xl font-black text-[10px] uppercase tracking-[0.2em] shadow-[0_8px_24px_rgba(139,92,246,0.3)] hover:bg-entropy-primary-dim transition active:scale-[0.98] flex items-center justify-center space-x-2">
        <LucideCheck size={16} />
        <span>Save Photo</span>
      </button>
    </div>
  </div>
</div>

<canvas bind:this={canvas} class="hidden"></canvas>

<svelte:window onmousemove={handleMouseMove} onmouseup={handleMouseUp} />

<style>
  input[type='range']::-webkit-slider-thumb {
    appearance: none;
    width: 16px;
    height: 16px;
    background: #fff;
    border-radius: 50%;
    cursor: grab;
    border: 3px solid var(--entropy-primary);
    box-shadow: 0 0 10px rgba(0,0,0,0.5);
  }
</style>
