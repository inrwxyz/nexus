package com.inex.nexuscompanion

import android.content.ActivityNotFoundException
import android.content.Intent
import android.os.Bundle
import android.speech.RecognizerIntent
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.launch

/**
 * No UI. Gets a command, sends it to the PC (Net.voice), shows one Toast, closes.
 *
 * Where the text comes from (first one found wins):
 *  - intent extra "query" / "command" / android.intent.extra.TEXT   (Tasker, shortcuts, Share)
 *  - deep link  nexus://cmd?q=close%20chrome
 *  - nothing given -> opens the system speech prompt and uses what you say
 */
class VoiceActivity : ComponentActivity() {
    private lateinit var store: Store
    private lateinit var net: Net

    private val speech = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { r ->
        val text = r.data?.getStringArrayListExtra(RecognizerIntent.EXTRA_RESULTS)?.firstOrNull()
        if (text.isNullOrBlank()) finish() else send(text)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        store = Store(this)
        if (!store.isPaired) {
            Toast.makeText(this, "Open Nexus and scan the PC QR first", Toast.LENGTH_LONG).show()
            startActivity(Intent(this, MainActivity::class.java))
            finish()
            return
        }
        net = Net().also { it.configure(store.ip, store.port, store.token, store.udpPort) }

        val preset = queryFrom(intent)
        if (preset.isNullOrBlank()) listen() else send(preset)
    }

    private fun queryFrom(i: Intent?): String? {
        if (i == null) return null
        return i.getStringExtra("query")
            ?: i.getStringExtra("command")
            ?: i.getStringExtra(Intent.EXTRA_TEXT)
            ?: i.data?.getQueryParameter("q")
    }

    private fun listen() {
        try {
            speech.launch(
                Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH)
                    .putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
            )
        } catch (_: ActivityNotFoundException) {
            Toast.makeText(this, "Speech input not available", Toast.LENGTH_LONG).show()
            finish()
        }
    }

    private fun send(query: String) {
        val app = applicationContext
        val q = query.trim()
        MainScope().launch {
            val reply = net.voice(q)
            if (reply == Reply.Unauthorized) store.clearPairing()
            val msg = when (reply) {
                Reply.Done -> "Done: $q"
                Reply.Failed -> "Failed: $q"
                Reply.Offline -> "PC offline"
                Reply.Unauthorized -> "Unpaired. Open Nexus and scan the QR again"
            }
            Toast.makeText(app, msg, Toast.LENGTH_SHORT).show()
            net.clear()
            finish()
        }
    }
}
