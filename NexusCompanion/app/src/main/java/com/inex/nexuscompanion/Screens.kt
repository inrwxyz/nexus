package com.inex.nexuscompanion

import android.Manifest
import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.speech.RecognizerIntent
import android.widget.Toast
import androidx.core.content.ContextCompat
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

@Composable
internal fun Modifier.plainClick(onClick: () -> Unit): Modifier =
    clickable(interactionSource = remember { MutableInteractionSource() }, indication = null, onClick = onClick)

@Composable
fun App(store: Store, net: Net) {
    val cs = MaterialTheme.colorScheme
    val scope = rememberCoroutineScope()
    var paired by remember { mutableStateOf(store.isPaired) }
    var tab by remember { mutableIntStateOf(0) }
    var speed by remember { mutableFloatStateOf(store.speed) }
    var padOn by remember { mutableStateOf(false) }
    var offline by remember { mutableStateOf(false) }
    var padLocked by remember { mutableStateOf(false) }
    var confirmGamepadOpen by remember { mutableStateOf(false) }
    var mac by remember { mutableStateOf(store.mac) }

    DisposableEffect(net) {
        net.onUnauthorized = {
            store.clearPairing()
            paired = false
            padLocked = false
        }
        onDispose { net.onUnauthorized = null }
    }

    LaunchedEffect(paired) {
        if (paired) {
            net.configure(store.ip, store.port, store.token, store.udpPort)
            offline = net.status() == PcState.Offline
            if (net.lastMac.isNotEmpty()) { store.mac = net.lastMac; mac = net.lastMac }
            if (net.lastPcName.isNotEmpty()) { store.pcName = net.lastPcName }
        } else {
            net.clear()
            tab = 0
        }
    }

    val fullLock = (padLocked && tab == 0) || tab == 4
    Column(
        Modifier
            .fillMaxSize()
            .background(if (paired && (tab == 0 || tab == 4)) Color.Black else cs.background)
            .systemBarsPadding()
            .imePadding()
    ) {
        if (!paired) {
            PairScreen(store, net) { paired = true; mac = store.mac }
        } else {
            if (!fullLock) {
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Image(painterResource(R.drawable.ic_nexus_logo), "Nexus", Modifier.size(28.dp))
                    Spacer(Modifier.width(10.dp))
                    Text(
                        "Nexus", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Medium,
                        color = if (tab == 0) Color.White else cs.onBackground,
                    )
                    Spacer(Modifier.weight(1f))
                    if (offline) {
                        if (mac.isNotEmpty()) {
                            PowerOnButton {
                                scope.launch {
                                    net.wake(mac, store.ip)
                                    delay(10_000)
                                    offline = net.status() == PcState.Offline
                                }
                            }
                        } else {
                            Box(
                                Modifier.clip(RoundedCornerShape(12.dp)).background(cs.surfaceContainerHigh)
                                    .plainClick { scope.launch { offline = net.status() == PcState.Offline } }
                                    .padding(horizontal = 10.dp, vertical = 4.dp),
                            ) { Text("PC Offline", style = MaterialTheme.typography.labelSmall, color = cs.onSurfaceVariant) }
                        }
                    }
                }
            }
            Box(Modifier.weight(1f).fillMaxWidth()) {
                when (tab) {
                    0 -> TrackpadScreen(net, speed, padOn, padLocked, store.autoKeyboard, { padLocked = it }) { padOn = !padOn }
                    1 -> CommandScreen(net) { reachable -> offline = !reachable }
                    2 -> ControlScreen(net, store, offline) { offline = it }
                    4 -> GameControllerScreen(store, net) { tab = 0 }
                    else -> SettingsScreen(store, net, speed, { speed = it; store.speed = it }) { paired = false }
                }
            }
            if (!fullLock) TabBar(tab) { nextTab ->
                if (nextTab == 4 && tab != 4) confirmGamepadOpen = true
                else tab = nextTab
            }
        }
    }

    if (confirmGamepadOpen) {
        AlertDialog(
            onDismissRequest = { confirmGamepadOpen = false },
            title = { Text("Open Gamepad?") },
            text = { Text("Switch to full-screen landscape controller HUD for PC gaming?") },
            confirmButton = {
                TextButton(onClick = {
                    confirmGamepadOpen = false
                    tab = 4
                }) { Text("Open") }
            },
            dismissButton = {
                TextButton(onClick = { confirmGamepadOpen = false }) { Text("Cancel") }
            },
        )
    }
}

@Composable
private fun PowerOnButton(onClick: () -> Unit) {
    val glow by rememberInfiniteTransition(label = "glow").animateFloat(
        initialValue = 0.25f, targetValue = 0.95f,
        animationSpec = infiniteRepeatable(tween(1400), RepeatMode.Reverse), label = "a",
    )
    val shape = RoundedCornerShape(20.dp)
    Box(
        Modifier
            .shadow(14.dp, shape, ambientColor = Color.White.copy(alpha = glow), spotColor = Color.White.copy(alpha = glow))
            .clip(shape)
            .background(Color(0xFF111111))
            .border(1.dp, Color.White.copy(alpha = glow), shape)
            .plainClick(onClick)
            .padding(horizontal = 16.dp, vertical = 8.dp),
    ) { Text("⚡ Wake PC", style = MaterialTheme.typography.labelMedium, color = Color.White) }
}

@Composable
private fun TabBar(selected: Int, onSelect: (Int) -> Unit) {
    val cs = MaterialTheme.colorScheme
    val tabs = listOf(
        0 to "Trackpad",
        1 to "Command",
        4 to "Gamepad",
        2 to "Control",
        3 to "Settings",
    )
    val pillShape = CircleShape
    Box(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 10.dp, vertical = 8.dp)
            .clip(pillShape)
            .border(1.dp, cs.outline.copy(alpha = 0.35f), pillShape)
            .background(cs.surfaceContainerLow)
            .padding(3.dp),
    ) {
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(3.dp),
        ) {
            tabs.forEach { (tabIdx, label) ->
                val on = tabIdx == selected
                Box(
                    Modifier
                        .weight(1f)
                        .height(38.dp)
                        .clip(pillShape)
                        .then(
                            if (on) {
                                Modifier
                                    .border(1.dp, cs.outline.copy(alpha = 0.55f), pillShape)
                                    .background(cs.surfaceContainerHigh)
                            } else {
                                Modifier.background(Color.Transparent)
                            }
                        )
                        .plainClick { onSelect(tabIdx) },
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        label,
                        style = MaterialTheme.typography.labelSmall,
                        fontWeight = if (on) FontWeight.SemiBold else FontWeight.Normal,
                        color = if (on) cs.onSurface else cs.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

@Composable
fun PairScreen(store: Store, net: Net, onDone: () -> Unit) {
    val cs = MaterialTheme.colorScheme
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    var busy by remember { mutableStateOf(false) }
    var err by remember { mutableStateOf<String?>(null) }

    fun doPair(ip: String, port: Int, udp: Int, key: String) {
        busy = true
        err = null
        scope.launch {
            val r = net.pair(ip, port, key, Build.MODEL)
            busy = false
            val t = r.token
            if (t != null) {
                store.ip = ip
                store.port = port
                store.udpPort = udp
                store.token = t
                store.mac = r.mac
                store.pcName = r.pcName
                onDone()
            } else {
                err = r.error
            }
        }
    }

    fun scan() {
        err = null
        val options = GmsBarcodeScannerOptions.Builder()
            .setBarcodeFormats(Barcode.FORMAT_QR_CODE)
            .build()
        GmsBarcodeScanning.getClient(ctx, options).startScan()
            .addOnSuccessListener { b ->
                val uri = runCatching { Uri.parse(b.rawValue ?: "") }.getOrNull()
                val ip = uri?.getQueryParameter("ip")
                val key = uri?.getQueryParameter("key")
                val port = uri?.getQueryParameter("port")?.toIntOrNull() ?: 41780
                val udp = uri?.getQueryParameter("udp")?.toIntOrNull() ?: 41781
                if (uri?.scheme == "nexus" && uri.host == "pair" && !ip.isNullOrBlank() && !key.isNullOrBlank()) {
                    doPair(ip, port, udp, key)
                } else if ((b.rawValue ?: "").startsWith("WIFI:T:ADB")) {
                    err = "This is a Wireless Debugging QR. Please scan from Phone Settings > Developer Options > Wireless Debugging > Pair device with QR code."
                } else {
                    err = "This is not a Nexus pairing QR."
                }
            }
            .addOnFailureListener { err = "QR scanner is not available on this phone." }
    }

    Column(
        Modifier.fillMaxSize().padding(32.dp),
        verticalArrangement = Arrangement.Center,
    ) {
        Image(painterResource(R.drawable.ic_nexus_logo), "Nexus", Modifier.size(72.dp))
        Spacer(Modifier.height(16.dp))
        Text("Nexus", style = MaterialTheme.typography.displaySmall, fontWeight = FontWeight.Light, color = cs.onBackground)
        Spacer(Modifier.height(8.dp))
        Text(
            "On your PC open Nexus, Phone Link, and scan the QR code.",
            style = MaterialTheme.typography.bodyMedium,
            color = cs.onSurfaceVariant,
        )
        Spacer(Modifier.height(40.dp))
        Button(
            onClick = { scan() },
            enabled = !busy,
            shape = RoundedCornerShape(18.dp),
            modifier = Modifier.fillMaxWidth().height(58.dp),
        ) { Text(if (busy) "Connecting" else "Scan PC QR Code") }
        err?.let {
            Spacer(Modifier.height(14.dp))
            Text(it, color = cs.error, style = MaterialTheme.typography.bodySmall)
        }
    }
}

private data class Cmd(val text: String, val tags: String = "")

private val BUILT_IN = listOf(
    Cmd("Close Chrome"), Cmd("Close Spotify"),
    Cmd("Volume 20%", "sound audio"), Cmd("Volume 50%", "sound audio"), Cmd("Volume 100%", "sound audio"),
    Cmd("Mute PC", "volume sound audio"),
    Cmd("Shutdown PC", "power off"), Cmd("Shutdown in 5 seconds", "power timer off"),
    Cmd("Shutdown in 10 minutes", "power timer off"), Cmd("Shutdown in 30 minutes", "power timer off"),
    Cmd("Shutdown in 1 hour", "power timer off"), Cmd("Cancel Shutdown", "abort power stop"),
    Cmd("Restart PC", "power reboot"), Cmd("Restart in 5 minutes", "power timer reboot"),
    Cmd("Sleep PC", "power suspend"),
    Cmd("Lock PC", "power"), Cmd("Show Desktop", "minimize"),
    Cmd("Spotify Pause", "music"), Cmd("Spotify Next", "music skip"),
)

private fun suggest(q: String): List<Cmd> {
    val s = q.trim().lowercase()
    if (s.isEmpty()) return emptyList()
    return BUILT_IN.filter { c ->
        c.text.lowercase().startsWith(s) ||
            "${c.text} ${c.tags}".lowercase().split(' ', '%').any { it.startsWith(s) }
    }.filter { !it.text.equals(q.trim(), ignoreCase = true) }.take(6)
}

@Composable
private fun AudioSwitcher(net: Net) {
    val cs = MaterialTheme.colorScheme
    val scope = rememberCoroutineScope()
    var devs by remember { mutableStateOf<List<AudioDev>>(emptyList()) }
    var expanded by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        devs = net.audioDevices() ?: emptyList()
    }

    val currentDev = devs.firstOrNull { it.isDefault } ?: devs.firstOrNull()

    Box(Modifier.fillMaxWidth()) {
        Row(
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(16.dp))
                .background(cs.surfaceContainerHigh)
                .plainClick { expanded = !expanded }
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            val icon = if (currentDev?.kind == "headphones") "🎧" else "🔊"
            Text(icon, fontSize = 20.sp)
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    currentDev?.name ?: if (devs.isEmpty()) "Loading devices..." else "Select Device",
                    style = MaterialTheme.typography.bodyMedium,
                    fontWeight = FontWeight.SemiBold,
                    color = cs.onSurface,
                    maxLines = 1,
                )
                Text(
                    if (devs.size > 1) "Tap to change output (${devs.size} devices)" else "Default audio playback device",
                    style = MaterialTheme.typography.labelSmall,
                    color = cs.onSurfaceVariant,
                )
            }
            Spacer(Modifier.width(8.dp))
            Text(if (expanded) "▲" else "▼", fontSize = 12.sp, color = cs.onSurfaceVariant)
        }

        DropdownMenu(
            expanded = expanded,
            onDismissRequest = { expanded = false },
            modifier = Modifier.background(cs.surfaceContainerHigh).clip(RoundedCornerShape(14.dp))
        ) {
            devs.forEach { dev ->
                val isCurrent = dev.isDefault
                DropdownMenuItem(
                    text = {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(if (dev.kind == "headphones") "🎧" else "🔊", fontSize = 16.sp)
                            Spacer(Modifier.width(10.dp))
                            Text(
                                dev.name,
                                fontWeight = if (isCurrent) FontWeight.Bold else FontWeight.Normal,
                                color = if (isCurrent) cs.primary else cs.onSurface,
                            )
                            if (isCurrent) {
                                Spacer(Modifier.width(8.dp))
                                Text("✓", color = cs.primary, fontWeight = FontWeight.Bold)
                            }
                        }
                    },
                    onClick = {
                        expanded = false
                        if (!isCurrent) {
                            scope.launch {
                                if (net.audioSwitch(dev.id)) {
                                    devs = devs.map { it.copy(isDefault = it.id == dev.id) }
                                }
                            }
                        }
                    }
                )
            }
        }
    }
}

@Composable
private fun GoogleStyleMicButton(onClick: () -> Unit, modifier: Modifier = Modifier) {
    val cs = MaterialTheme.colorScheme
    Box(
        modifier
            .size(92.dp)
            .shadow(10.dp, CircleShape, ambientColor = Color(0xFF4285F4).copy(alpha = 0.25f), spotColor = Color(0xFF4285F4).copy(alpha = 0.25f))
            .clip(CircleShape)
            .background(cs.surfaceContainerHigh)
            .border(1.dp, cs.outlineVariant.copy(alpha = 0.65f), CircleShape)
            .plainClick(onClick),
        contentAlignment = Alignment.Center,
    ) {
        Canvas(Modifier.size(40.dp)) {
            val w = size.width
            val h = size.height
            val cx = w / 2f
            val sw = w * 0.095f

            // Capsule head (Google Blue)
            val capW = w * 0.27f
            val capH = h * 0.44f
            drawRoundRect(
                color = Color(0xFF4285F4),
                topLeft = Offset(cx - capW / 2f, h * 0.13f),
                size = Size(capW, capH),
                cornerRadius = CornerRadius(capW / 2f, capW / 2f),
            )

            // Circular U-cradle (Yellow left + Red bottom-right)
            val arcR = w * 0.245f
            val arcTop = h * 0.235f
            val arcSize = Size(arcR * 2f, arcR * 2f)
            val arcLeft = cx - arcR

            drawArc(
                color = Color(0xFFFBBC05),
                startAngle = 132f,
                sweepAngle = 48f,
                useCenter = false,
                topLeft = Offset(arcLeft, arcTop),
                size = arcSize,
                style = Stroke(width = sw, cap = StrokeCap.Round),
            )
            drawArc(
                color = Color(0xFFEA4335),
                startAngle = 0f,
                sweepAngle = 136f,
                useCenter = false,
                topLeft = Offset(arcLeft, arcTop),
                size = arcSize,
                style = Stroke(width = sw, cap = StrokeCap.Round),
            )

            // Stem (Google Green)
            val cradleBottom = arcTop + arcSize.height
            drawLine(
                color = Color(0xFF34A853),
                start = Offset(cx, cradleBottom),
                end = Offset(cx, h * 0.88f),
                strokeWidth = sw,
                cap = StrokeCap.Round,
            )
        }
    }
}

@Composable
fun CommandScreen(net: Net, onReach: (Boolean) -> Unit) {
    val cs = MaterialTheme.colorScheme
    val scope = rememberCoroutineScope()
    var text by remember { mutableStateOf("") }
    val log = remember { mutableStateListOf<Pair<String, Reply>>() }
    var terminalAvailable by remember { mutableStateOf(net.remoteTerminalEnabled) }
    var terminalActive by remember { mutableStateOf(false) }
    var confirmTerminal by remember { mutableStateOf(false) }
    var shutdownTimerOpen by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        val st = net.status()
        onReach(st != PcState.Offline)
        terminalAvailable = net.remoteTerminalEnabled
        if (!terminalAvailable) terminalActive = false
    }

    fun sendCmd(c: String) {
        val cmd = c.trim()
        if (cmd.isEmpty()) return
        scope.launch {
            val reply = net.command(cmd)
            onReach(reply != Reply.Offline)
            log.add(0, cmd to reply)
        }
    }

    if (terminalActive) {
        RemoteTerminalView(net = net, onExit = { terminalActive = false })
        return
    }

    val speech = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { r ->
        r.data?.getStringArrayListExtra(RecognizerIntent.EXTRA_RESULTS)?.firstOrNull()?.let { sendCmd(it) }
    }
    fun listen() {
        try {
            speech.launch(
                Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH)
                    .putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
            )
        } catch (_: ActivityNotFoundException) {}
    }

    val suggestions = remember(text) { suggest(text) }

    Column(Modifier.fillMaxSize().padding(horizontal = 16.dp)) {
        Spacer(Modifier.height(20.dp))
        GoogleStyleMicButton(
            onClick = { listen() },
            modifier = Modifier.align(Alignment.CenterHorizontally),
        )
        Spacer(Modifier.height(22.dp))

        OutlinedTextField(
            value = text,
            onValueChange = { text = it },
            placeholder = { Text("Type a command...") },
            singleLine = true,
            shape = RoundedCornerShape(16.dp),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
            keyboardActions = KeyboardActions(onSend = { sendCmd(text); text = "" }),
            modifier = Modifier.fillMaxWidth(),
        )

        // Compact Shutdown Timer + optional Remote Terminal boxes below Type a command
        Spacer(Modifier.height(10.dp))
        Row(
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(16.dp))
                .background(cs.surfaceContainerLow)
                .border(1.dp, cs.outlineVariant.copy(alpha = 0.35f), RoundedCornerShape(16.dp))
                .plainClick { shutdownTimerOpen = true }
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("⏱", fontSize = 15.sp)
            Spacer(Modifier.width(10.dp))
            Text(
                "Shutdown Timer",
                style = MaterialTheme.typography.labelLarge,
                color = cs.onSurface,
                modifier = Modifier.weight(1f),
            )
            Text("Schedule ›", style = MaterialTheme.typography.labelSmall, color = cs.onSurfaceVariant)
        }

        if (terminalAvailable) {
            Spacer(Modifier.height(8.dp))
            Row(
                Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(16.dp))
                    .background(Color(0xFFEF4444).copy(alpha = 0.10f))
                    .border(1.dp, Color(0xFFEF4444).copy(alpha = 0.32f), RoundedCornerShape(16.dp))
                    .plainClick { confirmTerminal = true }
                    .padding(horizontal = 16.dp, vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(">_", fontSize = 13.sp, fontWeight = FontWeight.Bold, color = Color(0xFFEF4444))
                Spacer(Modifier.width(10.dp))
                Text(
                    "Remote Terminal",
                    style = MaterialTheme.typography.labelLarge,
                    fontWeight = FontWeight.SemiBold,
                    color = cs.onSurface,
                    modifier = Modifier.weight(1f),
                )
                Text("PowerShell ›", style = MaterialTheme.typography.labelSmall, color = Color(0xFFEF4444))
            }
        }

        // Live suggestions
        if (suggestions.isNotEmpty()) {
            Column(Modifier.fillMaxWidth().padding(top = 6.dp).clip(RoundedCornerShape(16.dp)).background(cs.surfaceContainerLow)) {
                suggestions.forEach { c ->
                    Text(
                        c.text,
                        style = MaterialTheme.typography.bodyLarge,
                        color = cs.onSurface,
                        modifier = Modifier.fillMaxWidth()
                            .plainClick {
                                scope.launch {
                                    text = c.text
                                    sendCmd(c.text)
                                    delay(350)
                                    text = ""
                                }
                            }
                            .padding(horizontal = 16.dp, vertical = 13.dp),
                    )
                }
            }
        }

        Spacer(Modifier.height(16.dp))
        LazyColumn(Modifier.fillMaxWidth().weight(1f)) {
            items(log) { (cmd, ok) ->
                Row(Modifier.fillMaxWidth().padding(vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(cmd, color = cs.onSurface, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                    Spacer(Modifier.width(12.dp))
                    Text(
                        when (ok) {
                            Reply.Done -> "Done"; Reply.Failed -> "Failed"
                            Reply.Offline -> "PC offline"; Reply.Unauthorized -> "Unpaired"
                        },
                        color = if (ok == Reply.Done) cs.onSurfaceVariant else cs.error,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
        }
    }

    if (shutdownTimerOpen) {
        ShutdownTimerDialog(
            onDismiss = { shutdownTimerOpen = false },
            onSubmitCommand = { sendCmd(it) },
        )
    }

    if (confirmTerminal) {
        AlertDialog(
            onDismissRequest = { confirmTerminal = false },
            title = { Text("Allow Terminal Access?") },
            text = { Text("Are you sure? Running terminal commands gives full PowerShell access on your PC. Pressing Back closes it immediately.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmTerminal = false
                    terminalActive = true
                }) { Text("Allow Access", color = Color(0xFFEF4444)) }
            },
            dismissButton = {
                TextButton(onClick = { confirmTerminal = false }) { Text("Cancel") }
            },
        )
    }
}

@Composable
fun ControlScreen(net: Net, store: Store, offline: Boolean, onOfflineChange: (Boolean) -> Unit) {
    val cs = MaterialTheme.colorScheme
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    var mac by remember { mutableStateOf(store.mac) }
    var confirmShutdown by remember { mutableStateOf(false) }

    Column(
        Modifier
            .fillMaxSize()
            .padding(horizontal = 16.dp)
            .verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        // Audio Output Switcher (Dropdown for all PC devices)
        Column(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(24.dp)).background(cs.surfaceContainerLow).padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Text("Audio Output", style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold, color = cs.onSurface)
            AudioSwitcher(net)
        }

        // Wake-on-LAN and Power Control
        Column(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(24.dp)).background(cs.surfaceContainerLow).padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text("Power & Remote Wake", style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold, color = cs.onSurface)

            if (mac.isNotEmpty()) {
                Button(
                    onClick = {
                        scope.launch {
                            net.wake(mac, store.ip)
                            delay(10_000)
                            val st = net.status()
                            onOfflineChange(st == PcState.Offline)
                        }
                    },
                    shape = RoundedCornerShape(16.dp),
                    modifier = Modifier.fillMaxWidth().height(48.dp),
                ) {
                    Text("⚡ Send Wake-on-LAN Packet")
                }
            }

            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf(
                    "Sleep" to "Sleep PC",
                    "Lock" to "Lock PC",
                    "Restart" to "Restart PC",
                ).forEach { (lbl, cmd) ->
                    Box(
                        Modifier.weight(1f).height(42.dp).clip(RoundedCornerShape(12.dp))
                            .background(cs.surfaceContainerHigh)
                            .plainClick { scope.launch { net.command(cmd) } },
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(lbl, style = MaterialTheme.typography.labelMedium, color = cs.onSurface)
                    }
                }
            }

            Box(
                Modifier
                    .fillMaxWidth()
                    .height(44.dp)
                    .clip(RoundedCornerShape(12.dp))
                    .background(Color(0xFFEF4444).copy(alpha = 0.14f))
                    .border(1.dp, Color(0xFFEF4444).copy(alpha = 0.35f), RoundedCornerShape(12.dp))
                    .plainClick { confirmShutdown = true },
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    "⏻  Shut Down PC",
                    style = MaterialTheme.typography.labelLarge,
                    fontWeight = FontWeight.SemiBold,
                    color = Color(0xFFEF4444),
                )
            }
        }
        Spacer(Modifier.height(16.dp))
    }

    if (confirmShutdown) {
        val targetPc = store.pcName.ifEmpty { "your PC" }
        AlertDialog(
            onDismissRequest = { confirmShutdown = false },
            title = { Text("Shut Down PC?") },
            text = { Text("Are you sure you want to shut down $targetPc? (5s timer starts on PC)") },
            confirmButton = {
                TextButton(onClick = {
                    confirmShutdown = false
                    scope.launch {
                        val res = net.command("Shutdown PC")
                        Toast.makeText(
                            ctx,
                            if (res == Reply.Done) "Shutting down $targetPc..." else "Could not reach PC",
                            Toast.LENGTH_SHORT,
                        ).show()
                    }
                }) { Text("Shut Down", color = Color(0xFFEF4444)) }
            },
            dismissButton = {
                TextButton(onClick = { confirmShutdown = false }) { Text("Cancel") }
            },
        )
    }
}

@Composable
fun SettingsScreen(store: Store, net: Net, speed: Float, onSpeed: (Float) -> Unit, onUnpaired: () -> Unit) {
    val cs = MaterialTheme.colorScheme
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    var autoKey by remember { mutableStateOf(store.autoKeyboard) }
    var clipSync by remember { mutableStateOf(store.clipboardSync) }
    var bgEnabled by remember { mutableStateOf(ClipboardAccessibilityService.isEnabled(ctx)) }

    val permLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {}

    LaunchedEffect(clipSync) {
        bgEnabled = ClipboardAccessibilityService.isEnabled(ctx)
    }

    fun enableClipboardSync(on: Boolean) {
        clipSync = on
        store.clipboardSync = on
        if (on) {
            val missing = mutableListOf<String>()
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                if (ContextCompat.checkSelfPermission(ctx, Manifest.permission.READ_MEDIA_IMAGES) != PackageManager.PERMISSION_GRANTED) {
                    missing += Manifest.permission.READ_MEDIA_IMAGES
                }
            } else if (ContextCompat.checkSelfPermission(ctx, Manifest.permission.READ_EXTERNAL_STORAGE) != PackageManager.PERMISSION_GRANTED) {
                missing += Manifest.permission.READ_EXTERNAL_STORAGE
            }
            if (missing.isNotEmpty()) permLauncher.launch(missing.toTypedArray())
            bgEnabled = ClipboardAccessibilityService.isEnabled(ctx)
            scope.launch {
                ClipboardSyncHelper.syncClipboardNow(ctx, store, net, force = true)
            }
        }
    }

    Column(
        Modifier
            .fillMaxSize()
            .padding(horizontal = 16.dp)
            .verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        // Connected PC (Shows PC name + Unpair button right inside the card!)
        Column(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(24.dp)).background(cs.surfaceContainerLow).padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Text("Connected PC", style = MaterialTheme.typography.bodySmall, color = cs.onSurfaceVariant)
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(8.dp).clip(CircleShape).background(Color(0xFF34C759)))
                Spacer(Modifier.width(8.dp))
                Text(
                    store.pcName.ifEmpty { "Windows PC" },
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    color = cs.onSurface,
                )
                Spacer(Modifier.weight(1f))
                Box(
                    Modifier
                        .clip(RoundedCornerShape(12.dp))
                        .background(cs.error.copy(alpha = 0.12f))
                        .plainClick {
                            scope.launch {
                                net.unpair()
                                store.clearPairing()
                                onUnpaired()
                            }
                        }
                        .padding(horizontal = 14.dp, vertical = 6.dp),
                ) {
                    Text("Unpair", style = MaterialTheme.typography.labelMedium, fontWeight = FontWeight.SemiBold, color = cs.error)
                }
            }
        }

        // Trackpad cursor speed
        Column(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(24.dp)).background(cs.surfaceContainerLow).padding(20.dp),
        ) {
            Text("Cursor Speed", style = MaterialTheme.typography.bodySmall, color = cs.onSurfaceVariant)
            Slider(value = speed, onValueChange = onSpeed, valueRange = 0.5f..2.5f)
        }

        // Keyboard Mode (Manual vs Auto)
        Column(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(24.dp)).background(cs.surfaceContainerLow).padding(20.dp),
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("Auto-open Keyboard", style = MaterialTheme.typography.bodyLarge, color = cs.onSurface)
                    Text("Off = manual tap on '⌨️ Keys' button", style = MaterialTheme.typography.bodySmall, color = cs.onSurfaceVariant)
                }
                Switch(
                    checked = autoKey,
                    onCheckedChange = {
                        autoKey = it
                        store.autoKeyboard = it
                    }
                )
            }
        }

        // Phone Clipboard Sync (Zero-notification idle background sync)
        Column(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(24.dp)).background(cs.surfaceContainerLow).padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f).padding(end = 12.dp)) {
                    Text("Clipboard Sync", style = MaterialTheme.typography.bodyLarge, color = cs.onSurface)
                    Text(
                        "Sync copied text, links & screenshots to PC (no notification)",
                        style = MaterialTheme.typography.bodySmall,
                        color = cs.onSurfaceVariant,
                    )
                }
                Switch(
                    checked = clipSync,
                    onCheckedChange = { enableClipboardSync(it) },
                )
            }
            if (clipSync) {
                Row(
                    Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text(
                        if (bgEnabled) "Idle auto-sync active (no battery drain)" else "Enable Accessibility for silent background sync",
                        style = MaterialTheme.typography.labelSmall,
                        color = if (bgEnabled) Color(0xFF34C759) else cs.onSurfaceVariant,
                        modifier = Modifier.weight(1f),
                    )
                    if (!bgEnabled) {
                        Box(
                            Modifier
                                .clip(RoundedCornerShape(10.dp))
                                .background(cs.primary)
                                .plainClick {
                                    ctx.startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))
                                }
                                .padding(horizontal = 12.dp, vertical = 6.dp),
                        ) {
                            Text("Enable", style = MaterialTheme.typography.labelMedium, color = cs.onPrimary)
                        }
                    }
                    Box(
                        Modifier
                            .clip(RoundedCornerShape(10.dp))
                            .background(cs.surfaceContainerHigh)
                            .plainClick {
                                bgEnabled = ClipboardAccessibilityService.isEnabled(ctx)
                                scope.launch {
                                    val ok = ClipboardSyncHelper.syncClipboardNow(ctx, store, net, force = true)
                                    Toast.makeText(
                                        ctx,
                                        if (ok) "Clipboard synced to PC" else "No text/link/image in clipboard",
                                        Toast.LENGTH_SHORT,
                                    ).show()
                                }
                            }
                            .padding(horizontal = 12.dp, vertical = 6.dp),
                    ) {
                        Text("Sync Now", style = MaterialTheme.typography.labelMedium, color = cs.onSurface)
                    }
                }
            }
        }

        Spacer(Modifier.height(16.dp))
    }
}
