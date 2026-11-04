// Sprint 15: the Look (System or Scrapbook) and the Density (Compact or
// Comfortable). Both are kept on this device only (localStorage), like the
// other view choices, so nothing in the Journey or its catalog changes.
// Loaded in <head> so the chosen look is in place before the first paint.
'use strict';

const LOOK_KEY = 'mdp.look';
const DENSITY_KEY = 'mdp.density';
const LOOKS = ['system', 'scrapbook'];
const DENSITIES = ['compact', 'comfortable'];

function readPref(key, allowed) {
  let v = null;
  try { v = localStorage.getItem(key); } catch { /* private window */ }
  return allowed.includes(v) ? v : allowed[0];
}
function keepPref(key, v) {
  try { localStorage.setItem(key, v); } catch { /* not kept */ }
}

// High contrast (Windows "contrast themes") or "more contrast" always gets
// the System look, whatever the setting says: the theme must never be the
// reason something is harder to read.
const highContrast = [matchMedia('(forced-colors: active)'), matchMedia('(prefers-contrast: more)')];
const contrastOn = () => highContrast.some((m) => m.matches);

function applyLook() {
  const root = document.documentElement;
  const look = readPref(LOOK_KEY, LOOKS);
  root.dataset.look = look === 'scrapbook' && !contrastOn() ? 'scrapbook' : 'system';
  root.dataset.density = readPref(DENSITY_KEY, DENSITIES);
  const note = document.getElementById('s-look-hc');
  if (note) note.hidden = !(look === 'scrapbook' && contrastOn());
}
applyLook();
for (const m of highContrast) m.addEventListener('change', applyLook);

document.addEventListener('DOMContentLoaded', () => {
  const look = document.getElementById('s-look');
  const density = document.getElementById('s-density');
  if (!look || !density) return;
  look.value = readPref(LOOK_KEY, LOOKS);
  density.value = readPref(DENSITY_KEY, DENSITIES);
  look.addEventListener('change', () => { keepPref(LOOK_KEY, look.value); applyLook(); });
  density.addEventListener('change', () => { keepPref(DENSITY_KEY, density.value); applyLook(); });
  applyLook();
});
