<script>
  // Multiband gain reduction: bars fill downward (like broadcast processors), 12 dB full scale.
  // `part`: 'both' (bars with labels under them), or 'bars' / 'labels' alone so a parent
  // grid can line the bars up with its meters and the labels with its text row.
  import { t } from './i18n.svelte.js';
  let { bands, part = 'both' } = $props();
  const NAMES = ['band.bass', 'band.lowMids', 'band.highMids', 'band.highs'];
  const SHORT = ['band.bassShort', 'band.lowMidsShort', 'band.highMidsShort', 'band.highsShort'];
</script>

{#if part === 'labels'}
  <div class="row labels" aria-hidden="true">
    {#each SHORT as s (s)}<span class="mb-label">{t(s)}</span>{/each}
  </div>
{:else}
  <div class="row" class:stretch={part === 'bars'} aria-label={t('band.aria')}>
    {#each bands as gr, i (i)}
      <div class="mb-col" title={t('band.tooltip', { band: t(NAMES[i]), db: gr.toFixed(1) })}>
        <div class="mb-track"><div style:height="{Math.min(100, (-gr / 12) * 100)}%"></div></div>
        {#if part === 'both'}<span class="mb-label">{t(SHORT[i])}</span>{/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .row { display: flex; gap: 6px; }
  /* Wide enough for two-character labels in any script (e.g. 中低, НС). */
  .mb-col { width: 20px; display: flex; flex-direction: column; align-items: center; gap: 3px; }
  .mb-track { width: 10px; height: 34px; background: var(--line); border-radius: 2px; position: relative; overflow: hidden; }
  .mb-track > div { position: absolute; top: 0; left: 0; right: 0; background: var(--warn); transition: height 60ms linear; }
  .mb-label { width: 20px; text-align: center; font-size: 9px; color: var(--muted); line-height: 1; white-space: nowrap; }
  .stretch { height: 100%; }
  .stretch .mb-col { height: 100%; }
  .stretch .mb-track { height: 100%; }
  .labels .mb-label { font-size: 10px; line-height: inherit; }
</style>
