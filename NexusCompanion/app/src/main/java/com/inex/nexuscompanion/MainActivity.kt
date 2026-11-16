package com.inex.nexuscompanion

import android.content.Context
import android.database.ContentObserver
import android.media.AudioManager
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.view.KeyEvent
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private val net = Net()
    private lateinit var store: Store
    private var volumeObserver: ContentObserver? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        store = Store(this)

        // Tell Android this window controls STREAM_MUSIC natively
        volumeControlStream = AudioManager.STREAM_MUSIC

        // Observe Android media volume changes (slider touch/drag & hardware keys)
        volumeObserver = object : ContentObserver(Handler(Looper.getMainLooper())) {
            override fun onChange(selfChange: Boolean) {
                super.onChange(selfChange)
                if (store.isPaired && !net.audioPlayer.isPlaying) {
                    val am = getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: return
                    val cur = am.getStreamVolume(AudioManager.STREAM_MUSIC)
                    val max = am.getStreamMaxVolume(AudioManager.STREAM_MUSIC)
                    if (max > 0) {
                        val pct = (cur * 100) / max
                        lifecycleScope.launch { net.command("Volume $pct%") }
                    }
                }
            }
        }
        try {
            contentResolver.registerContentObserver(
                Settings.System.CONTENT_URI,
                true,
                volumeObserver!!
            )
        } catch (_: Exception) {}

        setContent {
            NexusTheme {
                App(store, net)
            }
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        volumeObserver?.let {
            try { contentResolver.unregisterContentObserver(it) } catch (_: Exception) {}
        }
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent?): Boolean {
        if (store.isPaired) {
            // When audio is streaming to the phone, let Android handle volume natively (shows popup & scales earphone sound)
            if (net.audioPlayer.isPlaying) {
                return super.onKeyDown(keyCode, event)
            }

            // When audio is on PC speakers, show native Android popup & adjust volume
            val am = getSystemService(Context.AUDIO_SERVICE) as AudioManager
            when (keyCode) {
                KeyEvent.KEYCODE_VOLUME_UP -> {
                    am.adjustStreamVolume(AudioManager.STREAM_MUSIC, AudioManager.ADJUST_RAISE, AudioManager.FLAG_SHOW_UI)
                    net.volumeUp()
                    return true
                }
                KeyEvent.KEYCODE_VOLUME_DOWN -> {
                    am.adjustStreamVolume(AudioManager.STREAM_MUSIC, AudioManager.ADJUST_LOWER, AudioManager.FLAG_SHOW_UI)
                    net.volumeDown()
                    return true
                }
            }
        }
        return super.onKeyDown(keyCode, event)
    }
}
