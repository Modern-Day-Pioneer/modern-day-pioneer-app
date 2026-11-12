// Stands in for Tauri in a plain browser (used only by the screen checks):
// invoke goes to the core's ui_server example; dialogs answer "yes".
window.__TAURI__ = {
  core: {
    invoke: async (cmd, args) => {
      const r = await fetch(`/invoke/${cmd}`, { method: 'POST', body: JSON.stringify(args || {}) });
      if (!r.ok) throw await r.text();
      return r.json();
    },
    convertFileSrc: (path) => `/file/${encodeURIComponent(path).replace(/%2F/g, '/')}`,
  },
};
// Read-only, as Tauri's own dialog plugin defines it.
Object.defineProperty(window.__TAURI__, 'dialog', { value: { ask: async (msg) => { window.lastDialog = msg; return true; }, open: async () => window.nextOpen ?? null, save: async () => window.nextSave ?? null } });
// Drag and drop: the check calls window.drop([...paths]).
window.__TAURI__.webview = { getCurrentWebview: () => ({ onDragDropEvent: (f) => { window.drop = (paths) => f({ payload: { type: 'drop', paths } }); } }) };
