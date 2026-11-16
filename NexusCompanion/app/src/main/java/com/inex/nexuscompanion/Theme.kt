package com.inex.nexuscompanion

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.LocalRippleConfiguration
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext

// Phone er system colour (Material You) follow kore. Dark = dark, light = light.
// Android 12 er niche hole neutral grey fallback (kono purple/green na).
private val FallbackLight = lightColorScheme(
    primary = Color(0xFF232326),
    onPrimary = Color(0xFFFFFFFF),
    background = Color(0xFFFAFAFB),
    onBackground = Color(0xFF1A1A1D),
    surface = Color(0xFFFAFAFB),
    onSurface = Color(0xFF1A1A1D),
    onSurfaceVariant = Color(0xFF76767C),
    surfaceContainerLow = Color(0xFFF1F1F3),
    surfaceContainer = Color(0xFFEBEBED),
    surfaceContainerHigh = Color(0xFFE4E4E7),
    outline = Color(0xFFB9B9BF),
    outlineVariant = Color(0xFFDCDCE0),
)

private val FallbackDark = darkColorScheme(
    primary = Color(0xFFEDEDF0),
    onPrimary = Color(0xFF151518),
    background = Color(0xFF0F0F11),
    onBackground = Color(0xFFEDEDF0),
    surface = Color(0xFF0F0F11),
    onSurface = Color(0xFFEDEDF0),
    onSurfaceVariant = Color(0xFF93939A),
    surfaceContainerLow = Color(0xFF17171A),
    surfaceContainer = Color(0xFF1D1D21),
    surfaceContainerHigh = Color(0xFF26262A),
    outline = Color(0xFF55555B),
    outlineVariant = Color(0xFF2B2B30),
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NexusTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val ctx = LocalContext.current
    val scheme = when {
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.S ->
            if (dark) dynamicDarkColorScheme(ctx) else dynamicLightColorScheme(ctx)
        dark -> FallbackDark
        else -> FallbackLight
    }
    MaterialTheme(colorScheme = scheme) {
        // Ripple off: kono extra animation/micro-interaction nai.
        CompositionLocalProvider(LocalRippleConfiguration provides null, content = content)
    }
}
