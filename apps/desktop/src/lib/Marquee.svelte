<script>
  import { onMount } from 'svelte';

  /**
   * One line of text that never wraps: "…" when it doesn't fit, and every few seconds it
   * scrolls slowly to the end and back, so the whole title gets read. No scrolling when the
   * system asks for reduced motion; the full text is in the tooltip either way.
   */
  let { text = '', every = 8000, speed = 45, hold = 1500 } = $props();

  let box = $state();
  let inner = $state();
  let shift = $state(0);
  let moving = $state(false);
  let ms = $state(0);

  onMount(() => {
    const reduce = globalThis.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    let timer;
    const later = (f, t) => (timer = setTimeout(f, t));
    const cycle = () => {
      const over = inner && box ? inner.scrollWidth - box.clientWidth : 0;
      if (reduce || over <= 4) return later(cycle, every);
      ms = (over / speed) * 1000;
      moving = true;
      shift = over;
      later(() => {
        ms = 400;
        shift = 0;
        later(() => {
          moving = false;
          later(cycle, every);
        }, 450);
      }, ms + hold);
    };
    later(cycle, every);
    return () => clearTimeout(timer);
  });
</script>

<div class="marquee" bind:this={box} title={text}>
  <span bind:this={inner} class:moving style:transform="translateX({-shift}px)" style:transition-duration="{ms}ms">{text}</span>
</div>

<style>
  .marquee { overflow: hidden; white-space: nowrap; min-width: 0; max-width: 100%; }
  span { display: inline-block; transition-property: transform; transition-timing-function: linear; }
  span:not(.moving) { max-width: 100%; overflow: hidden; text-overflow: ellipsis; vertical-align: bottom; }
</style>
