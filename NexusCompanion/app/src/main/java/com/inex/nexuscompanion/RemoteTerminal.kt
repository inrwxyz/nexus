package com.inex.nexuscompanion

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch

private data class TerminalEntry(val cmd: String, val output: String, val exitCode: Int, val isError: Boolean)

@Composable
fun RemoteTerminalView(net: Net, onExit: () -> Unit) {
    val scope = rememberCoroutineScope()
    var input by remember { mutableStateOf("") }
    var running by remember { mutableStateOf(false) }
    val entries = remember { mutableStateListOf<TerminalEntry>() }

    // Pressing Back or leaving tab immediately exits Terminal & revokes permission
    BackHandler(enabled = true) { onExit() }
    DisposableEffect(Unit) { onDispose { onExit() } }

    fun submit() {
        val cmd = input.trim()
        if (cmd.isEmpty() || running) return
        input = ""
        running = true
        scope.launch {
            val res = net.runTerminal(cmd)
            running = false
            val text = when {
                !res.error.isNullOrBlank() -> res.error
                res.output.isNotBlank() -> res.output
                else -> "(done, exit ${res.exitCode})"
            }
            entries.add(TerminalEntry(cmd, text, res.exitCode, !res.ok || res.exitCode != 0))
            if (!net.remoteTerminalEnabled) onExit()
        }
    }

    Column(
        Modifier
            .fillMaxSize()
            .padding(horizontal = 14.dp, vertical = 6.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        // Header bar
        Row(
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(14.dp))
                .background(Color(0xFF10141C))
                .border(1.dp, Color(0xFF233042), RoundedCornerShape(14.dp))
                .padding(horizontal = 12.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.clip(CircleShape).background(Color(0xFF22C55E)).padding(4.dp))
            Spacer(Modifier.width(8.dp))
            Text(
                "PowerShell Terminal",
                color = Color.White,
                fontFamily = FontFamily.Monospace,
                fontSize = 13.sp,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.weight(1f),
            )
            Box(
                Modifier
                    .clip(RoundedCornerShape(8.dp))
                    .background(Color(0xFF1E2430))
                    .plainClick { entries.clear() }
                    .padding(horizontal = 10.dp, vertical = 5.dp),
            ) {
                Text("Clear", color = Color(0xFF9CA3AF), fontSize = 11.sp, fontWeight = FontWeight.Medium)
            }
            Spacer(Modifier.width(6.dp))
            Box(
                Modifier
                    .clip(RoundedCornerShape(8.dp))
                    .background(Color(0xFF7F1D1D))
                    .plainClick { onExit() }
                    .padding(horizontal = 10.dp, vertical = 5.dp),
            ) {
                Text("Exit", color = Color.White, fontSize = 11.sp, fontWeight = FontWeight.SemiBold)
            }
        }

        // Terminal output console
        LazyColumn(
            Modifier
                .weight(1f)
                .fillMaxWidth()
                .clip(RoundedCornerShape(16.dp))
                .background(Color(0xFF07090D))
                .border(1.dp, Color(0xFF1E2530), RoundedCornerShape(16.dp))
                .padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            if (entries.isEmpty()) {
                item {
                    Text(
                        "Remote PowerShell ready. Any command you run executes on your PC.\nPress Back or Exit to close session.",
                        color = Color(0xFF6B7280),
                        fontFamily = FontFamily.Monospace,
                        fontSize = 12.sp,
                    )
                }
            }
            items(entries) { e ->
                Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
                    Text(
                        "PS > ${e.cmd}",
                        color = Color(0xFF5EEAD4),
                        fontFamily = FontFamily.Monospace,
                        fontSize = 12.sp,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Text(
                        e.output,
                        color = if (e.isError) Color(0xFFFCA5A5) else Color(0xFFE5E7EB),
                        fontFamily = FontFamily.Monospace,
                        fontSize = 11.5.sp,
                    )
                }
            }
        }

        // Command prompt input
        Row(
            Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            OutlinedTextField(
                value = input,
                onValueChange = { input = it },
                placeholder = { Text("PS > command...", fontFamily = FontFamily.Monospace, fontSize = 12.5.sp) },
                singleLine = true,
                textStyle = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 13.sp),
                shape = RoundedCornerShape(14.dp),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                keyboardActions = KeyboardActions(onSend = { submit() }),
                modifier = Modifier.weight(1f),
            )
            Button(
                onClick = { submit() },
                enabled = !running && input.isNotBlank(),
                shape = RoundedCornerShape(14.dp),
                modifier = Modifier.height(52.dp),
            ) {
                Text(if (running) "..." else "Run")
            }
        }
    }
}

@Composable
fun ShutdownTimerDialog(
    onDismiss: () -> Unit,
    onSubmitCommand: (String) -> Unit,
) {
    val cs = MaterialTheme.colorScheme
    var amount by remember { mutableStateOf("10") }
    var unit by remember { mutableStateOf("minutes") }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Shutdown Timer") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text("Schedule PC shutdown or cancel an active shutdown timer.", style = MaterialTheme.typography.bodySmall, color = cs.onSurfaceVariant)
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    listOf(
                        "5s" to "Shutdown in 5 seconds",
                        "5m" to "Shutdown in 5 minutes",
                        "30m" to "Shutdown in 30 minutes",
                        "1h" to "Shutdown in 1 hour",
                    ).forEach { (lbl, cmd) ->
                        Box(
                            Modifier
                                .weight(1f)
                                .clip(RoundedCornerShape(10.dp))
                                .background(cs.surfaceContainerHigh)
                                .plainClick {
                                    onDismiss()
                                    onSubmitCommand(cmd)
                                }
                                .padding(vertical = 8.dp),
                            contentAlignment = Alignment.Center,
                        ) { Text(lbl, style = MaterialTheme.typography.labelMedium, fontWeight = FontWeight.SemiBold) }
                    }
                }
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(
                        value = amount,
                        onValueChange = { v -> amount = v.filter { it.isDigit() }.take(4) },
                        label = { Text("Delay") },
                        singleLine = true,
                        modifier = Modifier.weight(1f),
                    )
                    listOf("seconds" to "Sec", "minutes" to "Min", "hours" to "Hr").forEach { (u, lbl) ->
                        val sel = unit == u
                        Box(
                            Modifier
                                .clip(RoundedCornerShape(10.dp))
                                .background(if (sel) cs.primary else cs.surfaceContainerHigh)
                                .plainClick { unit = u }
                                .padding(horizontal = 10.dp, vertical = 10.dp),
                        ) {
                            Text(lbl, color = if (sel) cs.onPrimary else cs.onSurface, style = MaterialTheme.typography.labelSmall)
                        }
                    }
                }
                Box(
                    Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(10.dp))
                        .background(cs.error.copy(alpha = 0.12f))
                        .plainClick {
                            onDismiss()
                            onSubmitCommand("Cancel Shutdown")
                        }
                        .padding(vertical = 9.dp),
                    contentAlignment = Alignment.Center,
                ) { Text("Cancel Active Shutdown", color = cs.error, style = MaterialTheme.typography.labelMedium, fontWeight = FontWeight.SemiBold) }
            }
        },
        confirmButton = {
            TextButton(onClick = {
                val n = amount.toIntOrNull()?.coerceAtLeast(1) ?: 5
                onDismiss()
                onSubmitCommand("Shutdown in $n $unit")
            }) { Text("Set Timer") }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Close") }
        },
    )
}
