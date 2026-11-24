<script>
  // Multiband gain reduction: bars fill downward (like broadcast processors), 12 dB full scale.
  let { bands } = $props();
  const NAMES = ['Bass', 'Low mids', 'High mids', 'Highs'];
  const SHORT = ['B', 'LM', 'HM', 'H'];
</script>

<div class="mb-bars" aria-label="Multiband gain reduction: bass, low mids, high mids, highs">
  {#each bands as gr, i (i)}
    <div class="mb-col" title="{NAMES[i]}: {gr.toFixed(1)} dB of compression (bar fills downward, 12 dB max)">
      <div class="mb-track"><div style:height="{Math.min(100, (-gr / 12) * 100)}%"></div></div>
      <span class="mb-label">{SHORT[i]}</span>
    </div>
  {/each}
</div>

<style>
  .mb-bars { display: flex; gap: 6px; }
  .mb-col { width: 16px; display: flex; flex-direction: column; align-items: center; gap: 3px; }
  .mb-track { width: 10px; height: 34px; background: var(--line); border-radius: 2px; position: relative; overflow: hidden; }
  .mb-track > div { position: absolute; top: 0; left: 0; right: 0; background: var(--warn); transition: height 60ms linear; }
  .mb-label { font-size: 9px; color: var(--muted); line-height: 1; }
</style>
