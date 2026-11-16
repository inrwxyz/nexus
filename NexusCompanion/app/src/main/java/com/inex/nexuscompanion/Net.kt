package com.inex.nexuscompanion

import android.os.Handler
import android.os.Looper
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.Socket
import java.util.Locale

class PairResult(val token: String?, val error: String?, val mac: String = "", val pcName: String = "Windows PC")

data class AudioDev(val id: String, val name: String, val kind: String, val isDefault: Boolean)

enum class Reply { Done, Failed, Offline, Unauthorized }

enum class PcState { Online, Offline, Unauthorized }

/**
 * Everything is event-driven: no timers, no polling loops.
 * - HTTP calls happen only when the user does something (open app, send a command, unpair).
 * - UDP receive just blocks until the PC says "unauthorized" (PC unpaired this phone).
 */
class Net {
    @Volatile private var ip = ""
    @Volatile private var port = 41780
    @Volatile private var udpPort = 41781
    @Volatile private var token = ""
    @Volatile private var addr: InetAddress? = null
    @Volatile private var socket: DatagramSocket? = null

    /** Called on the main thread when the PC rejects our token (401 or UDP "unauthorized"). */
    @Volatile var onUnauthorized: (() -> Unit)? = null
    @Volatile var onAutoKeyboard: ((Boolean) -> Unit)? = null
    @Volatile var lastMac: String = ""
    @Volatile var lastPcName: String = "Windows PC"

    private val main = Handler(Looper.getMainLooper())
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val queue = Channel<String>(Channel.UNLIMITED)

    init {
        scope.launch {
            for (msg in queue) {
                try {
                    val a = addr ?: InetAddress.getByName(ip).also { addr = it }
                    val s = socket ?: openSocket()
                    val b = msg.toByteArray()
                    s.send(DatagramPacket(b, b.size, a, udpPort))
                } catch (_: Exception) {
                    addr = null
                }
            }
        }
    }

    private fun openSocket(): DatagramSocket {
        val s = DatagramSocket()
        socket = s
        scope.launch {
            val buf = ByteArray(512)
            while (!s.isClosed) {
                try {
                    val p = DatagramPacket(buf, buf.size)
                    s.receive(p) // blocks, costs nothing while idle
                    val msg = String(p.data, 0, p.length)
                    if (msg.contains("unauthorized")) fireUnauthorized()
                    if (msg.contains("keyboard_auto")) {
                        val open = msg.contains("\"open\":true")
                        main.post { onAutoKeyboard?.invoke(open) }
                    }
                } catch (_: Exception) {
                    break
                }
            }
        }
        return s
    }

    private fun fireUnauthorized() {
        if (token.isEmpty()) return
        token = "" // stop sending right away
        main.post { onUnauthorized?.invoke() }
    }

    fun configure(ip: String, port: Int, token: String, udpPort: Int) {
        this.ip = ip
        this.port = port
        this.token = token
        this.udpPort = udpPort
        this.addr = null
    }

    val audioPlayer = PcAudioPlayer()

    fun clear() {
        token = ""
        audioPlayer.stop()
        try { socket?.close() } catch (_: Exception) {}
        socket = null
    }

    // ---- UDP trackpad ----
    private fun udp(json: String) {
        if (token.isEmpty()) return
        queue.trySend(json)
    }

    fun move(dx: Float, dy: Float) =
        udp(String.format(Locale.US, "{\"token\":\"%s\",\"type\":\"move\",\"dx\":%.2f,\"dy\":%.2f}", token, dx, dy))

    fun scroll(dx: Float, dy: Float) =
        udp(String.format(Locale.US, "{\"token\":\"%s\",\"type\":\"scroll\",\"dx\":%.2f,\"dy\":%.2f}", token, dx, dy))

    fun scrollEnd() = udp("{\"token\":\"$token\",\"type\":\"scroll_end\"}")

    fun click(btn: String) = udp("{\"token\":\"$token\",\"type\":\"click\",\"btn\":\"$btn\"}")

    fun gesture(name: String) = udp("{\"token\":\"$token\",\"type\":\"gesture\",\"name\":\"$name\"}")

    fun key(k: String) = udp("{\"token\":\"$token\",\"type\":\"key\",\"key\":\"$k\"}")

    fun volumeUp() = udp("{\"token\":\"$token\",\"type\":\"key\",\"key\":\"volume_up\"}")
    fun volumeDown() = udp("{\"token\":\"$token\",\"type\":\"key\",\"key\":\"volume_down\"}")

    fun text(t: String) {
        val escaped = JSONObject.quote(t)
        udp("{\"token\":\"$token\",\"type\":\"text\",\"text\":$escaped}")
    }

fun down(btn: String) = udp("{\"token\":\"$token\",\"type\":\"down\",\"btn\":\"$btn\"}")
fun up(btn: String) = udp("{\"token\":\"$token\",\"type\":\"up\",\"btn\":\"$btn\"}")

    // ---- HTTP (raw socket, one write: headers + body together) ----
    private class Http(val code: Int, val json: JSONObject?)

    private suspend fun http(
        host: String,
        port: Int,
        method: String,
        path: String,
        body: String?,
        bearer: String?,
        readMs: Int = 20000,
    ): Http? = withContext(Dispatchers.IO) {
        try {
            Socket().use { s ->
                s.connect(InetSocketAddress(host, port), 4000)
                s.soTimeout = readMs
                val b = body?.toByteArray() ?: ByteArray(0)
                val head = StringBuilder()
                    .append("$method $path HTTP/1.1\r\n")
                    .append("Host: $host:$port\r\n")
                    .append("Content-Type: application/json\r\n")
                    .apply { if (!bearer.isNullOrEmpty()) append("Authorization: Bearer $bearer\r\n") }
                    .append("Content-Length: ${b.size}\r\n")
                    .append("Connection: close\r\n\r\n")
                    .toString()
                val out = s.getOutputStream()
                out.write(head.toByteArray() + b)
                out.flush()
                val resp = s.getInputStream().readBytes().toString(Charsets.UTF_8)
                val i = resp.indexOf("\r\n\r\n")
                if (i < 0) {
                    null
                } else {
                    val code = resp.substringBefore("\r\n").split(" ").getOrNull(1)?.toIntOrNull() ?: 0
                    val json = try { JSONObject(resp.substring(i + 4)) } catch (_: Exception) { null }
                    Http(code, json)
                }
            }
        } catch (_: Exception) {
            null
        }
    }

    /** One-time handshake: QR key in, session token out. */
    suspend fun pair(host: String, port: Int, key: String, device: String): PairResult {
        val body = JSONObject().put("key", key).put("device", device).toString()
        val r = http(host, port, "POST", "/api/companion/pair", body, null)
            ?: return PairResult(null, "Can't reach the PC. Check that it is on the same Wi-Fi and Nexus is running.")
        val j = r.json
        if (r.code == 200 && j != null && j.optString("status") == "ok" && j.optString("token").isNotEmpty()) {
            val pc = j.optString("pc_name").takeIf { it.isNotEmpty() } ?: "Windows PC"
            lastPcName = pc
            return PairResult(j.optString("token"), null, j.optString("mac"), pc)
        }
        val msg = j?.optString("message").orEmpty()
        return PairResult(null, if (msg.isNotEmpty()) msg else "Pairing failed")
    }

    /** Single lightweight ping, used once when the app opens. */
    suspend fun status(): PcState {
        val r = http(ip, port, "GET", "/api/companion/status", null, token, readMs = 4000)
            ?: return PcState.Offline
        if (r.code == 401) {
            fireUnauthorized()
            return PcState.Unauthorized
        }
        r.json?.optString("mac")?.takeIf { it.isNotEmpty() }?.let { lastMac = it }
        r.json?.optString("pc_name")?.takeIf { it.isNotEmpty() }?.let { lastPcName = it }
        return PcState.Online
    }

    suspend fun command(text: String): Reply = relay("/api/companion/command", "command", text)

    /** Spoken query (VoiceActivity, Assistant routines). Same pipeline on the PC. */
    suspend fun voice(query: String): Reply = relay("/api/companion/voice", "query", query)

    private suspend fun relay(path: String, field: String, text: String): Reply {
        val body = JSONObject().put(field, text).toString()
        val r = http(ip, port, "POST", path, body, token) ?: return Reply.Offline
        return when {
            r.code == 401 -> { fireUnauthorized(); Reply.Unauthorized }
            r.code == 200 && r.json?.optBoolean("success") == true -> Reply.Done
            else -> Reply.Failed
        }
    }

    suspend fun audioDevices(): List<AudioDev>? {
        val r = http(ip, port, "GET", "/api/companion/audio/devices", null, token, readMs = 4000) ?: return null
        if (r.code == 401) { fireUnauthorized(); return null }
        val arr = r.json?.optJSONArray("devices") ?: return emptyList()
        return List(arr.length()) { i ->
            val o = arr.getJSONObject(i)
            AudioDev(o.optString("id"), o.optString("name"), o.optString("kind"), o.optBoolean("default"))
        }
    }

    suspend fun audioSwitch(idOrKind: String): Boolean {
        val body = JSONObject().put("id", idOrKind).put("kind", idOrKind).toString()
        val r = http(ip, port, "POST", "/api/companion/audio/switch", body, token, readMs = 5000) ?: return false
        return r.code == 200 && r.json?.optBoolean("success") == true
    }

    /** start=true -> returns sample rate, or null on failure */
    suspend fun audioStream(start: Boolean, udpPort: Int = 41782): Int? {
        val body = JSONObject().put("action", if (start) "start" else "stop").put("port", udpPort).toString()
        val r = http(ip, port, "POST", "/api/companion/audio/stream", body, token, readMs = 5000) ?: return null
        if (r.code != 200) return null
        return r.json?.optInt("rate", 48000) ?: 48000
    }

    /** Wake-on-LAN magic packet: 6x0xFF + 16xMAC. */
    suspend fun wake(mac: String, hostIp: String): Boolean = withContext(Dispatchers.IO) {
        try {
            val m = mac.split(":", "-").map { it.toInt(16).toByte() }
            if (m.size != 6) return@withContext false
            val pkt = ByteArray(6) { 0xFF.toByte() } + (1..16).flatMap { m }.toByteArray()
            DatagramSocket().use { s ->
                s.broadcast = true
                val targets = mutableListOf("255.255.255.255")
                hostIp.substringBeforeLast('.', "").takeIf { it.isNotEmpty() }?.let { targets += "$it.255" }
                for (t in targets) for (p in intArrayOf(9, 7)) {
                    s.send(DatagramPacket(pkt, pkt.size, InetAddress.getByName(t), p))
                }
            }
            true
        } catch (_: Exception) { false }
    }

    /** Tell the PC to revoke this phone. Local wipe is done by the caller either way. */
    suspend fun unpair() {
        http(ip, port, "POST", "/api/companion/unpair", "{}", token, readMs = 4000)
        clear()
    }
}
