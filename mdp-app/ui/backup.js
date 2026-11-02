// Back up and restore: the Settings
// section and the first-run "restore a backup" link. The archive is made
// and checked by mdp-core; this file starts it and shows its progress.
'use strict';

extraLoaders.settings = async () => {
  await loadSettings();
  await loadBackup();
};

let backupInfo = null;
async function loadBackup() {
  backupInfo = await invoke('backup_info');
  $('#bk-last').textContent = backupInfo.last
    ? `Last backed up ${new Date(backupInfo.last).toLocaleString()}.`
    : 'This Journey has not been backed up from this app yet.';
}

async function startBackup() {
  try {
    const info = backupInfo || await invoke('backup_info');
    // The phone's picker takes just a file name.
    const dest = await dialog.save({ title: 'Save the backup as', defaultPath: PHONE ? info.suggested.split(/[\\/]/).pop() : info.suggested,
      filters: [{ name: 'Journey backup', extensions: ['zip'] }] });
    if (!dest) return;
    await invoke('start_backup', { dest });
    watchBackup();
    return true;
  } catch (e) { showError(e); }
  return false;
}
$('#bk-start').addEventListener('click', () => startBackup());

// Upgrade safety (sprint 12): the first time a new version opens a Journey,
// offer a backup before anything changes. Asked once per version.
async function offerBackupForNewVersion() {
  const v = await invoke('version_note').catch(() => null);
  if (!v || !v.before) return;
  const ok = await modal('A new version of the app', [
    el('p', {}, `This Journey was last opened with ${/^\d/.test(v.before) ? `version ${v.before}` : v.before}; this is version ${v.now}.`),
    el('p', {}, 'Nothing has changed yet. It\'s a good moment to make a backup, so you can go back if something isn\'t right.')],
  { ok: 'Make a backup…', cancel: 'Not now' });
  if (ok) await startBackup();
}

// Restore: choose the .zip, confirm the new folder, then open the Journey.
async function restoreBackup() {
  const archive = await dialog.open({ title: 'Choose a backup to restore', filters: [{ name: 'Journey backup', extensions: ['zip'] }] });
  if (!archive) return;
  let plan;
  try { plan = await invoke('restore_plan', { archive }); } catch (e) { showError(e); return; }
  const folder = textField('Restore into this new folder', plan.folder, { hint: 'An existing Journey is never replaced; the backup is put back beside it.' });
  if (PHONE) folder.hidden = true; // the phone keeps Journeys in the app's own storage
  const made = plan.created ? new Date(plan.created).toLocaleString() : 'an unknown date';
  const ok = await modal('Restore a backup', [el('p', {}, `This restores "${plan.name}", backed up ${made}. Every file is checked before the Journey is put in place.`), folder],
    { ok: 'Restore', wide: true, check: async () => {
      if (!folder.input.value.trim()) return 'Choose a folder.';
      await invoke('start_restore', { archive: plan.archive || archive, folder: folder.input.value.trim() });
      return true;
    } });
  if (!ok) return;
  if ($('#app').hidden) { enterRestoreScreen(); }
  watchBackup();
}
$('#bk-restore').addEventListener('click', () => restoreBackup().catch(showError));
$('#restore-first').addEventListener('click', () => restoreBackup().catch((e) => { $('#create-error').textContent = String(e); $('#create-error').hidden = false; }));

// From the first-run screen, progress shows on Settings once the app opens;
// until then the first-run box shows it in its error line.
function enterRestoreScreen() {
  $('#create-error').hidden = false;
  $('#create-error').textContent = 'Restoring the backup…';
}

let backupPoll = null;
function watchBackup() {
  clearInterval(backupPoll);
  $('#bk-done').hidden = true;
  const tick = async () => {
    const s = await invoke('backup_status').catch(() => null);
    if (!s) { clearInterval(backupPoll); return; }
    const restoring = s.kind === 'restore';
    $('#bk-job').hidden = !s.running;
    if (s.running) {
      $('#bk-msg').textContent = s.message || (restoring ? 'Restoring…' : 'Backing up…');
      $('#bk-bar').max = Math.max(1, s.total);
      $('#bk-bar').value = Math.min(s.done, s.total);
      if (!$('#firstrun').hidden) $('#create-error').textContent = `Restoring the backup: ${s.done} of ${s.total} files…`;
      return;
    }
    clearInterval(backupPoll);
    await invoke('clear_backup').catch(() => {});
    if (!$('#firstrun').hidden) {
      // Restored from the first-run screen: open it straight away.
      if (s.path) { try { enterApp(await invoke('open_journey', { folder: s.path })); toast('The backup is restored and open.'); } catch (e) { $('#create-error').textContent = String(e); } }
      else $('#create-error').textContent = s.canceled ? 'The restore was canceled. Nothing was changed.' : `The backup could not be restored: ${s.error}`;
      return;
    }
    $('#bk-done').hidden = false;
    $('#bk-open').hidden = !(restoring && s.path);
    $('#bk-show').hidden = !(!restoring && s.path);
    $('#bk-open').dataset.path = s.path || '';
    $('#bk-show').dataset.path = s.path || '';
    $('#bk-done-msg').textContent = s.canceled ? `Canceled. ${restoring ? 'Nothing was restored.' : 'No backup was made.'}`
      : s.error ? `${restoring ? 'The backup could not be restored' : 'The backup could not be made'}: ${s.error}`
        : restoring ? `Restored into ${s.path}. Your current Journey is unchanged.` : `Backed up to ${s.path}. Every file is checked by its checksum when it is restored.`;
    if (!restoring) loadBackup().catch(() => {});
  };
  tick();
  backupPoll = setInterval(tick, 400);
}

$('#bk-cancel').addEventListener('click', () => { invoke('cancel_backup'); $('#bk-msg').textContent = 'Canceling…'; });
$('#bk-dismiss').addEventListener('click', () => { $('#bk-done').hidden = true; $('#bk-start').focus(); });
$('#bk-show').addEventListener('click', (e) => invoke('show_backup', { path: e.currentTarget.dataset.path }).catch(showError));
$('#bk-open').addEventListener('click', async (e) => {
  try { enterApp(await invoke('open_journey', { folder: e.currentTarget.dataset.path })); toast('The restored Journey is open.'); } catch (err) { showError(err); }
});
