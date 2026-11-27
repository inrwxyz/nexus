package com.inex.nexuscompanion

import android.accessibilityservice.AccessibilityService
import android.app.Activity
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.ComponentName
import android.content.ContentUris
import android.content.Context
import android.content.Intent
import android.database.ContentObserver
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.MediaStore
import android.provider.Settings
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import android.util.Patterns
import android.view.accessibility.AccessibilityEvent
import android.widget.Toast
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import java.io.ByteArrayOutputStream
import java.security.MessageDigest
import java.util.Locale

/**
 * Shared clipboard & screenshot encoder + deduplicating uploader.
 */
object ClipboardSyncHelper {
    private const val MAX_IMAGE_DIM = 2200
    @Volatile private var lastHash: String = ""
    @Volatile private var lastScreenshotId: Long = -1L

    private fun sha256(bytes: ByteArray): String =
        MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }

    private fun looksLikeLink(text: String): Boolean {
        val t = text.trim()
        return t.startsWith("http://", ignoreCase = true) ||
            t.startsWith("https://", ignoreCase = true) ||
            (!t.contains('\n') && Patterns.WEB_URL.matcher(t).matches())
    }

    private fun encodeUriAsPng(ctx: Context, uri: Uri): ByteArray? {
        return try {
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            ctx.contentResolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
            if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null

            var sample = 1
            val maxSide = maxOf(bounds.outWidth, bounds.outHeight)
            while (maxSide / sample > MAX_IMAGE_DIM) sample *= 2

            val opts = BitmapFactory.Options().apply { inSampleSize = sample }
            val bmp = ctx.contentResolver.openInputStream(uri)?.use {
                BitmapFactory.decodeStream(it, null, opts)
            } ?: return null

            val scaled = if (maxOf(bmp.width, bmp.height) > MAX_IMAGE_DIM) {
                val ratio = MAX_IMAGE_DIM.toFloat() / maxOf(bmp.width, bmp.height).toFloat()
                Bitmap.createScaledBitmap(
                    bmp,
                    (bmp.width * ratio).toInt().coerceAtLeast(1),
                    (bmp.height * ratio).toInt().coerceAtLeast(1),
                    true,
                ).also { if (it !== bmp) bmp.recycle() }
            } else {
                bmp
            }

            ByteArrayOutputStream().use { out ->
                scaled.compress(Bitmap.CompressFormat.PNG, 95, out)
                scaled.recycle()
                out.toByteArray()
            }
        } catch (_: Exception) {
            null
        }
    }

    /**
     * Reads current Android clipboard (text, links, or image URI) and syncs it to PC if changed.
     * Returns true when a new clipboard item was synced.
     */
    suspend fun syncClipboardNow(ctx: Context, store: Store, net: Net, force: Boolean = false): Boolean {
        if (!store.isPaired || !store.clipboardSync) return false
        net.configure(store.ip, store.port, store.token, store.udpPort)

        val cm = ctx.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return false
        val clip = try { cm.primaryClip } catch (_: Exception) { null } ?: return false
        if (clip.itemCount == 0) return false

        val desc = clip.description
        val item = clip.getItemAt(0)

        // 1. Copied image in clipboard
        val uri = item.uri
        if (uri != null && (desc.hasMimeType("image/*") || ctx.contentResolver.getType(uri)?.startsWith("image/") == true)) {
            val png = encodeUriAsPng(ctx, uri)
            if (png != null && png.isNotEmpty()) {
                val hash = "img:" + sha256(png)
                if (!force && hash == lastHash) return false
                if (net.pushClipboardPng(png)) {
                    lastHash = hash
                    return true
                }
            }
            return false
        }

        // 2. Copied text or link
        val rawText = when {
            item.text != null -> item.text.toString()
            uri != null && (uri.scheme == "http" || uri.scheme == "https") -> uri.toString()
            desc.hasMimeType(ClipDescription.MIMETYPE_TEXT_PLAIN) ||
                desc.hasMimeType(ClipDescription.MIMETYPE_TEXT_HTML) -> item.coerceToText(ctx)?.toString()
            else -> null
        } ?: return false

        if (rawText.isBlank()) return false
        val hash = "txt:" + sha256(rawText.toByteArray(Charsets.UTF_8))
        if (!force && hash == lastHash) return false

        if (net.pushClipboardText(rawText, looksLikeLink(rawText))) {
            lastHash = hash
            return true
        }
        return false
    }

    /** Checks MediaStore for the newest completed screenshot since `sinceEpochSec`. */
    suspend fun syncLatestScreenshotSince(ctx: Context, store: Store, net: Net, sinceEpochSec: Long): Boolean {
        if (!store.isPaired || !store.clipboardSync) return false
        net.configure(store.ip, store.port, store.token, store.udpPort)

        val cols = mutableListOf(
            MediaStore.Images.Media._ID,
            MediaStore.Images.Media.DISPLAY_NAME,
            MediaStore.Images.Media.DATE_ADDED,
        )
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            cols += MediaStore.Images.Media.RELATIVE_PATH
            cols += MediaStore.Images.Media.IS_PENDING
        }

        val uriFound: Uri? = try {
            ctx.contentResolver.query(
                MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
                cols.toTypedArray(),
                "${MediaStore.Images.Media.DATE_ADDED} >= ?",
                arrayOf(sinceEpochSec.toString()),
                "${MediaStore.Images.Media._ID} DESC",
            )?.use { c ->
                val idIdx = c.getColumnIndexOrThrow(MediaStore.Images.Media._ID)
                val nameIdx = c.getColumnIndexOrThrow(MediaStore.Images.Media.DISPLAY_NAME)
                val relIdx = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    c.getColumnIndex(MediaStore.Images.Media.RELATIVE_PATH)
                } else -1
                val pendIdx = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    c.getColumnIndex(MediaStore.Images.Media.IS_PENDING)
                } else -1

                var picked: Uri? = null
                while (c.moveToNext()) {
                    val id = c.getLong(idIdx)
                    if (id == lastScreenshotId) break
                    val pending = if (pendIdx >= 0) c.getInt(pendIdx) != 0 else false
                    if (pending) continue
                    val name = (c.getString(nameIdx) ?: "").lowercase(Locale.US)
                    val rel = (if (relIdx >= 0) c.getString(relIdx) else "")?.lowercase(Locale.US) ?: ""
                    if (rel.contains("screenshot") || name.contains("screenshot")) {
                        lastScreenshotId = id
                        picked = ContentUris.withAppendedId(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, id)
                        break
                    }
                }
                picked
            }
        } catch (_: SecurityException) {
            null
        } catch (_: Exception) {
            null
        }

        val uri = uriFound ?: return false
        val png = encodeUriAsPng(ctx, uri) ?: return false
        val hash = "img:" + sha256(png)
        if (hash == lastHash) return false
        if (net.pushClipboardPng(png)) {
            lastHash = hash
            return true
        }
        return false
    }
}

/**
 * Zero-notification, 100% idle/event-driven background service bound by Android Accessibility.
 * Sleeps with 0 CPU/timers until a UI copy event or screenshot MediaStore update occurs.
 */
class ClipboardAccessibilityService : AccessibilityService() {
    companion object {
        fun isEnabled(ctx: Context): Boolean {
            val expected = ComponentName(ctx, ClipboardAccessibilityService::class.java).flattenToString()
            val enabled = Settings.Secure.getString(
                ctx.contentResolver,
                Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES,
            ) ?: return false
            return enabled.split(':').any { it.equals(expected, ignoreCase = true) }
        }
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private lateinit var store: Store
    private lateinit var net: Net
    private var cm: ClipboardManager? = null
    private var startEpochSec: Long = 0L
    private var eventJob: Job? = null
    private var shotJob: Job? = null

    private val clipListener = ClipboardManager.OnPrimaryClipChangedListener {
        if (!store.isPaired || !store.clipboardSync) return@OnPrimaryClipChangedListener
        scope.launch { ClipboardSyncHelper.syncClipboardNow(applicationContext, store, net) }
    }

    private val screenshotObserver = object : ContentObserver(Handler(Looper.getMainLooper())) {
        override fun onChange(selfChange: Boolean) {
            super.onChange(selfChange)
            if (!store.isPaired || !store.clipboardSync) return
            shotJob?.cancel()
            shotJob = scope.launch {
                delay(350)
                ClipboardSyncHelper.syncLatestScreenshotSince(applicationContext, store, net, startEpochSec)
            }
        }
    }

    override fun onServiceConnected() {
        super.onServiceConnected()
        store = Store(this)
        net = Net()
        startEpochSec = System.currentTimeMillis() / 1000L

        cm = (getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager)?.also {
            it.addPrimaryClipChangedListener(clipListener)
        }
        try {
            contentResolver.registerContentObserver(
                MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
                true,
                screenshotObserver,
            )
        } catch (_: Exception) {
        }
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        if (!store.isPaired || !store.clipboardSync) return
        eventJob?.cancel()
        eventJob = scope.launch {
            delay(220)
            ClipboardSyncHelper.syncClipboardNow(applicationContext, store, net)
        }
    }

    override fun onInterrupt() {}

    override fun onDestroy() {
        cm?.removePrimaryClipChangedListener(clipListener)
        try { contentResolver.unregisterContentObserver(screenshotObserver) } catch (_: Exception) {}
        net.clear()
        scope.cancel()
        super.onDestroy()
    }
}

/**
 * Quick Settings Tile ("Sync Clipboard") — zero persistent notifications, works anytime with 1 swipe+tap.
 */
class ClipboardTileService : TileService() {
    override fun onStartListening() {
        super.onStartListening()
        val store = Store(this)
        qsTile?.apply {
            state = if (store.isPaired && store.clipboardSync) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE
            updateTile()
        }
    }

    override fun onClick() {
        super.onClick()
        val intent = Intent(this, ClipboardTrampolineActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            val pi = android.app.PendingIntent.getActivity(
                this,
                42,
                intent,
                android.app.PendingIntent.FLAG_UPDATE_CURRENT or android.app.PendingIntent.FLAG_IMMUTABLE,
            )
            startActivityAndCollapse(pi)
        } else {
            @Suppress("DEPRECATION")
            startActivityAndCollapse(intent)
        }
    }
}

/**
 * Translucent 1-frame focus trampoline used by Quick Settings Tile / shortcuts to push clipboard on demand.
 */
class ClipboardTrampolineActivity : Activity() {
    private var synced = false

    private fun disableTransition(kind: Int) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            overrideActivityTransition(kind, 0, 0)
        } else {
            @Suppress("DEPRECATION")
            overridePendingTransition(0, 0)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        disableTransition(OVERRIDE_TRANSITION_OPEN)
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus || synced) return
        synced = true
        val store = Store(this)
        val net = Net()
        val ctx = applicationContext
        CoroutineScope(Dispatchers.Main).launch {
            val ok = ClipboardSyncHelper.syncClipboardNow(ctx, store, net, force = true)
            Toast.makeText(
                ctx,
                if (ok) "Synced to PC clipboard" else "Nothing new to sync",
                Toast.LENGTH_SHORT,
            ).show()
            net.clear()
            finish()
            disableTransition(OVERRIDE_TRANSITION_CLOSE)
        }
    }
}
