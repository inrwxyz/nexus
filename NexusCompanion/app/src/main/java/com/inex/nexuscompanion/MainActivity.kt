package com.inex.nexuscompanion

import android.media.AudioManager
import android.os.Bundle
import android.view.KeyEvent
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private val net = Net()
    private lateinit var store: Store

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        store = Store(this)

        // Tell Android this window controls STREAM_MUSIC natively
        volumeControlStream = AudioManager.STREAM_MUSIC

        setContent {
            NexusTheme {
                App(store, net)
            }
        }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus && store.isPaired && store.clipboardSync) {
            lifecycleScope.launch {
                ClipboardSyncHelper.syncClipboardNow(applicationContext, store, net)
            }
        }
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent?): Boolean {
        if (store.isPaired) {
            when (keyCode) {
                KeyEvent.KEYCODE_VOLUME_UP -> { net.volumeUp(); return true }
                KeyEvent.KEYCODE_VOLUME_DOWN -> { net.volumeDown(); return true }
            }
        }
        return super.onKeyDown(keyCode, event)
    }
}
