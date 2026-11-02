package org.moderndaypioneer.app

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import android.view.WindowManager
import androidx.activity.result.ActivityResult
import androidx.documentfile.provider.DocumentFile
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream

@InvokeArg
class CopyInArgs {
  lateinit var uri: String
  lateinit var dest: String
}

@InvokeArg
class CopyOutArgs {
  lateinit var src: String
  lateinit var uri: String
}

@InvokeArg
class CopyTreeArgs {
  lateinit var src: String
  lateinit var tree: String
  lateinit var name: String
}

@InvokeArg
class AwakeArgs {
  var on: Boolean = false
}

@InvokeArg
class TreeArgs {
  lateinit var tree: String
}

// The phone's files (sprint 9): copying picked files in through Android's
// own streams (which work for every kind of picker address, including
// photos still in the cloud), saving files and folders out to places the
// person picks, and the system folder picker. Copies run off the screen's
// thread so big videos don't freeze the app.
@TauriPlugin
class PhoneFilesPlugin(private val activity: Activity) : Plugin(activity) {

  private fun inBackground(invoke: Invoke, work: () -> JSObject) {
    Thread {
      try {
        invoke.resolve(work())
      } catch (e: Exception) {
        invoke.reject(e.message ?: e.toString())
      }
    }.start()
  }

  // Copy a picked file into the app; returns its byte count and type.
  @Command
  fun copyIn(invoke: Invoke) {
    val args = invoke.parseArgs(CopyInArgs::class.java)
    inBackground(invoke) {
      val uri = Uri.parse(args.uri)
      val resolver = activity.contentResolver
      var name: String? = null
      try {
        resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
          if (c.moveToFirst()) name = c.getString(0)
        }
      } catch (_: Exception) {}
      val bytes = (resolver.openInputStream(uri) ?: throw Exception("Android could not open ${args.uri}")).use { input ->
        FileOutputStream(args.dest).use { input.copyTo(it, 1 shl 16) }
      }
      val r = JSObject()
      r.put("bytes", bytes)
      r.put("mime", resolver.getType(uri))
      r.put("name", name)
      r
    }
  }

  // Write a file from the app to a picked place (a backup saved with the picker).
  @Command
  fun copyOut(invoke: Invoke) {
    val args = invoke.parseArgs(CopyOutArgs::class.java)
    inBackground(invoke) {
      val out = activity.contentResolver.openOutputStream(Uri.parse(args.uri), "wt")
        ?: throw Exception("Android could not write to the place picked.")
      out.use { o -> FileInputStream(args.src).use { it.copyTo(o, 1 shl 16) } }
      JSObject()
    }
  }

  // Copy a folder from the app into a picked folder as `name`, replacing an
  // earlier copy of the same name (an export saved to a place picked).
  @Command
  fun copyTreeOut(invoke: Invoke) {
    val args = invoke.parseArgs(CopyTreeArgs::class.java)
    inBackground(invoke) {
      val root = DocumentFile.fromTreeUri(activity, Uri.parse(args.tree))
        ?: throw Exception("The folder picked can't be opened any more. Please choose it again.")
      root.findFile(args.name)?.delete()
      val dir = root.createDirectory(args.name) ?: throw Exception("A folder couldn't be made there.")
      val count = copyDir(File(args.src), dir)
      val r = JSObject()
      r.put("files", count)
      r
    }
  }

  private fun copyDir(src: File, dest: DocumentFile): Int {
    var count = 0
    for (f in src.listFiles() ?: emptyArray()) {
      if (f.isDirectory) {
        val sub = dest.createDirectory(f.name) ?: throw Exception("A folder couldn't be made for ${f.name}.")
        count += copyDir(f, sub)
      } else {
        // octet-stream keeps the file name exactly as it is.
        val doc = dest.createFile("application/octet-stream", f.name) ?: throw Exception("${f.name} couldn't be saved.")
        activity.contentResolver.openOutputStream(doc.uri, "w")!!.use { o -> FileInputStream(f).use { it.copyTo(o, 1 shl 16) } }
        count++
      }
    }
    return count
  }

  // The system folder picker; the app keeps the right to write there.
  @Command
  fun pickFolder(invoke: Invoke) {
    val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
    startActivityForResult(invoke, intent, "folderPicked")
  }

  @ActivityCallback
  fun folderPicked(invoke: Invoke, result: ActivityResult) {
    val uri = result.data?.data
    if (result.resultCode != Activity.RESULT_OK || uri == null) {
      invoke.resolve(JSObject())
      return
    }
    try {
      activity.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
    } catch (_: Exception) {}
    val r = JSObject()
    r.put("tree", uri.toString())
    r.put("name", DocumentFile.fromTreeUri(activity, uri)?.name)
    invoke.resolve(r)
  }

  // The name of a folder picked earlier, for showing instead of its address.
  @Command
  fun folderName(invoke: Invoke) {
    val args = invoke.parseArgs(TreeArgs::class.java)
    val r = JSObject()
    r.put("name", DocumentFile.fromTreeUri(activity, Uri.parse(args.tree))?.name)
    invoke.resolve(r)
  }

  // Keep the screen on while a model downloads or a transcript is made;
  // a phone that goes dark stops the app's work.
  @Command
  fun keepAwake(invoke: Invoke) {
    val args = invoke.parseArgs(AwakeArgs::class.java)
    activity.runOnUiThread {
      if (args.on) activity.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
      else activity.window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
      invoke.resolve(JSObject())
    }
  }
}
