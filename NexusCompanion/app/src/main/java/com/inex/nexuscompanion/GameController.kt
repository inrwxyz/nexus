package com.inex.nexuscompanion

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.content.pm.ActivityInfo
import android.graphics.Rect
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.os.Build
import android.view.HapticFeedbackConstants
import android.widget.Toast
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlin.math.atan2
import kotlin.math.hypot
import kotlin.math.roundToInt

private tailrec fun Context.findHostActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findHostActivity()
    else -> null
}

private fun vkToBitmask(vk: Int): Int = when (vk) {
    'W'.code -> 1 shl 0
    'S'.code -> 1 shl 1
    'A'.code -> 1 shl 2
    'D'.code -> 1 shl 3
    0xA0 -> 1 shl 4
    0x20 -> 1 shl 5
    0xF1 -> 1 shl 6
    0xF2 -> 1 shl 7
    'R'.code -> 1 shl 8
    'C'.code -> 1 shl 9
    'E'.code -> 1 shl 10
    'F'.code -> 1 shl 11
    'V'.code -> 1 shl 12
    'Q'.code -> 1 shl 13
    0x1B -> 1 shl 14
    'G'.code -> 1 shl 15
    else -> 0
}

@Composable
fun GameControllerScreen(
    store: Store,
    net: Net,
    onExit: () -> Unit,
) {
    val ctx = LocalContext.current
    val view = LocalView.current
    val scope = rememberCoroutineScope()

    var hudLocked by remember { mutableStateOf(false) }

    DisposableEffect(Unit) {
        val act = ctx.findHostActivity()
        view.keepScreenOn = true
        if (act != null) {
            act.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
            val c = WindowCompat.getInsetsController(act.window, view)
            c.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            c.hide(WindowInsetsCompat.Type.systemBars())
        }
        onDispose {
            net.sendGamepadFrame(0, 0, 0f, 0f, 0, 0, IntArray(4))
            net.onGamepadProfile = null
            view.keepScreenOn = false
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) view.systemGestureExclusionRects = emptyList()
            if (act != null) {
                try { act.stopLockTask() } catch (_: Exception) {}
                act.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
                WindowCompat.getInsetsController(act.window, view).show(WindowInsetsCompat.Type.systemBars())
            }
        }
    }
    BackHandler { if (!hudLocked) onExit() }

    DisposableEffect(hudLocked) {
        val act = ctx.findHostActivity()
        if (act != null && hudLocked) {
            val c = WindowCompat.getInsetsController(act.window, view)
            c.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            c.hide(WindowInsetsCompat.Type.systemBars())
            try { act.startLockTask() } catch (_: Exception) {}
        }
        onDispose {
            if (hudLocked && act != null) {
                try { act.stopLockTask() } catch (_: Exception) {}
            }
        }
    }

    var pcGamepadEnabled by remember { mutableStateOf(net.gamepadConfig.enabled) }
    var detectedGameName by remember { mutableStateOf<String?>(null) }
    var profiles by remember { mutableStateOf(GameProfileStore.load(store.gpProfilesJson)) }
    var activeProfileId by remember { mutableStateOf(store.gpActiveProfileId) }
    val activeProfile = profiles.firstOrNull { it.id == activeProfileId } ?: profiles.first()

    var gyroOn by remember(activeProfile.id) { mutableStateOf(activeProfile.gyroEnabled) }
    var editMode by remember { mutableStateOf(false) }
    var selectedItemId by remember { mutableStateOf<String?>(null) }
    var profileMenuOpen by remember { mutableStateOf(false) }
    var keyMenuOpen by remember { mutableStateOf(false) }
    var shapeMenuOpen by remember { mutableStateOf(false) }
    var saveDialogOpen by remember { mutableStateOf(false) }
    var renameLabelOpen by remember { mutableStateOf(false) }

    fun saveLocal(next: List<GameProfile>, selectId: String) {
        profiles = next
        activeProfileId = selectId
        store.gpActiveProfileId = selectId
        store.gpProfilesJson = GameProfileStore.serialize(next)
    }

    fun syncFromPc(requestedId: String? = null) {
        scope.launch {
            net.status()
            pcGamepadEnabled = net.gamepadConfig.enabled
            val remote = net.fetchGamepadProfiles()
            if (remote != null && remote.profiles.isNotEmpty()) {
                val targetId = requestedId ?: remote.activeProfileId
                val picked = remote.profiles.firstOrNull { it.id == targetId } ?: remote.profiles.first()
                saveLocal(remote.profiles, picked.id)
                gyroOn = picked.gyroEnabled
                detectedGameName = remote.detectedGame
            }
        }
    }

    // Initial sync with PC + live UDP push listener (PC profile picker / auto game detection)
    LaunchedEffect(Unit) {
        syncFromPc()
    }
    DisposableEffect(net) {
        net.onGamepadProfile = { pushedId ->
            if (!editMode) {
                val localMatch = profiles.firstOrNull { it.id == pushedId }
                if (localMatch != null) {
                    activeProfileId = pushedId
                    store.gpActiveProfileId = pushedId
                    gyroOn = localMatch.gyroEnabled
                }
                syncFromPc(pushedId)
            }
        }
        onDispose { net.onGamepadProfile = null }
    }

    fun updateActiveProfile(transform: (GameProfile) -> GameProfile) {
        saveLocal(profiles.map { if (it.id == activeProfile.id) transform(it) else it }, activeProfile.id)
    }

    fun updateItem(id: String, transform: (GameHudItem) -> GameHudItem) {
        updateActiveProfile { p -> p.copy(items = p.items.map { if (it.id == id) transform(it) else it }) }
    }

    fun commitSaveCurrent(exitEdit: Boolean) {
        val updated = activeProfile.copy(gyroEnabled = gyroOn)
        val nextList = profiles.map { if (it.id == updated.id) updated else it }
        saveLocal(nextList, updated.id)
        scope.launch {
            val remote = net.saveGamepadProfile(updated, makeActive = true)
            if (remote != null && remote.profiles.isNotEmpty()) {
                saveLocal(remote.profiles, remote.activeProfileId)
            }
            Toast.makeText(ctx, "Saved \"${updated.name}\" on Phone & PC", Toast.LENGTH_SHORT).show()
        }
        if (exitEdit) {
            editMode = false
            selectedItemId = null
        }
    }

    val heldIds = remember { mutableStateListOf<String>() }
    var joyX by remember { mutableFloatStateOf(0f) }
    var joyY by remember { mutableFloatStateOf(0f) }
    var gyroTilt by remember { mutableFloatStateOf(0f) }
    var gyroZeroOffset by remember { mutableFloatStateOf(0f) }
    var rawRollRad by remember { mutableFloatStateOf(0f) }

    val aimPendingX = remember { floatArrayOf(0f) }
    val aimPendingY = remember { floatArrayOf(0f) }

    // Gyro/Tilt accelerometer steering
    DisposableEffect(gyroOn, editMode, pcGamepadEnabled) {
        if (!gyroOn || editMode || !pcGamepadEnabled) {
            gyroTilt = 0f
            onDispose { }
        } else {
            val sm = ctx.getSystemService(Context.SENSOR_SERVICE) as? SensorManager
            val sensor = sm?.getDefaultSensor(Sensor.TYPE_ACCELEROMETER)
            val listener = object : SensorEventListener {
                override fun onSensorChanged(ev: SensorEvent) {
                    val ax = ev.values[0]
                    val ay = ev.values[1]
                    val az = ev.values[2]
                    val roll = atan2(ay.toDouble(), hypot(ax.toDouble(), az.toDouble())).toFloat()
                    rawRollRad = roll
                    val sens = net.gamepadConfig.gyroSensitivity.coerceIn(0.4f, 3.0f)
                    val delta = (roll - gyroZeroOffset) * sens * 1.45f
                    val dz = net.gamepadConfig.gyroDeadzone.coerceIn(0.02f, 0.35f)
                    val clamped = delta.coerceIn(-1f, 1f)
                    val smoothed = if (kotlin.math.abs(clamped) < dz) 0f else clamped
                    gyroTilt = gyroTilt * 0.55f + smoothed * 0.45f
                }

                override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) {}
            }
            if (sm != null && sensor != null) {
                sm.registerListener(listener, sensor, SensorManager.SENSOR_DELAY_GAME)
            }
            onDispose {
                sm?.unregisterListener(listener)
                gyroTilt = 0f
            }
        }
    }

    // ~90 Hz binary UDP transmitter + heartbeat keepalive
    LaunchedEffect(activeProfile.id, editMode, pcGamepadEnabled) {
        if (!pcGamepadEnabled || editMode) {
            net.sendGamepadFrame(0, 0, 0f, 0f, 0, 0, IntArray(4))
            return@LaunchedEffect
        }
        var lastSig = ""
        var lastSendAt = 0L
        while (true) {
            var mask = 0
            val extras = IntArray(4)
            var extraIdx = 0

            for (id in heldIds.toList()) {
                val item = activeProfile.items.firstOrNull { it.id == id } ?: continue
                val vk = item.vk
                if (vk != 0) {
                    val bit = vkToBitmask(vk)
                    if (bit != 0) mask = mask or bit
                    else if (extraIdx < 4) extras[extraIdx++] = vk
                }
            }

            val outLx = if (gyroOn) {
                (if (kotlin.math.abs(joyX) > 0.05f) joyX else gyroTilt).coerceIn(-1f, 1f)
            } else {
                joyX.coerceIn(-1f, 1f)
            }
            val outLy = joyY.coerceIn(-1f, 1f)

            val adx = aimPendingX[0].roundToInt()
            val ady = aimPendingY[0].roundToInt()
            aimPendingX[0] -= adx
            aimPendingY[0] -= ady

            val flags = if (gyroOn) 0x01 else 0x00
            val activeNow = mask != 0 || extraIdx > 0 || adx != 0 || ady != 0 ||
                kotlin.math.abs(outLx) > 0.04f || kotlin.math.abs(outLy) > 0.04f

            val sig = "$flags|$mask|${(outLx * 40).roundToInt()}|${(outLy * 40).roundToInt()}|${extras.joinToString(",")}"
            val now = System.currentTimeMillis()
            if (adx != 0 || ady != 0 || sig != lastSig || (activeNow && now - lastSendAt >= 120L)) {
                net.sendGamepadFrame(flags, mask, outLx, outLy, adx, ady, extras)
                lastSig = sig
                lastSendAt = now
            }
            delay(11L)
        }
    }

    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black)
            .onSizeChanged { sz ->
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    view.systemGestureExclusionRects =
                        if (hudLocked) listOf(Rect(0, 0, sz.width, sz.height)) else emptyList()
                }
            },
    ) {
        if (!pcGamepadEnabled) {
            Column(
                Modifier.fillMaxSize().padding(32.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
            ) {
                Text("Game Controller is Turned Off", color = Color.White, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.height(8.dp))
                Text(
                    "Enable Game Controller inside Nexus Settings → Gamepad → System on your PC.",
                    color = Color(0xFF9BA1AC),
                    style = MaterialTheme.typography.bodyMedium,
                )
                Spacer(Modifier.height(20.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Button(onClick = { syncFromPc() }) { Text("Retry") }
                    OutlinedButton(onClick = onExit) { Text("Exit") }
                }
            }
            return@Box
        }

        val currentProfileState by rememberUpdatedState(activeProfile)

        BoxWithConstraints(
            Modifier
                .fillMaxSize()
                // Fixed-coordinate drag & selection in Edit Mode -> ZERO oscillation or jumping!
                .then(
                    if (editMode) {
                        Modifier.pointerInput(editMode) {
                            awaitEachGesture {
                                val down = awaitFirstDown(requireUnconsumed = false)
                                val w = size.width.toFloat().coerceAtLeast(1f)
                                val h = size.height.toFloat().coerceAtLeast(1f)
                                // Avoid hijacking top pill bar or bottom inspector bar
                                if (down.position.y < 52.dp.toPx() || down.position.y > h - 68.dp.toPx()) {
                                    return@awaitEachGesture
                                }
                                val prof = currentProfileState
                                val hit = prof.items.minByOrNull { item ->
                                    hypot(down.position.x - item.nx * w, down.position.y - item.ny * h)
                                }
                                val hitDist = hit?.let {
                                    hypot(down.position.x - it.nx * w, down.position.y - it.ny * h)
                                } ?: Float.MAX_VALUE
                                val maxHitR = hit?.let { (hudItemWidthDp(it).toPx().coerceAtLeast(hudItemHeightDp(it).toPx()) / 2f) + 22.dp.toPx() } ?: 0f

                                if (hit == null || hitDist > maxHitR) {
                                    selectedItemId = null
                                    return@awaitEachGesture
                                }

                                selectedItemId = hit.id
                                val startNx = hit.nx
                                val startNy = hit.ny
                                val startPos = down.position

                                while (true) {
                                    val ev = awaitPointerEvent()
                                    val ch = ev.changes.firstOrNull { it.id == down.id } ?: break
                                    if (!ch.pressed) break
                                    ch.consume()
                                    val dx = ch.position.x - startPos.x
                                    val dy = ch.position.y - startPos.y
                                    updateItem(hit.id) { cur ->
                                        cur.copy(
                                            nx = (startNx + dx / w).coerceIn(0.06f, 0.94f),
                                            ny = (startNy + dy / h).coerceIn(0.14f, 0.88f),
                                        )
                                    }
                                }
                            }
                        }
                    } else {
                        Modifier.pointerInput(activeProfile.id) {
                            awaitEachGesture {
                                val down = awaitFirstDown(requireUnconsumed = false)
                                val w = size.width.toFloat().coerceAtLeast(1f)
                                if (down.position.x < w * 0.44f) return@awaitEachGesture
                                val pid = down.id
                                while (true) {
                                    val ev = awaitPointerEvent(PointerEventPass.Final)
                                    val ch = ev.changes.firstOrNull { it.id == pid } ?: break
                                    if (!ch.pressed) break
                                    if (!ch.isConsumed) {
                                        val d = ch.positionChange()
                                        if (d != Offset.Zero) {
                                            aimPendingX[0] += d.x * 1.15f
                                            aimPendingY[0] += d.y * 1.15f
                                        }
                                    }
                                }
                            }
                        }
                    },
                ),
        ) {
            val density = LocalDensity.current
            val areaW = constraints.maxWidth.toFloat().coerceAtLeast(1f)
            val areaH = constraints.maxHeight.toFloat().coerceAtLeast(1f)

            if (editMode) {
                Canvas(Modifier.fillMaxSize()) {
                    for (i in 1 until 12) {
                        val x = size.width * i / 12f
                        drawLine(Color.White.copy(alpha = 0.06f), Offset(x, 0f), Offset(x, size.height), 1f)
                    }
                    for (j in 1 until 6) {
                        val y = size.height * j / 6f
                        drawLine(Color.White.copy(alpha = 0.06f), Offset(0f, y), Offset(size.width, y), 1f)
                    }
                }
            } else {
                Text(
                    buildString {
                        if (detectedGameName != null) append("$detectedGameName  ·  ")
                        append(if (gyroOn) "Tilt phone to steer  ·  Drag right to look" else "Drag right half to look / aim")
                    },
                    color = Color.White.copy(alpha = 0.28f),
                    fontSize = 11.sp,
                    modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 8.dp),
                )
            }

            activeProfile.items.forEach { item ->
                val isSelected = editMode && selectedItemId == item.id
                val wDp = hudItemWidthDp(item)
                val hDp = hudItemHeightDp(item)
                val wPx = with(density) { wDp.toPx() }
                val hPx = with(density) { hDp.toPx() }
                val offset = IntOffset((item.nx * areaW - wPx / 2f).roundToInt(), (item.ny * areaH - hPx / 2f).roundToInt())

                if (item.kind == "joystick") {
                    JoystickControl(
                        item = item,
                        sizeDp = wDp,
                        offsetPx = offset,
                        editMode = editMode,
                        selected = isSelected,
                        onStick = { x, y -> joyX = x; joyY = y },
                    )
                } else {
                    val pressed = heldIds.contains(item.id)
                    StyledHudButton(
                        item = item,
                        widthDp = wDp,
                        heightDp = hDp,
                        offsetPx = offset,
                        editMode = editMode,
                        selected = isSelected,
                        pressed = pressed,
                        onPressChange = { down ->
                            if (down) {
                                if (!heldIds.contains(item.id)) heldIds.add(item.id)
                                if (net.gamepadConfig.haptics) {
                                    view.performHapticFeedback(HapticFeedbackConstants.VIRTUAL_KEY)
                                }
                            } else {
                                heldIds.remove(item.id)
                            }
                        },
                    )
                }
            }

            // Compact Top Bar (hidden when HUD locked just like Trackpad)
            if (!hudLocked) {
                Row(
                    Modifier
                        .align(Alignment.TopCenter)
                        .padding(top = 8.dp)
                        .clip(CircleShape)
                        .background(Color(0xD8151820))
                        .border(1.dp, Color.White.copy(alpha = 0.14f), CircleShape)
                        .padding(horizontal = 10.dp, vertical = 5.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Box {
                        TopPillChip("${activeProfile.name} ▾", highlight = false) { profileMenuOpen = true }
                        DropdownMenu(expanded = profileMenuOpen, onDismissRequest = { profileMenuOpen = false }) {
                            profiles.forEach { p ->
                                DropdownMenuItem(
                                    text = { Text(p.name) },
                                    onClick = {
                                        profileMenuOpen = false
                                        heldIds.clear()
                                        joyX = 0f; joyY = 0f
                                        selectedItemId = null
                                        activeProfileId = p.id
                                        store.gpActiveProfileId = p.id
                                        gyroOn = p.gyroEnabled
                                        scope.launch { net.selectGamepadProfileOnPc(p.id) }
                                    },
                                )
                            }
                        }
                    }

                    TopPillChip(if (gyroOn) "Gyro ON" else "Gyro OFF", highlight = gyroOn) {
                        gyroOn = !gyroOn
                        if (gyroOn) gyroZeroOffset = rawRollRad
                        updateActiveProfile { it.copy(gyroEnabled = gyroOn) }
                    }
                    if (gyroOn) {
                        TopPillChip("Zero", highlight = false) { gyroZeroOffset = rawRollRad }
                    }

                    TopPillChip(if (editMode) "Cancel Edit" else "Edit HUD", highlight = editMode) {
                        editMode = !editMode
                        heldIds.clear()
                        joyX = 0f; joyY = 0f
                        if (!editMode) {
                            selectedItemId = null
                            syncFromPc(activeProfile.id)
                        }
                    }

                    if (editMode) {
                        TopPillChip("Save", highlight = false, accentGreen = true) {
                            commitSaveCurrent(exitEdit = true)
                        }
                    } else {
                        TopPillChip("🔒 Lock", highlight = false) {
                            hudLocked = true
                            Toast.makeText(ctx, "Locked · Double-tap 🔒 to unlock", Toast.LENGTH_SHORT).show()
                        }
                        TopPillChip("Exit", highlight = false, danger = true, onClick = onExit)
                    }
                }
            } else {
                // Floating lock badge in locked mode: double-tap to unlock (same as Trackpad)
                Box(
                    Modifier
                        .align(Alignment.TopEnd)
                        .padding(14.dp)
                        .size(44.dp)
                        .clip(CircleShape)
                        .background(Color.White.copy(alpha = 0.16f))
                        .pointerInput(Unit) {
                            detectTapGestures(onDoubleTap = { hudLocked = false })
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Text("🔒", fontSize = 17.sp)
                }
            }

            if (gyroOn && !editMode) {
                SteeringWheelGraphic(
                    tilt = gyroTilt,
                    modifier = Modifier.align(Alignment.TopCenter).padding(top = 46.dp),
                )
            }

            // Bottom Editor Inspector Bar
            if (editMode) {
                val sel = activeProfile.items.firstOrNull { it.id == selectedItemId }
                Row(
                    Modifier
                        .align(Alignment.BottomCenter)
                        .padding(bottom = 10.dp, start = 14.dp, end = 14.dp)
                        .clip(RoundedCornerShape(20.dp))
                        .background(Color(0xEA151820))
                        .border(1.dp, Color.White.copy(alpha = 0.18f), RoundedCornerShape(20.dp))
                        .padding(horizontal = 12.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    if (sel != null) {
                        TopPillChip(sel.label, highlight = false) { renameLabelOpen = true }
                        if (sel.kind == "button") {
                            Box {
                                TopPillChip("Key: ${shortKeyTag(sel.vk)} ▾", highlight = true) { keyMenuOpen = true }
                                DropdownMenu(expanded = keyMenuOpen, onDismissRequest = { keyMenuOpen = false }) {
                                    KEY_BIND_OPTIONS.forEach { opt ->
                                        DropdownMenuItem(
                                            text = { Text(opt.label) },
                                            onClick = {
                                                keyMenuOpen = false
                                                updateItem(sel.id) { it.copy(vk = opt.vk) }
                                            },
                                        )
                                    }
                                }
                            }
                            Box {
                                TopPillChip("Shape: ${sel.shape} ▾", highlight = false) { shapeMenuOpen = true }
                                DropdownMenu(expanded = shapeMenuOpen, onDismissRequest = { shapeMenuOpen = false }) {
                                    HUD_SHAPE_OPTIONS.forEach { sh ->
                                        DropdownMenuItem(
                                            text = { Text(sh) },
                                            onClick = {
                                                shapeMenuOpen = false
                                                updateItem(sel.id) { it.copy(shape = sh) }
                                            },
                                        )
                                    }
                                }
                            }
                        }
                        Text("Size", color = Color(0xFF9BA1AC), fontSize = 11.sp)
                        Slider(
                            value = sel.scale,
                            onValueChange = { s -> updateItem(sel.id) { it.copy(scale = s) } },
                            valueRange = 0.65f..1.80f,
                            modifier = Modifier.width(96.dp).height(26.dp),
                        )
                        if (activeProfile.items.size > 1) {
                            TopPillChip("Del", highlight = false, danger = true) {
                                val next = activeProfile.items.filterNot { it.id == sel.id }
                                selectedItemId = next.firstOrNull()?.id
                                updateActiveProfile { it.copy(items = next) }
                            }
                        }
                    } else {
                        Text("Drag any control cleanly or tap to edit", color = Color(0xFF9BA1AC), fontSize = 11.5.sp)
                    }

                    Spacer(Modifier.width(4.dp))
                    TopPillChip("+ Add", highlight = true) {
                        val id = "btn_custom_${System.currentTimeMillis() % 10000}"
                        val newItem = GameHudItem(id, "button", "circle", "BTN", 0.50f, 0.50f, 1.0f, 0.90f, 'F'.code, 0xFF334155)
                        updateActiveProfile { it.copy(items = it.items + newItem) }
                        selectedItemId = id
                    }
                    TopPillChip("Save As", highlight = false, accentGreen = true) { saveDialogOpen = true }
                    if (activeProfile.id.startsWith("preset_")) {
                        TopPillChip("Reset", highlight = false) {
                            val def = builtInPresets().firstOrNull { it.id == activeProfile.id }
                            if (def != null) {
                                updateActiveProfile { def }
                                gyroOn = def.gyroEnabled
                                scope.launch { net.saveGamepadProfile(def, makeActive = true) }
                                Toast.makeText(ctx, "Reset \"${def.name}\" to default", Toast.LENGTH_SHORT).show()
                            }
                        }
                    }
                }
            }
        }
    }

    if (saveDialogOpen) {
        var name by remember { mutableStateOf("") }
        AlertDialog(
            onDismissRequest = { saveDialogOpen = false },
            title = { Text("Save Profile to Phone & PC") },
            text = {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text("Game name (e.g. GG, GTA 5, NFS)") },
                    singleLine = true,
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    val clean = name.trim()
                    if (clean.isNotEmpty()) {
                        val newId = "custom_${System.currentTimeMillis()}"
                        val clone = activeProfile.copy(
                            id = newId,
                            name = clean,
                            category = "custom",
                            description = "Custom layout",
                            gyroEnabled = gyroOn,
                        )
                        saveLocal(profiles + clone, newId)
                        scope.launch {
                            val remote = net.saveGamepadProfile(clone, makeActive = true)
                            if (remote != null && remote.profiles.isNotEmpty()) {
                                saveLocal(remote.profiles, remote.activeProfileId)
                            }
                            Toast.makeText(ctx, "Saved \"$clean\" to Phone & PC", Toast.LENGTH_SHORT).show()
                        }
                        editMode = false
                        selectedItemId = null
                    }
                    saveDialogOpen = false
                }) { Text("Save") }
            },
            dismissButton = {
                TextButton(onClick = { saveDialogOpen = false }) { Text("Cancel") }
            },
        )
    }

    if (renameLabelOpen && selectedItemId != null) {
        val current = activeProfile.items.firstOrNull { it.id == selectedItemId }
        var label by remember(current?.id) { mutableStateOf(current?.label ?: "") }
        AlertDialog(
            onDismissRequest = { renameLabelOpen = false },
            title = { Text("Button Label") },
            text = {
                OutlinedTextField(
                    value = label,
                    onValueChange = { label = it.take(10) },
                    label = { Text("Label (max 10 chars)") },
                    singleLine = true,
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    val clean = label.trim().ifEmpty { "BTN" }
                    updateItem(selectedItemId!!) { it.copy(label = clean) }
                    renameLabelOpen = false
                }) { Text("OK") }
            },
            dismissButton = {
                TextButton(onClick = { renameLabelOpen = false }) { Text("Cancel") }
            },
        )
    }
}
