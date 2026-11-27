package com.inex.nexuscompanion

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.content.pm.ActivityInfo
import android.graphics.Rect
import android.os.Build
import android.os.SystemClock
import android.view.HapticFeedbackConstants
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.pointer.PointerId
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import kotlinx.coroutines.delay
import kotlinx.coroutines.withTimeoutOrNull
import kotlin.math.abs

private const val MOVE_GAIN = 1.4f
private const val MOVE_ACCEL = 0.12f
private const val MOVE_GAIN_MAX = 4.2f
private const val SCROLL_GAIN = 0.6f           // Smooth, normal-speed two finger scrolling
private const val NATURAL_SCROLL = true
private const val TAP_MAX_MS = 320L
private const val HOLD_MS = 350L              // finger held still this long => mouse-down (drag)

private tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}

@Composable
fun TrackpadScreen(
    net: Net,
    speed: Float,
    enabled: Boolean,
    locked: Boolean,
    autoKeyboard: Boolean = false,
    onLockedChange: (Boolean) -> Unit,
    onToggle: () -> Unit,
) {
    val view = LocalView.current
    val ctx = LocalContext.current
    val userSpeed by rememberUpdatedState(speed)
    val chip = Color(0xFF1C1C1E)

    var keyboardOpen by remember { mutableStateOf(false) }
    var textInput by remember { mutableStateOf("") }
    val focusRequester = remember { FocusRequester() }
    val keyboardController = LocalSoftwareKeyboardController.current

    // Auto-open keyboard when user clicks text box on PC (and auto-close on enter or leave)
    DisposableEffect(autoKeyboard) {
        if (autoKeyboard) {
            net.onAutoKeyboard = { open ->
                keyboardOpen = open
            }
        } else {
            net.onAutoKeyboard = null
        }
        onDispose { net.onAutoKeyboard = null }
    }

    DisposableEffect(Unit) {
        view.keepScreenOn = true
        onDispose {
            view.keepScreenOn = false
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) view.systemGestureExclusionRects = emptyList()
        }
    }

    // Child lock: immersive (bars hidden) + screen pinning (blocks home/recents/back navigation) strictly portrait
    DisposableEffect(locked) {
        val act = ctx.findActivity()
        if (act != null) {
            act.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
            if (locked) {
                val c = WindowCompat.getInsetsController(act.window, view)
                c.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
                c.hide(WindowInsetsCompat.Type.systemBars())
                try { act.startLockTask() } catch (_: Exception) {}
            }
        }
        onDispose {
            if (locked && act != null) {
                try { act.stopLockTask() } catch (_: Exception) {}
                act.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
                WindowCompat.getInsetsController(act.window, view).show(WindowInsetsCompat.Type.systemBars())
            }
        }
    }
    BackHandler(enabled = locked) { /* swallowed */ }

    // Auto-focus and show soft keyboard when keyboardOpen becomes true
    LaunchedEffect(keyboardOpen) {
        if (keyboardOpen) {
            delay(100)
            try {
                focusRequester.requestFocus()
                keyboardController?.show()
            } catch (_: Exception) {}
        } else {
            try {
                keyboardController?.hide()
            } catch (_: Exception) {}
        }
    }

    val doEnter = {
        net.key("enter")
        textInput = ""
        if (autoKeyboard) {
            keyboardOpen = false
            keyboardController?.hide()
        }
    }

    Column(Modifier.fillMaxSize().background(Color.Black)) {
        if (!locked) {
            Row(
                Modifier.fillMaxWidth().padding(start = 24.dp, end = 16.dp, bottom = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Touchpad", style = MaterialTheme.typography.titleSmall, color = Color.White.copy(alpha = 0.7f))
                Spacer(Modifier.weight(1f))

                // Keyboard toggle button
                Box(
                    Modifier.clip(RoundedCornerShape(20.dp))
                        .background(if (keyboardOpen) Color.White else chip)
                        .plainClick { keyboardOpen = !keyboardOpen }
                        .padding(horizontal = 14.dp, vertical = 8.dp),
                ) {
                    Text(
                        "⌨️ Keys",
                        style = MaterialTheme.typography.labelMedium,
                        fontWeight = if (keyboardOpen) FontWeight.SemiBold else FontWeight.Normal,
                        color = if (keyboardOpen) Color.Black else Color.White,
                    )
                }

                Spacer(Modifier.width(8.dp))

                // Lock button
                Box(
                    Modifier.clip(RoundedCornerShape(20.dp)).background(chip)
                        .plainClick {
                            onLockedChange(true)
                        }
                        .padding(horizontal = 14.dp, vertical = 8.dp),
                ) { Text("🔒 Lock", style = MaterialTheme.typography.labelMedium, color = Color.White) }

                Spacer(Modifier.width(8.dp))

                // On / Off button
                Box(
                    Modifier.clip(RoundedCornerShape(20.dp))
                        .background(if (enabled) Color.White else chip)
                        .plainClick(onToggle)
                        .padding(horizontal = 18.dp, vertical = 8.dp),
                ) {
                    Text(
                        if (enabled) "On" else "Off",
                        style = MaterialTheme.typography.labelLarge,
                        color = if (enabled) Color.Black else Color.White.copy(alpha = 0.6f),
                    )
                }
            }
        }

        // Keyboard panel (quick keys + input field) - available in both locked and unlocked modes!
        if (keyboardOpen) {
            Column(
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 4.dp)
                    .clip(RoundedCornerShape(16.dp))
                    .background(Color(0xFF141416))
                    .border(1.dp, Color(0xFF2C2C2E), RoundedCornerShape(16.dp))
                    .padding(10.dp)
            ) {
                // Quick keys row (essential PC navigation keys not found on Android keyboards)
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    val quickKeys = listOf(
                        "Esc" to { net.key("escape") },
                        "Tab" to { net.key("tab") },
                        "↑" to { net.key("up") },
                        "↓" to { net.key("down") },
                        "←" to { net.key("left") },
                        "→" to { net.key("right") },
                    )
                    quickKeys.forEach { (lbl, action) ->
                        Box(
                            Modifier.clip(RoundedCornerShape(8.dp))
                                .background(Color(0xFF242426))
                                .plainClick {
                                    view.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)
                                    action()
                                }
                                .padding(horizontal = 10.dp, vertical = 6.dp),
                            contentAlignment = Alignment.Center,
                        ) {
                            Text(lbl, style = MaterialTheme.typography.labelSmall, color = Color.White)
                        }
                    }
                    Spacer(Modifier.weight(1f))
                    Box(
                        Modifier.size(28.dp).clip(CircleShape).background(Color(0xFF2C2C2E))
                            .plainClick {
                                keyboardOpen = false
                                keyboardController?.hide()
                            },
                        contentAlignment = Alignment.Center,
                    ) { Text("✕", fontSize = 12.sp, color = Color.White.copy(alpha = 0.7f)) }
                }

                Spacer(Modifier.height(8.dp))

                // Real-time text input field with Enter button
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(10.dp))
                        .background(Color(0xFF1E1E20))
                        .border(1.dp, Color(0xFF2C2C2E), RoundedCornerShape(10.dp))
                        .padding(horizontal = 12.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(Modifier.weight(1f)) {
                        if (textInput.isEmpty()) {
                            Text("Type here (sends keys live to PC)...", style = MaterialTheme.typography.bodySmall, color = Color.Gray)
                        }
                        BasicTextField(
                            value = textInput,
                            onValueChange = { newText ->
                                val old = textInput
                                var prefix = 0
                                val maxPrefix = minOf(old.length, newText.length)
                                while (prefix < maxPrefix && old[prefix] == newText[prefix]) {
                                    prefix++
                                }
                                if (prefix > 0 && Character.isHighSurrogate(old[prefix - 1])) {
                                    prefix--
                                }
                                val delCount = if (prefix < old.length) {
                                    Character.codePointCount(old, prefix, old.length)
                                } else 0
                                repeat(delCount) { net.key("backspace") }
                                if (prefix < newText.length) {
                                    net.text(newText.substring(prefix))
                                }
                                textInput = newText
                            },
                            textStyle = TextStyle(color = Color.White, fontSize = 14.sp),
                            cursorBrush = SolidColor(Color.White),
                            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                            keyboardActions = KeyboardActions(onSend = { doEnter() }),
                            modifier = Modifier.fillMaxWidth().focusRequester(focusRequester),
                        )
                    }
                    Spacer(Modifier.width(8.dp))
                    Box(
                        Modifier
                            .clip(RoundedCornerShape(8.dp))
                            .background(if (textInput.isNotEmpty()) Color.White else Color(0xFF2C2C2E))
                            .plainClick { doEnter() }
                            .padding(horizontal = 12.dp, vertical = 6.dp),
                    ) {
                        Text(
                            "Enter ↵",
                            style = MaterialTheme.typography.labelSmall,
                            fontWeight = FontWeight.SemiBold,
                            color = if (textInput.isNotEmpty()) Color.Black else Color.Gray,
                        )
                    }
                }
            }
        }

        Box(
            Modifier.fillMaxWidth().weight(1f).onSizeChanged { size ->
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    // Screen pinning is the real lock; exclusion rect is best effort
                    view.systemGestureExclusionRects =
                        if (locked) listOf(Rect(0, 0, size.width, size.height)) else emptyList()
                }
            }
        ) {
            // ── Pitch-black glass pad: proven original gesture engine with tuned scroll ──
            Box(
                Modifier
                    .fillMaxSize()
                    .background(Color.Black)
                    .pointerInput(enabled) {
                        if (!enabled) return@pointerInput
                        val slop = viewConfiguration.touchSlop
                        val swipeMin = 60.dp.toPx()
                        val dens = density

                        awaitEachGesture {
                            val downMap = mutableMapOf<PointerId, Offset>()
                            var t0 = 0L
                            var endT = 0L
                            var maxFingers = 0
                            var moved = false
                            var isScrolling = false
                            var dragging = false
                            var swipe = Offset.Zero

                            try {
                                while (true) {
                                    // 1 finger held still => wake up after HOLD_MS and start dragging
                                    val holdArmed = t0 != 0L && maxFingers == 1 && !moved && !dragging
                                    val event = if (holdArmed) {
                                        val left = HOLD_MS - (SystemClock.uptimeMillis() - t0)
                                        if (left <= 0) null else withTimeoutOrNull(left) { awaitPointerEvent() }
                                    } else awaitPointerEvent()

                                    if (event == null) {
                                        dragging = true
                                        net.down("left")
                                        view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
                                        continue
                                    }

                                    val pressed = event.changes.filter { it.pressed }
                                    if (pressed.isEmpty()) {
                                        endT = event.changes.firstOrNull()?.uptimeMillis ?: SystemClock.uptimeMillis()
                                        event.changes.forEach { it.consume() }
                                        break
                                    }
                                    if (t0 == 0L) t0 = pressed.first().uptimeMillis
                                    maxFingers = maxOf(maxFingers, pressed.size)

                                    // Second finger while dragging => drop the item, continue as scroll/gesture
                                    if (dragging && pressed.size >= 2) {
                                        dragging = false
                                        net.up("left")
                                    }

                                    for (p in pressed) if (!downMap.containsKey(p.id)) downMap[p.id] = p.position
                                    for (p in pressed) {
                                        if ((p.position - (downMap[p.id] ?: p.position)).getDistance() > slop) moved = true
                                    }

                                    when (pressed.size) {
                                        1 -> {
                                            val delta = pressed[0].positionChange()
                                            if ((moved || dragging) && delta != Offset.Zero) {
                                                val dp = Offset(delta.x / dens, delta.y / dens)
                                                val g = (MOVE_GAIN + dp.getDistance() * MOVE_ACCEL)
                                                    .coerceAtMost(MOVE_GAIN_MAX) * userSpeed
                                                net.move(dp.x * g, dp.y * g)
                                            }
                                        }
                                        2 -> {
                                            val avg = (pressed[0].positionChange() + pressed[1].positionChange()) / 2f
                                            if (moved && avg != Offset.Zero) {
                                                val dp = Offset(avg.x / dens, avg.y / dens)
                                                val sx = if (NATURAL_SCROLL) -dp.x else dp.x
                                                val sy = if (NATURAL_SCROLL) -dp.y else dp.y
                                                net.scroll(sx * SCROLL_GAIN, sy * SCROLL_GAIN)
                                                isScrolling = true
                                            }
                                        }
                                        3 -> {
                                            swipe += Offset(
                                                pressed.map { it.positionChange().x }.average().toFloat(),
                                                pressed.map { it.positionChange().y }.average().toFloat(),
                                            )
                                        }
                                    }
                                    event.changes.forEach { it.consume() }
                                }

                                val duration = endT - t0
                                when {
                                    dragging -> { /* release handled in finally */ }
                                    !moved && duration < TAP_MAX_MS -> when (maxFingers) {
                                        1 -> net.click("left")
                                        2 -> net.click("right")
                                    }
                                    maxFingers >= 3 && moved -> {
                                        val ax = abs(swipe.x); val ay = abs(swipe.y)
                                        if (maxOf(ax, ay) > swipeMin) {
                                            net.gesture(
                                                if (ay >= ax) { if (swipe.y < 0) "swipe_up" else "swipe_down" }
                                                else { if (swipe.x < 0) "swipe_left" else "swipe_right" }
                                            )
                                        }
                                    }
                                    isScrolling -> net.scrollEnd()
                                }
                            } finally {
                                if (dragging) {
                                    net.up("left")
                                    net.up("left") // twice: UDP can drop one packet
                                }
                            }
                        }
                    }
            )

            // ── Floating controls in locked mode ──
            if (!enabled) {
                Text(
                    "Touchpad is Off · Tap 'Off' above to turn on",
                    style = MaterialTheme.typography.labelMedium,
                    color = Color.White.copy(alpha = 0.26f),
                    modifier = Modifier.align(Alignment.Center),
                )
            }

            if (locked) {
                Row(
                    Modifier
                        .align(Alignment.TopEnd)
                        .padding(16.dp),
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    // Floating keyboard toggle button in locked mode
                    Box(
                        Modifier
                            .size(46.dp)
                            .clip(CircleShape)
                            .background(if (keyboardOpen) Color.White else Color.White.copy(alpha = 0.18f))
                            .plainClick { keyboardOpen = !keyboardOpen },
                        contentAlignment = Alignment.Center,
                    ) {
                        Text("⌨️", fontSize = 18.sp)
                    }

                    // Floating lock icon: double-tap to unlock
                    Box(
                        Modifier
                            .size(46.dp)
                            .clip(CircleShape)
                            .background(Color.White.copy(alpha = 0.18f))
                            .pointerInput(Unit) {
                                detectTapGestures(onDoubleTap = { onLockedChange(false) })
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Text("🔒", fontSize = 18.sp)
                    }
                }
            }
        }
    }
}
