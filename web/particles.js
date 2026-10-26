/* A small, local canvas: slow wind, pointer ripples, no animation library. */
(() => {
  'use strict';
  const canvas = document.getElementById('ambient-particles');
  const button = document.getElementById('motion-toggle');
  if (!canvas || !button) return;
  const context = canvas.getContext('2d');
  if (!context) { canvas.remove(); button.hidden = true; return; }

  const reduced = matchMedia('(prefers-reduced-motion: reduce)');
  const colors = ['rgba(191,213,244,.48)', 'rgba(219,231,250,.30)', 'rgba(255,124,112,.50)'];
  const mouse = { x: -1000, y: -1000, active: false };
  let enabled = true;
  try { enabled = localStorage.getItem('allfiles.motion') !== '0'; } catch { /* Optional preference. */ }
  let width = 1, height = 1, points = [], bounds, frame = 0, last = 0, time = 0;
  let inView = true, departed = false, cost = 0, samples = 0;

  function draw() {
    context.clearRect(0, 0, width, height);
    // Three batches keep canvas state changes independent of particle count.
    for (let group = 0; group < colors.length; group++) {
      context.fillStyle = colors[group];
      for (const p of points) {
        if (p.color === group) context.fillRect(p.x, p.y, p.size, p.size);
      }
    }
  }

  function resize() {
    bounds = canvas.getBoundingClientRect();
    width = Math.max(1, bounds.width);
    height = Math.max(1, bounds.height);
    // A Retina/4K display must not multiply this decorative canvas unchecked.
    const scale = Math.min(devicePixelRatio || 1, 1.25, Math.sqrt(2000000 / (width * height)));
    canvas.width = Math.max(1, Math.round(width * scale));
    canvas.height = Math.max(1, Math.round(height * scale));
    context.setTransform(scale, 0, 0, scale, 0, 0);
    const count = Math.min(width < 600 ? 180 : 600, Math.max(90, Math.round(width * height / 2100)));
    points = Array.from({ length: count }, () => ({
      x: Math.random() * width, y: Math.random() * height,
      vx: 0, vy: 0, size: Math.random() < .15 ? 2.2 : 1.4,
      color: Math.random() < .04 ? 2 : Math.random() < .6 ? 0 : 1,
    }));
    cost = 0; samples = 0;
    draw();
  }

  function update(dt) {
    time += dt;
    const wind = Math.cos(time * .12) * 17;
    const phase = time * .32;
    const ease = Math.min(1, dt * 2.4);
    for (const p of points) {
      const flowX = wind + Math.sin(p.y / 110 + phase) * 12;
      const flowY = Math.sin(p.x / 190 - phase * .7) * 8;
      p.vx += (flowX - p.vx) * ease;
      p.vy += (flowY - p.vy) * ease;
      if (mouse.active) {
        const dx = p.x - mouse.x, dy = p.y - mouse.y, distance2 = dx * dx + dy * dy;
        if (distance2 < 12100 && distance2 > .1) {
          const distance = Math.sqrt(distance2);
          const force = (1 - distance / 110) ** 2 * 460 * dt / distance;
          p.vx += dx * force; p.vy += dy * force;
        }
      }
      p.x += p.vx * dt; p.y += p.vy * dt;
      if (p.x < -8) p.x = width + 8;
      else if (p.x > width + 8) p.x = -8;
      if (p.y < -8) p.y = height + 8;
      else if (p.y > height + 8) p.y = -8;
    }
  }

  function animate(now) {
    frame = 0;
    if (!canRun()) return;
    const elapsed = now - last;
    if (elapsed >= 1000 / 30) {
      const started = performance.now();
      update(Math.min((elapsed - elapsed % (1000 / 30)) / 1000, .08));
      draw();
      last = now - elapsed % (1000 / 30);
      cost += performance.now() - started;
      if (++samples === 60) {
        // Shed work on a slow device; this measures JS/draw submission, not GPU time.
        if (cost / samples > 5 && points.length > 120) points.length = Math.max(120, Math.floor(points.length * .75));
        cost = 0; samples = 0;
      }
    }
    frame = requestAnimationFrame(animate);
  }

  function canRun() { return enabled && !reduced.matches && !document.hidden && inView && !departed; }
  function reconcile() {
    const on = enabled && !reduced.matches;
    button.setAttribute('aria-pressed', String(on));
    button.disabled = reduced.matches;
    button.title = reduced.matches ? 'Animacija je isključena prema postavkama smanjenog kretanja'
      : on ? 'Isključi animaciju tačkica' : 'Pokreni animaciju tačkica';
    button.setAttribute('aria-label', button.title);
    if (!canRun()) {
      cancelAnimationFrame(frame); frame = 0; mouse.active = false;
    } else if (!frame) {
      last = performance.now(); frame = requestAnimationFrame(animate);
    }
  }

  button.addEventListener('click', () => {
    enabled = !enabled;
    try { localStorage.setItem('allfiles.motion', enabled ? '1' : '0'); } catch { /* Still works this session. */ }
    reconcile();
  });
  document.addEventListener('pointermove', event => {
    if (!canRun() || event.pointerType === 'touch') return;
    mouse.x = event.clientX - bounds.left; mouse.y = event.clientY - bounds.top;
    mouse.active = mouse.x >= 0 && mouse.y >= 0 && mouse.x <= width && mouse.y <= height;
  }, { passive: true });
  document.addEventListener('pointerout', event => { if (!event.relatedTarget) mouse.active = false; });
  window.addEventListener('blur', () => { mouse.active = false; });
  window.addEventListener('scroll', () => { bounds = canvas.getBoundingClientRect(); }, { passive: true });
  document.addEventListener('visibilitychange', reconcile);
  reduced.addEventListener('change', reconcile);
  window.addEventListener('pagehide', () => { departed = true; reconcile(); });
  window.addEventListener('pageshow', () => { departed = false; reconcile(); });
  new ResizeObserver(resize).observe(canvas.parentElement);
  new IntersectionObserver(entries => { inView = entries.at(-1).isIntersecting; reconcile(); }).observe(canvas);
  resize();
  reconcile();
})();
