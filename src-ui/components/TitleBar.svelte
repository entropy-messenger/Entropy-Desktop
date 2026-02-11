
<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { LucideMinus, LucideX, LucideSquare } from 'lucide-svelte';

  const appWindow = getCurrentWindow();

  const handleMinimize = () => appWindow.minimize();
  const handleMaximize = () => appWindow.toggleMaximize();
  const handleClose = () => appWindow.hide(); 
</script>

<div data-tauri-drag-region class="h-12 bg-entropy-surface/95 backdrop-blur-md flex items-center justify-between px-4 select-none shrink-0 z-[100]">
    <div class="flex items-center space-x-3 pointer-events-none" data-tauri-drag-region>
        <img src="/logo.png" alt="logo" class="w-10 h-10 object-contain -ml-1" />
        <div class="flex flex-col -space-y-0.5">
            <span class="text-[14px] font-black text-entropy-text-primary tracking-tight">Entropy</span>
            <span class="text-[9px] font-bold text-entropy-primary uppercase tracking-wide opacity-80">Absolute Security</span>
        </div>
    </div>

    <div class="flex items-center h-full">
        <button 
            onclick={handleMinimize}
            class="w-10 h-8 flex items-center justify-center hover:bg-entropy-surface-light rounded-lg text-entropy-text-dim hover:text-entropy-text-primary transition-all active:scale-95"
        >
            <LucideMinus size={14} strokeWidth={3} />
        </button>
        <button 
            onclick={handleMaximize}
            class="w-10 h-8 flex items-center justify-center hover:bg-entropy-surface-light rounded-lg text-entropy-text-dim hover:text-entropy-text-primary transition-all active:scale-95"
        >
            <LucideSquare size={11} strokeWidth={3} />
        </button>
        <button 
            onclick={handleClose}
            class="w-10 h-8 flex items-center justify-center hover:bg-red-500 hover:text-white rounded-lg text-entropy-text-dim transition-all active:scale-95"
        >
            <LucideX size={14} strokeWidth={3} />
        </button>
    </div>
</div>

<style>
    [data-tauri-drag-region] {
        cursor: default;
    }
    button {
        -webkit-app-region: no-drag;
    }
</style>
