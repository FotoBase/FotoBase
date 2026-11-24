package io.github.fotobase.cameraroll

import android.Manifest
import android.app.Activity
import android.database.ContentObserver
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.provider.MediaStore
import app.tauri.PermissionState
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File

@InvokeArg
class ObserveArgs {
    lateinit var onChange: Channel
}

/** Android 13 split read access per media type; older versions use one permission. */
@TauriPlugin(
    permissions = [
        Permission(
            strings = [Manifest.permission.READ_MEDIA_IMAGES, Manifest.permission.READ_MEDIA_VIDEO],
            alias = "photos"
        ),
        Permission(strings = [Manifest.permission.READ_EXTERNAL_STORAGE], alias = "photosLegacy"),
    ]
)
class CameraRollPlugin(private val activity: Activity) : Plugin(activity) {
    private val main = Handler(Looper.getMainLooper())
    private var observer: ContentObserver? = null
    private var pending: Runnable? = null

    private val alias: String
        get() = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) "photos" else "photosLegacy"

    private fun granted(): Boolean = getPermissionStates()[alias] == PermissionState.GRANTED

    @Command
    fun requestAccess(invoke: Invoke) {
        if (granted()) {
            resolveAccess(invoke)
        } else {
            requestPermissionForAliases(arrayOf(alias), invoke, "accessResult")
        }
    }

    @PermissionCallback
    fun accessResult(invoke: Invoke) {
        resolveAccess(invoke)
    }

    private fun resolveAccess(invoke: Invoke) {
        val reply = JSObject()
        reply.put("state", if (granted()) "granted" else "denied")
        invoke.resolve(reply)
    }

    /**
     * Registers MediaStore change observers (event-driven: no timer, no
     * polling) and resolves with every camera roll folder. Each change is
     * coalesced for a moment, then the folders are sent on the channel.
     */
    @Command
    fun startObserving(invoke: Invoke) {
        val args = invoke.parseArgs(ObserveArgs::class.java)
        if (!granted()) {
            invoke.reject("photo library access not granted")
            return
        }
        stop()
        val channel = args.onChange
        val obs = object : ContentObserver(main) {
            override fun onChange(selfChange: Boolean, uri: Uri?) {
                // A burst (a camera writing a shot and its thumbnail) is one change.
                pending?.let { main.removeCallbacks(it) }
                val run = Runnable { Thread { channel.send(foldersReply()) }.start() }
                pending = run
                main.postDelayed(run, COALESCE_MS)
            }
        }
        for (collection in COLLECTIONS) {
            activity.contentResolver.registerContentObserver(collection, true, obs)
        }
        observer = obs
        invoke.resolve(foldersReply())
    }

    @Command
    fun stopObserving(invoke: Invoke) {
        stop()
        invoke.resolve()
    }

    private fun stop() {
        pending?.let { main.removeCallbacks(it) }
        pending = null
        observer?.let { activity.contentResolver.unregisterContentObserver(it) }
        observer = null
    }

    private fun foldersReply(): JSObject {
        val folders = JSArray()
        for (dir in cameraRollFolders()) {
            folders.put(dir)
        }
        val reply = JSObject()
        reply.put("folders", folders)
        return reply
    }

    /** Folders under DCIM and Pictures that hold at least one indexed image or video. */
    private fun cameraRollFolders(): Set<String> {
        val roots = listOf(Environment.DIRECTORY_DCIM, Environment.DIRECTORY_PICTURES)
            .map { Environment.getExternalStoragePublicDirectory(it).absolutePath + File.separator }
        val folders = sortedSetOf<String>()
        for (collection in COLLECTIONS) {
            activity.contentResolver.query(
                collection, arrayOf(MediaStore.MediaColumns.DATA), null, null, null
            )?.use { cursor ->
                while (cursor.moveToNext()) {
                    val parent = File(cursor.getString(0) ?: continue).parent ?: continue
                    val inRoll = roots.any { (parent + File.separator).startsWith(it) }
                    // Hidden folders hold thumbnails and trash, not photos.
                    if (inRoll && !parent.contains("/.")) {
                        folders.add(parent)
                    }
                }
            }
        }
        return folders
    }

    companion object {
        private const val COALESCE_MS = 500L
        private val COLLECTIONS = listOf(
            MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
            MediaStore.Video.Media.EXTERNAL_CONTENT_URI,
        )
    }
}
