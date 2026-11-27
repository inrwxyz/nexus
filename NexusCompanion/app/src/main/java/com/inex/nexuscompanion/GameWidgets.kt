package com.inex.nexuscompanion

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.hypot
import kotlin.math.roundToInt

fun hudItemWidthDp(item: GameHudItem): Dp = when (item.shape) {
    "joystick" -> 116.dp * item.scale
    "pedal" -> 58.dp * item.scale
    "pill" -> 78.dp * item.scale
    else -> 64.dp * item.scale
}

fun hudItemHeightDp(item: GameHudItem): Dp = when (item.shape) {
    "joystick" -> 116.dp * item.scale
    "pedal" -> 92.dp * item.scale
    "pill" -> 44.dp * item.scale
    else -> 64.dp * item.scale
}

@Composable
fun TopPillChip(
    text: String,
    highlight: Boolean,
    danger: Boolean = false,
    accentGreen: Boolean = false,
    onClick: () -> Unit,
) {
    val bg = when {
        danger -> Color(0xFF421A1D)
        accentGreen -> Color(0xFF14532D)
        highlight -> Color(0xFF0EA5E9).copy(alpha = 0.25f)
        else -> Color.White.copy(alpha = 0.09f)
    }
    val fg = when {
        danger -> Color(0xFFFCA5A5)
        accentGreen -> Color(0xFF86EFAC)
        highlight -> Color(0xFF7DD3FC)
        else -> Color.White
    }
    Box(
        Modifier
            .clip(CircleShape)
            .background(bg)
            .clickable { onClick() }
            .padding(horizontal = 10.dp, vertical = 4.dp),
    ) {
        Text(
            text,
            color = fg,
            fontSize = 11.sp,
            fontWeight = FontWeight.Medium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
fun SteeringWheelGraphic(tilt: Float, modifier: Modifier = Modifier) {
    Box(
        modifier
            .size(62.dp)
            .graphicsLayer { rotationZ = tilt.coerceIn(-1f, 1f) * 52f },
        contentAlignment = Alignment.Center,
    ) {
        Canvas(Modifier.fillMaxSize()) {
            val r = size.minDimension * 0.42f
            val c = Offset(size.width / 2f, size.height / 2f)
            drawCircle(
                color = Color(0xFF1F242D),
                radius = r,
                center = c,
                style = Stroke(width = 7.dp.toPx()),
            )
            // Sport top stripe marker
            drawLine(
                color = Color(0xFF38BDF8),
                start = Offset(c.x, c.y - r - 3.dp.toPx()),
                end = Offset(c.x, c.y - r + 4.dp.toPx()),
                strokeWidth = 4.dp.toPx(),
                cap = StrokeCap.Round,
            )
            // Spokes
            drawLine(
                color = Color(0xFF475569),
                start = Offset(c.x - r + 4.dp.toPx(), c.y),
                end = Offset(c.x + r - 4.dp.toPx(), c.y),
                strokeWidth = 4.5.dp.toPx(),
                cap = StrokeCap.Round,
            )
            drawLine(
                color = Color(0xFF475569),
                start = c,
                end = Offset(c.x, c.y + r - 4.dp.toPx()),
                strokeWidth = 4.5.dp.toPx(),
                cap = StrokeCap.Round,
            )
            drawCircle(color = Color(0xFF0F172A), radius = 7.dp.toPx(), center = c)
            drawCircle(color = Color(0xFF38BDF8), radius = 2.5.dp.toPx(), center = c)
        }
    }
}

@Composable
fun StyledHudButton(
    item: GameHudItem,
    widthDp: Dp,
    heightDp: Dp,
    offsetPx: IntOffset,
    editMode: Boolean,
    selected: Boolean,
    pressed: Boolean,
    onPressChange: (Boolean) -> Unit,
) {
    val accent = Color(item.accentArgb)
    val shape = when (item.shape) {
        "pedal" -> RoundedCornerShape(14.dp)
        "pill" -> CircleShape
        else -> CircleShape
    }

    val brush = when {
        pressed -> Brush.radialGradient(listOf(accent.copy(alpha = 0.98f), accent.copy(alpha = 0.72f)))
        item.shape == "xbox" -> Brush.radialGradient(listOf(Color(0xFF262A34), Color(0xFF15181F)))
        item.shape == "pedal" -> Brush.verticalGradient(listOf(accent.copy(alpha = 0.70f), accent.copy(alpha = 0.35f)))
        else -> Brush.radialGradient(listOf(accent.copy(alpha = 0.68f), Color(0xFF12151C).copy(alpha = 0.78f)))
    }

    val borderCol = when {
        selected -> Color(0xFF38BDF8)
        pressed -> Color.White
        item.shape == "xbox" -> accent
        else -> Color.White.copy(alpha = 0.28f)
    }

    Box(
        Modifier
            .offset { offsetPx }
            .width(widthDp)
            .height(heightDp)
            .alpha(item.alpha)
            .clip(shape)
            .background(brush)
            .border(if (selected || pressed || item.shape == "xbox") 2.dp else 1.2.dp, borderCol, shape)
            .then(
                if (!editMode) {
                    Modifier.pointerInput(item.id) {
                        awaitEachGesture {
                            awaitFirstDown(requireUnconsumed = false)
                            onPressChange(true)
                            try {
                                do {
                                    val ev = awaitPointerEvent()
                                } while (ev.changes.any { it.pressed })
                            } finally {
                                onPressChange(false)
                            }
                        }
                    }
                } else {
                    Modifier
                },
            ),
        contentAlignment = Alignment.Center,
    ) {
        if (item.shape == "pedal") {
            // Racing pedal ribs + badge
            Column(
                Modifier.fillMaxSize().padding(vertical = 10.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.SpaceBetween,
            ) {
                repeat(3) {
                    Box(
                        Modifier
                            .width(widthDp * 0.56f)
                            .height(3.dp)
                            .clip(CircleShape)
                            .background(Color.White.copy(alpha = if (pressed) 0.72f else 0.28f)),
                    )
                }
                Text(
                    item.label,
                    color = Color.White,
                    fontSize = (12f * item.scale).coerceIn(11f, 16f).sp,
                    fontWeight = FontWeight.ExtraBold,
                )
                Spacer(Modifier.height(2.dp))
            }
        } else {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Text(
                    item.label,
                    color = if (item.shape == "xbox") accent.copy(alpha = 1f) else Color.White,
                    fontSize = when (item.shape) {
                        "xbox" -> (16f * item.scale).coerceIn(13f, 22f).sp
                        else -> (11f * item.scale).coerceIn(10f, 15f).sp
                    },
                    fontWeight = FontWeight.Bold,
                )
                if (editMode && item.vk != 0) {
                    Text(shortKeyTag(item.vk), color = Color(0xFF7DD3FC), fontSize = 9.sp)
                }
            }
        }
    }
}

@Composable
fun JoystickControl(
    item: GameHudItem,
    sizeDp: Dp,
    offsetPx: IntOffset,
    editMode: Boolean,
    selected: Boolean,
    onStick: (Float, Float) -> Unit,
) {
    var knobDx by remember { mutableFloatStateOf(0f) }
    var knobDy by remember { mutableFloatStateOf(0f) }
    val active = hypot(knobDx, knobDy) > 4f
    val borderCol = when {
        selected -> Color(0xFF38BDF8)
        active -> Color(0xFF38BDF8).copy(alpha = 0.7f)
        else -> Color.White.copy(alpha = 0.25f)
    }

    Box(
        Modifier
            .offset { offsetPx }
            .size(sizeDp)
            .alpha(item.alpha)
            .clip(CircleShape)
            .background(Color(0xFF141821).copy(alpha = 0.62f))
            .border(if (selected || active) 2.dp else 1.4.dp, borderCol, CircleShape)
            .then(
                if (!editMode) {
                    Modifier.pointerInput(item.id) {
                        awaitEachGesture {
                            val down = awaitFirstDown(requireUnconsumed = false)
                            val radius = (size.width / 2f).coerceAtLeast(1f) * 0.72f
                            fun applyOffset(pos: Offset) {
                                val rawX = pos.x - size.width / 2f
                                val rawY = pos.y - size.height / 2f
                                val dist = hypot(rawX, rawY)
                                val clamped = if (dist > radius) radius / dist else 1f
                                knobDx = rawX * clamped
                                knobDy = rawY * clamped
                                onStick((knobDx / radius).coerceIn(-1f, 1f), (-knobDy / radius).coerceIn(-1f, 1f))
                            }
                            applyOffset(down.position)
                            try {
                                while (true) {
                                    val ev = awaitPointerEvent()
                                    val ch = ev.changes.firstOrNull { it.id == down.id } ?: break
                                    if (!ch.pressed) break
                                    ch.consume()
                                    applyOffset(ch.position)
                                }
                            } finally {
                                knobDx = 0f
                                knobDy = 0f
                                onStick(0f, 0f)
                            }
                        }
                    }
                } else {
                    Modifier
                },
            ),
        contentAlignment = Alignment.Center,
    ) {
        Canvas(Modifier.fillMaxSize()) {
            val c = Offset(size.width / 2f, size.height / 2f)
            val r = size.minDimension * 0.34f
            drawCircle(Color.White.copy(alpha = 0.10f), radius = r, center = c, style = Stroke(1.5.dp.toPx()))
        }
        Box(
            Modifier
                .offset { IntOffset(knobDx.roundToInt(), knobDy.roundToInt()) }
                .size(sizeDp * 0.42f)
                .clip(CircleShape)
                .background(
                    Brush.radialGradient(
                        listOf(
                            Color.White.copy(alpha = if (active) 0.48f else 0.30f),
                            Color.White.copy(alpha = 0.12f),
                        ),
                    ),
                )
                .border(1.5.dp, Color.White.copy(alpha = 0.8f), CircleShape),
        )
    }
}
