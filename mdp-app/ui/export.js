// Sprint 6: preview and export.
// The pages are made by mdp_core::export; this screen starts them and shows progress.

let exportPolling = null;

extraLoaders.preview = async () => {
  const info = await invoke('export_info');
  $('#pv-note').textContent = info.note;
  $('#ex-folder').textContent = info.folder;
  $('#ex-open').hidden = !info.exported;
  const s = await invoke('export_status').catch(() => null);
  if (s && s.running) watchExport();
};

// The phone shows the preview in a frame over the screen. Its address keeps
// the folders as folders, so the pages' own links between them still work
// (served by the app's media server, so its videos play too).
function showPhonePreview(front) {
  $('#phone-preview-frame').src = fileUrl(front);
  $('#phone-preview').hidden = false;
  history.pushState({ preview: true }, '');
}
function closePhonePreview() {
  $('#phone-preview').hidden = true;
  $('#phone-preview-frame').src = 'about:blank';
}
$('#phone-preview-close').addEventListener('click', () => history.back());
window.addEventListener('popstate', () => { if (!$('#phone-preview').hidden) closePhonePreview(); });

async function openPreview(browser) {
  const b = browser ? $('#pv-browser') : $('#pv-open');
  b.disabled = true;
  const label = b.textContent;
  b.textContent = 'Making the preview…';
  try {
    if (PHONE) {
      const p = await invoke('phone_preview');
      $('#pv-note').textContent = p.note;
      showPhonePreview(p.front);
    } else {
      const note = await invoke('open_preview', { browser });
      $('#pv-note').textContent = note;
    }
  } catch (e) { showError(e); }
  b.textContent = label;
  b.disabled = false;
}

$('#pv-open').addEventListener('click', () => openPreview(false));
$('#pv-browser').addEventListener('click', () => openPreview(true));

$('#ex-start').addEventListener('click', async () => {
  try {
    $('#ex-done').hidden = true;
    await invoke('start_export');
    watchExport();
  } catch (e) { showError(e); }
});

function watchExport() {
  clearInterval(exportPolling);
  $('#ex-start').disabled = true;
  const tick = async () => {
    const s = await invoke('export_status').catch(() => null);
    if (!s) { clearInterval(exportPolling); $('#ex-start').disabled = false; return; }
    $('#ex-job').hidden = !s.running;
    if (s.running) {
      $('#ex-msg').textContent = s.message || 'Starting…';
      $('#ex-bar').max = Math.max(1, s.total);
      $('#ex-bar').value = Math.min(s.done, s.total);
      return;
    }
    clearInterval(exportPolling);
    $('#ex-start').disabled = false;
    showExportResult(s);
    await invoke('clear_export').catch(() => {});
  };
  tick();
  exportPolling = setInterval(tick, 400);
}

function showExportResult(s) {
  $('#ex-done').hidden = false;
  const ok = !!s.report;
  $('#ex-open').hidden = !ok;
  $('#ex-show').hidden = !ok;
  if (s.canceled) $('#ex-done-msg').textContent = 'Canceled. The previous export, if there was one, is unchanged.';
  else if (s.error) $('#ex-done-msg').textContent = `The history could not be exported: ${s.error} The previous export, if there was one, is unchanged.`;
  else {
    const r = s.report;
    const parts = [`Exported ${plural(r.pages, 'page', 'pages')} and ${plural(r.files, 'file', 'files')} to ${r.folder}.`, r.note];
    if (r.untagged.length) parts.push(`${plural(r.untagged.length, 'file was', 'files were')} copied without the facts written inside: ${r.untagged.join(', ')}.`);
    // A FAT32 USB stick can't hold a file of 4 GB or more.
    if (r.too_big_for_fat32?.length) parts.push(`${plural(r.too_big_for_fat32.length, 'file is', 'files are')} 4 GB or larger, so ${r.too_big_for_fat32.length === 1 ? 'it' : 'they'} will not fit on a USB stick formatted as FAT32 (use exFAT or NTFS): ${r.too_big_for_fat32.join(', ')}.`);
    $('#ex-done-msg').textContent = parts.join(' ');
  }
  $('#ex-skipped').hidden = s.skipped.length === 0;
  $('#ex-skipped-sum').textContent = `${plural(s.skipped.length, 'file was', 'files were')} left out`;
  $('#ex-skipped-list').replaceChildren(...s.skipped.map((k) => el('li', {}, el('b', {}, k.item), ` ${k.reason}`)));
  (ok ? $('#ex-open') : $('#ex-dismiss')).focus();
}

$('#ex-cancel').addEventListener('click', () => { invoke('cancel_export'); $('#ex-msg').textContent = 'Canceling…'; });
$('#ex-open').addEventListener('click', () => invoke('open_export').catch(showError));
$('#ex-show').addEventListener('click', () => invoke('show_export_folder').catch(showError));
$('#ex-dismiss').addEventListener('click', () => { $('#ex-done').hidden = true; $('#ex-start').focus(); });
