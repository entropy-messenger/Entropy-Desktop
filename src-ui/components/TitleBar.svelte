
<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { LucideMinus, LucideX, LucideSquare } from 'lucide-svelte';

  const appWindow = getCurrentWindow();

  const handleMinimize = () => appWindow.minimize();
  const handleMaximize = () => appWindow.toggleMaximize();
  const handleClose = () => appWindow.hide(); 
</script>

<div data-tauri-drag-region class="h-9 bg-entropy-surface/95 backdrop-blur-md flex items-center justify-between px-3 select-none shrink-0 z-[100]">
    <div class="flex items-center space-x-2 pointer-events-none" data-tauri-drag-region>
        <img src="/logo.png" alt="logo" class="w-6 h-6 object-contain -ml-0.5" />
    </div>

    <div class="flex items-center h-full">
        <button 
            onclick={handleMinimize}
            class="w-11 h-9 flex items-center justify-center hover:bg-entropy-surface-light text-entropy-text-dim hover:text-entropy-text-primary transition-all active:scale-95"
        >
            <LucideMinus size={16} strokeWidth={2.5} />
        </button>
        <button 
            onclick={handleMaximize}
            class="w-11 h-9 flex items-center justify-center hover:bg-entropy-surface-light text-entropy-text-dim hover:text-entropy-text-primary transition-all active:scale-95"
        >
            <LucideSquare size={12} strokeWidth={2.5} />
        </button>
        <button 
            onclick={handleClose}
            class="w-11 h-9 flex items-center justify-center hover:bg-red-500 hover:text-white text-entropy-text-dim transition-all active:scale-95"
        >
            <LucideX size={16} strokeWidth={2.5} />
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
