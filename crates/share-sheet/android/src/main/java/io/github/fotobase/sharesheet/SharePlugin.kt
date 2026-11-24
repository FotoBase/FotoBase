package io.github.fotobase.sharesheet

import android.app.Activity
import android.content.ClipData
import android.content.Intent
import android.net.Uri
import androidx.core.content.FileProvider
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin
import java.io.File

/** Own subclass so the manifest merger never collides with the app's FileProvider. */
class ShareFileProvider : FileProvider()

@InvokeArg
class ShareFilesArgs {
    var files: Array<String> = arrayOf()
}

@TauriPlugin
class SharePlugin(private val activity: Activity) : Plugin(activity) {
    /** Opens one chooser with every file attached (TASK-09-005). */
    @Command
    fun shareFiles(invoke: Invoke) {
        val args = invoke.parseArgs(ShareFilesArgs::class.java)
        if (args.files.isEmpty()) {
            invoke.reject("no files to share")
            return
        }
        val authority = "${activity.packageName}.fotobase.share"
        val uris = ArrayList<Uri>()
        try {
            for (path in args.files) {
                val file = File(path)
                if (!file.isFile) {
                    invoke.reject("file not found: $path")
                    return
                }
                uris.add(FileProvider.getUriForFile(activity, authority, file))
            }
        } catch (e: IllegalArgumentException) {
            // File outside the roots in res/xml/fotobase_share_paths.xml.
            invoke.reject("file cannot be shared: ${e.message}")
            return
        }

        val send = if (uris.size == 1) {
            Intent(Intent.ACTION_SEND).putExtra(Intent.EXTRA_STREAM, uris[0])
        } else {
            Intent(Intent.ACTION_SEND_MULTIPLE).putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris)
        }
        send.type = "image/jpeg"
        send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        // The read grant covers only what the ClipData names.
        val clip = ClipData.newRawUri(null, uris[0])
        for (uri in uris.drop(1)) {
            clip.addItem(ClipData.Item(uri))
        }
        send.clipData = clip

        activity.runOnUiThread {
            try {
                activity.startActivity(Intent.createChooser(send, null))
                invoke.resolve()
            } catch (e: Exception) {
                invoke.reject("share sheet failed: ${e.message}")
            }
        }
    }
}
